#!/usr/bin/env python3
"""
pytest tests for tools/rush-autopilot (the deterministic planner).

Tests the 8 required scenarios:
  1. laptop generates battery + mixed-load plan
  2. desktop does not claim laptop evidence
  3. no open hardware gates exits cleanly
  4. ambiguous slot is reported
  5. missing milestone file fails clearly
  6. already satisfied evidence prevents duplicate required work
  7. generated plan validates against schema
  8. dry-run has no destructive action

Plus bonus tests for determinism and the safety floor.

Run with:
  python3 -m pytest tools/test-rush-autopilot.py -v
  # or
  python3 tools/test-rush-autopilot.py  # standalone
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _load_module(name: str, path: Path):
    # For files without a .py extension, we need to specify the loader explicitly.
    # The module must also be registered in sys.modules before exec so that
    # dataclass decorators can resolve the module's namespace.
    import importlib.machinery
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


ap = _load_module("rush_autopilot", _TOOLS_DIR / "rush-autopilot")


# ─── Fake sysfs fixtures ─────────────────────────────────────────────────────


def _make_fake_sysfs_laptop(tmpdir: Path) -> Path:
    """Create a fake sysfs/proc tree that looks like a laptop."""
    sys_root = tmpdir / "fake-sys-laptop"
    sys_base = sys_root / "sys"
    proc_base = sys_root / "proc"

    # Battery.
    bat = sys_base / "class" / "power_supply" / "BAT0"
    bat.mkdir(parents=True)
    (bat / "status").write_text("Discharging\n")
    (bat / "capacity").write_text("75\n")
    (bat / "energy_full_design").write_text("48000000\n")

    # AC (offline — on battery).
    ac = sys_base / "class" / "power_supply" / "AC" / "online"
    ac.parent.mkdir(parents=True)
    ac.write_text("0\n")

    # platform_profile.
    pp = sys_base / "firmware" / "acpi" / "platform_profile"
    pp.parent.mkdir(parents=True)
    pp.write_text("balanced\n")
    (sys_base / "firmware" / "acpi" / "platform_profile_choices").write_text("low-power balanced performance\n")

    # cpufreq.
    cpu = sys_base / "devices" / "system" / "cpu" / "cpu0" / "cpufreq"
    cpu.mkdir(parents=True)
    (cpu / "scaling_driver").write_text("intel_pstate\n")
    (cpu / "energy_performance_preference").write_text("balance_performance\n")
    (cpu / "energy_performance_available_preferences").write_text("default performance balance_performance power\n")

    # RAPL.
    rapl = sys_base / "class" / "powercap" / "intel-rapl:0"
    rapl.mkdir(parents=True)
    (rapl / "name").write_text("package-0\n")

    # CPU info.
    (proc_base).mkdir(parents=True, exist_ok=True)
    (proc_base / "cpuinfo").write_text(
        "processor\t: 0\n"
        "vendor_id\t: GenuineIntel\n"
        "model name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
        "processor\t: 1\n"
        "model name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
        "processor\t: 2\n"
        "model name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
        "processor\t: 3\n"
        "model name\t: Intel(R) Core(TM) i7-8650U CPU @ 1.90GHz\n"
    )
    (proc_base / "sys" / "kernel" / "osrelease").parent.mkdir(parents=True, exist_ok=True)
    (proc_base / "sys" / "kernel" / "osrelease").write_text("6.1.0-test-laptop\n")

    # DMI.
    dmi = sys_base / "class" / "dmi" / "id"
    dmi.mkdir(parents=True)
    (dmi / "board_vendor").write_text("TestVendor\n")
    (dmi / "board_name").write_text("LaptopBoard\n")

    return sys_root


def _make_fake_sysfs_desktop(tmpdir: Path) -> Path:
    """Create a fake sysfs/proc tree that looks like a desktop (no battery, AC online)."""
    sys_root = tmpdir / "fake-sys-desktop"
    sys_base = sys_root / "sys"
    proc_base = sys_root / "proc"

    # No battery directory.
    # AC online.
    ac = sys_base / "class" / "power_supply" / "AC" / "online"
    ac.parent.mkdir(parents=True)
    ac.write_text("1\n")

    # platform_profile.
    pp = sys_base / "firmware" / "acpi" / "platform_profile"
    pp.parent.mkdir(parents=True)
    pp.write_text("performance\n")

    # cpufreq.
    cpu = sys_base / "devices" / "system" / "cpu" / "cpu0" / "cpufreq"
    cpu.mkdir(parents=True)
    (cpu / "scaling_driver").write_text("intel_pstate\n")
    (cpu / "energy_performance_preference").write_text("performance\n")

    # CPU info — 8 cores.
    (proc_base).mkdir(parents=True, exist_ok=True)
    lines = []
    for i in range(8):
        lines.append(f"processor\t: {i}")
        lines.append("model name\t: Intel(R) Core(TM) i9-9900K CPU @ 3.60GHz")
    (proc_base / "cpuinfo").write_text("\n".join(lines) + "\n")

    # DMI.
    dmi = sys_base / "class" / "dmi" / "id"
    dmi.mkdir(parents=True)
    (dmi / "board_vendor").write_text("DesktopVendor\n")
    (dmi / "board_name").write_text("DesktopBoard\n")

    return sys_root


def _make_fake_sysfs_ambiguous(tmpdir: Path) -> Path:
    """Create a fake sysfs/proc tree with no battery and no AC info — ambiguous slot."""
    sys_root = tmpdir / "fake-sys-ambiguous"
    sys_base = sys_root / "sys"
    proc_base = sys_root / "proc"
    (proc_base).mkdir(parents=True, exist_ok=True)
    (proc_base / "cpuinfo").write_text(
        "processor\t: 0\nmodel name\t: Test CPU\n"
    )
    return sys_root


def _make_fake_repo(tmpdir: Path, with_milestones: bool = True, with_evidence: bool = False,
                     evidence_count: int = 0) -> Path:
    """Create a minimal fake repo with VERSION + milestones.toml."""
    repo = tmpdir / "fake-repo"
    repo.mkdir()

    (repo / "VERSION").write_text("0.7.0-beta.1\n")

    if with_milestones:
        rel = repo / "release" / "evidence" / "host-bench"
        rel.mkdir(parents=True)
        (rel / "_TEMPLATE").mkdir()

        milestones = repo / "release" / "milestones.toml"
        milestones.write_text(
            '[project]\n'
            'current_version = "0.7.0-beta.1"\n\n'
            '[[milestone]]\n'
            'version = "0.6.0-beta.1"\n'
            'channel = "beta"\n'
            'name = "Hardware-Aware optid"\n'
            'status = "in-progress"\n'
            'exit_criteria = [\n'
            '  "unsupported knobs are skipped with reasons",\n'
            '  "mixed-load responsiveness improves on two machines",\n'
            '  "battery behavior matches or improves mainstream defaults",\n'
            '  "no unsafe write occurs outside allowlisted paths",\n'
            ']\n\n'
            '[[milestone.criteria_status]]\n'
            'criterion = "mixed-load responsiveness improves on two machines"\n'
            'verified = false\n'
            'transcript = ""\n'
            'note = "PENDING PHASE D (hardware gate). Requires two nominated reference machines."\n\n'
            '[[milestone.criteria_status]]\n'
            'criterion = "battery behavior matches or improves mainstream defaults"\n'
            'verified = false\n'
            'transcript = ""\n'
            'note = "PENDING PHASE D (hardware gate). Requires the battery-equipped laptop."\n'
        )

    if with_evidence:
        for i in range(evidence_count):
            bundle = repo / "release" / "evidence" / "host-bench" / f"2026-07-0{i}-machine{i}"
            bundle.mkdir(parents=True)
            (bundle / "hwtest-manifest.json").write_text("{}\n")

    return repo


def _run_planner(*args: str) -> subprocess.CompletedProcess:
    """Run rush-autopilot and return the CompletedProcess."""
    return subprocess.run(
        ["python3", str(_TOOLS_DIR / "rush-autopilot"), *args],
        capture_output=True,
        text=True,
        timeout=30,
    )


# ─── Test 1: laptop generates battery + mixed-load plan ──────────────────────


def test_laptop_generates_battery_and_mixed_load_plan():
    """A laptop (battery present) generates a plan with battery prompts + mixed-load runs."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        hw = ap.detect_hardware(fake_sys)
        assert hw.battery_present, "laptop fake sysfs should have a battery"
        assert hw.battery_design_uwh > 0

        plan = ap.generate_plan(repo, hw, slot_override=None)

        # Should detect laptop slot.
        assert plan.hardware_slot == "laptop"
        assert plan.slot_confidence == "high"

        # Should have open criteria.
        assert len(plan.open_criteria) > 0
        criteria_texts = [c["criterion"] for c in plan.open_criteria]
        assert any("mixed-load" in c for c in criteria_texts)

        # Should have physical prompts for battery (unplug AC).
        actions = [s.action for s in plan.steps if s.kind == "physical-prompt"]
        assert any("battery" in a.lower() or "unplug" in a.lower() for a in actions), \
            f"laptop plan should have a battery/unplug prompt; actions were: {actions}"

        # Should have baseline + optid paired runs.
        argvs = [s.argv for s in plan.steps if s.kind == "command" and s.argv]
        rushbench_argv = [a for a in argvs if any("rushbench" in str(x) for x in a)]
        assert len(rushbench_argv) >= 2, "should have baseline + optid rushbench runs"

        # Should have a validation step.
        val_steps = [s for s in plan.steps if s.kind == "validation"]
        assert len(val_steps) >= 1


# ─── Test 2: desktop does not claim laptop evidence ──────────────────────────


def test_desktop_does_not_claim_laptop_evidence():
    """A desktop (no battery) does not generate battery prompts."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_desktop(tmpdir)

        hw = ap.detect_hardware(fake_sys)
        assert not hw.battery_present
        assert hw.ac_online is True

        plan = ap.generate_plan(repo, hw, slot_override=None)

        assert plan.hardware_slot == "desktop"

        # Desktop should NOT have battery/unplug prompts.
        actions = [s.action for s in plan.steps if s.kind == "physical-prompt"]
        for a in actions:
            assert "battery" not in a.lower() and "unplug" not in a.lower(), \
                f"desktop plan should not have battery prompts; found: {a}"

        # Desktop should not claim Criterion 3 (battery behavior).
        for c in plan.open_criteria:
            if "battery" in c["criterion"].lower():
                # Criterion 3 should be filtered out for desktop.
                pytest_fail(f"desktop plan should not include battery criterion: {c}")


def pytest_fail(msg: str):
    import pytest
    pytest.fail(msg)


# ─── Test 3: no open hardware gates exits cleanly ────────────────────────────


def test_no_open_hardware_gates_exits_cleanly():
    """When there are no open hardware-gated criteria, the plan exits cleanly."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        # Overwrite milestones with a milestone that has all criteria verified.
        (repo / "release" / "milestones.toml").write_text(
            '[project]\n'
            'current_version = "0.7.0-beta.1"\n\n'
            '[[milestone]]\n'
            'version = "0.6.0-beta.1"\n'
            'channel = "beta"\n'
            'name = "Hardware-Aware optid"\n'
            'status = "complete"\n'
            'exit_criteria = ["mixed-load responsiveness improves on two machines"]\n\n'
            '[[milestone.criteria_status]]\n'
            'criterion = "mixed-load responsiveness improves on two machines"\n'
            'verified = true\n'
            'transcript = "release/evidence/host-bench/2026-01-01-machine1/VERDICT.md"\n'
            'note = "Closed."\n'
        )
        # Create the transcript file so the plan can find it.
        (repo / "release" / "evidence" / "host-bench" / "2026-01-01-machine1").mkdir(parents=True)
        (repo / "release" / "evidence" / "host-bench" / "2026-01-01-machine1" / "VERDICT.md").write_text("PASS\n")

        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        hw = ap.detect_hardware(fake_sys)

        plan = ap.generate_plan(repo, hw)

        # No open criteria.
        assert len(plan.open_criteria) == 0
        # Plan should still have steps (session start + finish) but no benchmark runs.
        rushbench_steps = [s for s in plan.steps if s.argv and any("rushbench" in str(a) for a in s.argv)]
        assert len(rushbench_steps) == 0, "no benchmark runs when no open criteria"


# ─── Test 4: ambiguous slot is reported ──────────────────────────────────────


def test_ambiguous_slot_is_reported():
    """When hardware detection can't determine the slot, it's reported as ambiguous."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_ambiguous(tmpdir)

        hw = ap.detect_hardware(fake_sys)
        slot, confidence, ambiguities = hw.slot()

        assert slot == "ambiguous"
        assert confidence == "low"
        assert len(ambiguities) > 0, "ambiguous slot should have ambiguity explanations"

        plan = ap.generate_plan(repo, hw)
        assert plan.hardware_slot == "ambiguous"
        assert len(plan.ambiguities) > 0


# ─── Test 5: missing milestone file fails clearly ────────────────────────────


def test_missing_milestone_file_fails_clearly():
    """When release/milestones.toml is missing, the planner fails with a clear error."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir, with_milestones=False)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        # The planner should raise FileNotFoundError.
        try:
            ap.generate_plan(repo, ap.detect_hardware(fake_sys))
            assert False, "should have raised FileNotFoundError"
        except FileNotFoundError as e:
            assert "milestones" in str(e).lower(), f"error should mention milestones: {e}"

        # CLI should exit 1.
        r = _run_planner("plan", "--dry-run", "--repo", str(repo), "--fake-sys", str(fake_sys))
        assert r.returncode == 1, f"should exit 1, got {r.returncode}"
        assert "milestones" in r.stderr.lower()


# ─── Test 6: already satisfied evidence prevents duplicate required work ─────


def test_already_satisfied_evidence_prevents_duplicate_work():
    """When evidence already exists, the planner doesn't generate duplicate work."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        # Create a repo with existing evidence bundles.
        repo = _make_fake_repo(tmpdir, with_evidence=True, evidence_count=2)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        hw = ap.detect_hardware(fake_sys)
        plan = ap.generate_plan(repo, hw)

        # The "both slots" criterion requires 2 bundles; we have 2, so it's satisfied.
        # The plan should have no open criteria (or fewer than without evidence).
        existing = ap.find_existing_evidence(repo)
        assert len(existing) == 2, f"should find 2 existing bundles, got {existing}"

        # The mixed-load criterion requires both slots (requires_slot=""). With 2 bundles,
        # is_evidence_satisfied returns True, so it should be filtered out.
        criteria = [c for c in plan.open_criteria if "mixed-load" in c["criterion"]]
        assert len(criteria) == 0, \
            f"mixed-load criterion should be satisfied by 2 existing bundles; got: {criteria}"


# ─── Test 7: generated plan validates against schema ─────────────────────────


def test_generated_plan_validates_against_schema():
    """The generated plan is valid JSON and has all required fields."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        hw = ap.detect_hardware(fake_sys)
        plan = ap.generate_plan(repo, hw)

        plan_dict = plan.to_dict()

        # Required top-level fields.
        for field in ["schema_version", "plan_kind", "generated_at", "source_version",
                       "source_commit", "repo_root", "hardware_slot", "slot_confidence",
                       "ambiguities", "open_criteria", "existing_evidence", "steps"]:
            assert field in plan_dict, f"plan missing required field: {field}"

        assert plan_dict["schema_version"] == 1
        assert plan_dict["plan_kind"] == "rush-autopilot-plan"
        assert plan_dict["source_version"] == "0.7.0-beta.1"
        # source_commit is "unknown" when the repo is not a git repo (fake repo),
        # or a 40-char SHA when it is. Both are valid for this test.
        assert plan_dict["source_commit"] in ("unknown",) or len(plan_dict["source_commit"]) == 40

        # Each step has required fields.
        for step in plan_dict["steps"]:
            for field in ["seq", "kind", "default", "reason", "rollback"]:
                assert field in step, f"step missing required field: {field}"
            assert step["default"] in ("proceed", "skip", "ask", "abort", "wait")
            assert step["kind"] in ("command", "physical-prompt", "validation")

        # The plan should be JSON-serializable (it already is, but verify).
        json.dumps(plan_dict)


# ─── Test 8: dry-run has no destructive action ───────────────────────────────


def test_dry_run_has_no_destructive_action():
    """A dry-run plan must not have any destructive or final-approval actions."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        hw = ap.detect_hardware(fake_sys)
        plan = ap.generate_plan(repo, hw, dry_run=True)

        assert plan.dry_run is True

        # Safety floor: no step has default=proceed for destructive/final-approval.
        violations = ap.check_safety_floor(plan)
        assert violations == [], f"safety floor violations: {violations}"

        # No destructive patterns in any step's argv or action.
        for step in plan.steps:
            text = " ".join(step.argv or []) + " " + (step.action or "")
            text_lower = text.lower()
            for pat in ap.DESTRUCTIVE_PATTERNS:
                assert pat not in text_lower, \
                    f"step {step.seq} contains destructive pattern {pat!r}: {text!r}"
            for pat in ap.FINAL_APPROVAL_PATTERNS:
                assert pat not in text_lower, \
                    f"step {step.seq} contains final-approval pattern {pat!r}: {text!r}"


# ─── Bonus: determinism ──────────────────────────────────────────────────────


def test_plan_is_deterministic():
    """The same inputs always produce the same plan."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        hw = ap.detect_hardware(fake_sys)
        plan1 = ap.generate_plan(repo, hw, generated_at="2026-01-01T00:00:00Z")
        plan2 = ap.generate_plan(repo, hw, generated_at="2026-01-01T00:00:00Z")

        # Plans should be byte-identical (same JSON).
        j1 = json.dumps(plan1.to_dict(), sort_keys=True)
        j2 = json.dumps(plan2.to_dict(), sort_keys=True)
        assert j1 == j2, "same inputs should produce identical plans"


# ─── Bonus: safety floor for all slot types ──────────────────────────────────


def test_safety_floor_holds_for_all_slots():
    """The safety floor (no proceed for destructive/final-approval) holds for all slots."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)

        for fake_sys_fn in (_make_fake_sysfs_laptop, _make_fake_sysfs_desktop, _make_fake_sysfs_ambiguous):
            fake_sys = fake_sys_fn(tmpdir)
            hw = ap.detect_hardware(fake_sys)
            plan = ap.generate_plan(repo, hw)
            violations = ap.check_safety_floor(plan)
            assert violations == [], \
                f"safety floor violated for {fake_sys_fn.__name__}: {violations}"


# ─── Bonus: CLI invocation ───────────────────────────────────────────────────


def test_cli_plan_laptop():
    """The CLI produces a valid plan for a laptop."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)

        r = _run_planner("plan", "--dry-run", "--repo", str(repo), "--fake-sys", str(fake_sys))
        assert r.returncode == 0, f"CLI should exit 0, got {r.returncode}\n{r.stderr}"

        plan = json.loads(r.stdout)
        assert plan["hardware_slot"] == "laptop"
        assert plan["schema_version"] == 1


def test_cli_plan_slot_override():
    """The CLI --slot flag overrides the detected slot."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_desktop(tmpdir)  # desktop hardware

        # Override to laptop.
        r = _run_planner("plan", "--dry-run", "--slot", "laptop", "--repo", str(repo), "--fake-sys", str(fake_sys))
        assert r.returncode == 0
        plan = json.loads(r.stdout)
        assert plan["hardware_slot"] == "laptop"
        # Should report the ambiguity (overridden to laptop but no battery).
        assert len(plan["ambiguities"]) > 0


def test_cli_plan_output_to_file():
    """The --output flag writes the plan to a file."""
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        repo = _make_fake_repo(tmpdir)
        fake_sys = _make_fake_sysfs_laptop(tmpdir)
        out_path = tmpdir / "plan.json"

        r = _run_planner("plan", "--dry-run", "--output", str(out_path), "--repo", str(repo), "--fake-sys", str(fake_sys))
        assert r.returncode == 0
        assert out_path.exists(), "plan file should be created"

        plan = json.loads(out_path.read_text())
        assert plan["hardware_slot"] == "laptop"


# ─── Standalone runner ───────────────────────────────────────────────────────


def _run_all_tests() -> int:
    test_funcs = [
        (name, obj)
        for name, obj in sorted(globals().items())
        if name.startswith("test_") and callable(obj)
    ]
    passed = 0
    failed = 0
    for name, func in test_funcs:
        try:
            func()
            print(f"  PASS {name}")
            passed += 1
        except Exception as e:
            print(f"  FAIL {name}: {e}")
            import traceback
            traceback.print_exc()
            failed += 1
    print(f"\n{passed} passed, {failed} failed, {passed + failed} total")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(_run_all_tests())
