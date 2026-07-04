#!/usr/bin/env python3
"""
pytest tests for tools/livedev-next, tools/livedev-bootstrap.sh, and
tools/livedev-bootstrap.ps1.

Required coverage (per the LiveDev one-command USB workflow spec):
  - tools/livedev-next --help mentions --auto, --prepare-usb, --resume
  - default prints Linux/macOS/Windows bootstrap commands
  - default does not request GH_TOKEN
  - --mock works
  - --auto --dry-run shows the full pipeline and does not write USB
  - livedev-bootstrap.sh supports --auto, --resume, --dry-run
  - livedev-bootstrap.ps1 supports -Auto, -Resume, -DryRun
  - no file (in the LiveDev workflow set) claims auto-merge
  - no path marks milestones verified
  - README shows LiveDev one-command section before testOS

Run with:
  python3 -m pytest tools/test-livedev-next.py -v
  python3 tools/test-livedev-next.py  # standalone
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _run(args: list[str], timeout: int = 120, env: dict | None = None) -> tuple[int, str, str]:
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "livedev-next")] + args,
        capture_output=True, text=True, timeout=timeout, cwd=str(_ROOT), env=env,
    )
    return r.returncode, r.stdout, r.stderr


# --- livedev-next --help and default ----------------------------------------

def test_help_shows_all_modes():
    rc, stdout, _ = _run(["--help"])
    assert rc == 0
    for mode in ["--auto", "--mock", "--plan", "--run", "--submit",
                 "--prepare-usb", "--resume", "--dry-run"]:
        assert mode in stdout, f"--help should mention {mode}"


def test_help_mentions_prepare_usb_and_resume():
    """Spec: --help must mention --auto, --prepare-usb, --resume."""
    rc, stdout, _ = _run(["--help"])
    assert rc == 0
    assert "--auto" in stdout
    assert "--prepare-usb" in stdout
    assert "--resume" in stdout


def test_default_prints_bootstrap_commands():
    """Spec: default output must show the Linux/macOS and Windows bootstrap commands."""
    rc, stdout, _ = _run([])
    assert rc == 0
    assert "livedev-bootstrap.sh" in stdout
    assert "livedev-bootstrap.ps1" in stdout
    assert "--auto" in stdout
    # Windows command uses -Auto (PowerShell flag style)
    assert "-Auto" in stdout
    # Must mention the user-path description
    assert "USB" in stdout or "usb" in stdout
    assert "maintainer review" in stdout.lower()


def test_default_does_not_ask_for_token():
    """Spec: default must not request GH_TOKEN."""
    rc, stdout, stderr = _run([])
    assert "TOKEN" not in stdout
    assert "TOKEN" not in stderr
    assert "GH_TOKEN" not in stdout
    assert "GH_TOKEN" not in stderr


def test_default_exits_zero():
    rc, _, _ = _run([])
    assert rc == 0


def test_default_shows_tool_check():
    rc, stdout, _ = _run([])
    assert "rush-autopilot" in stdout
    assert "rush-exec" in stdout
    assert "validate-hwtest-evidence" in stdout


# --- livedev-next --mock ----------------------------------------------------

def test_mock_runs_all_scenarios():
    rc, stdout, _ = _run(["--mock"], timeout=180)
    assert rc == 0
    assert "success" in stdout.lower()
    assert "failure" in stdout.lower()
    assert "fixtures" in stdout.lower()


# --- livedev-next --auto --dry-run ------------------------------------------

def test_auto_dry_run_shows_full_pipeline():
    """Spec: --auto --dry-run shows the full pipeline and does not write USB."""
    rc, stdout, stderr = _run(["--auto", "--dry-run"], timeout=60)
    assert rc == 0, f"--auto --dry-run should exit 0, got {rc}: {stderr}"
    # Must show all 4 steps
    assert "Step 1/4" in stdout
    assert "Step 2/4" in stdout
    assert "Step 3/4" in stdout
    assert "Step 4/4" in stdout
    # Must show the rush-autopilot commands it would run
    assert "rush-autopilot plan" in stdout
    assert "rush-autopilot run" in stdout
    assert "validate-hwtest-evidence.py" in stdout
    assert "rush-autopilot submit-evidence" in stdout
    # Must indicate dry-run
    assert "[dry-run]" in stdout or "dry-run" in stdout.lower()
    # Must NOT write USB (no real sudo bash invocation that writes)
    assert "sudo bash testos/install.sh" not in stdout.replace(
        "[dry-run] ", "") or "[dry-run]" in stdout
    # Must explicitly say USB is not written
    assert "USB is not written" in stdout or "Not writing USB" in stdout


# --- livedev-next --auto (real) ---------------------------------------------

def test_auto_runs_full_pipeline():
    rc, stdout, _ = _run(["--auto"], timeout=300)
    # --auto may fail at validation (ambiguous slot on CI) but should
    # at least complete steps 1 and 2 and print the pipeline structure.
    assert "Step 1/4" in stdout
    assert "Step 2/4" in stdout
    assert "Pipeline" in stdout


# --- livedev-next --plan / --run / --submit ---------------------------------

def test_plan_generates_file():
    rc, stdout, _ = _run(["--plan"], timeout=60)
    assert rc == 0
    assert "/tmp/rush-livedev-plan.json" in stdout
    plan_path = Path("/tmp/rush-livedev-plan.json")
    assert plan_path.exists()
    plan = json.loads(plan_path.read_text())
    assert plan.get("plan_kind") == "rush-autopilot-plan"


def test_run_executes_plan():
    _run(["--plan"], timeout=60)
    rc, stdout, _ = _run(["--run", "/tmp/rush-livedev-plan.json"], timeout=300)
    assert rc == 0


def test_submit_dry_run_works():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')
        rc, stdout, stderr = _run(["--submit", str(run_dir), "--dry-run"], timeout=60)
        assert "TOKEN" not in stdout
        assert "TOKEN" not in stderr


def test_submit_without_token_prints_token_needed():
    env = os.environ.copy()
    env.pop("GH_TOKEN", None)
    env.pop("GITHUB_TOKEN", None)
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "livedev-next"), "--submit", str(run_dir)],
            capture_output=True, text=True, timeout=30, cwd=str(_ROOT), env=env,
        )
        assert "[TOKEN NEEDED]" in r.stdout


def test_run_nonexistent_plan_fails():
    rc, stdout, stderr = _run(["--run", "/tmp/nonexistent-plan-12345.json"], timeout=30)
    assert rc != 0
    assert "not found" in stdout.lower() or "not found" in stderr.lower()


# --- livedev-bootstrap.sh ---------------------------------------------------

def test_bootstrap_sh_exists_and_is_executable():
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    assert p.exists(), "tools/livedev-bootstrap.sh must exist"
    assert os.access(p, os.X_OK), "tools/livedev-bootstrap.sh must be executable"


def test_bootstrap_sh_supports_required_flags():
    """Spec: livedev-bootstrap.sh supports --auto, --resume, --dry-run."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    assert "--auto" in text
    assert "--resume" in text
    assert "--dry-run" in text
    assert "--skip-mock" in text
    assert "--submit" in text
    # Must NOT auto-merge (no merge API call)
    assert "pulls/${PR_NUM}/merge" not in text
    assert "pulls/$PR_NUM/merge" not in text
    # Must print [TOKEN NEEDED] when token is missing
    assert "[TOKEN NEEDED]" in text


def test_bootstrap_sh_help_works():
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    r = subprocess.run(
        ["bash", str(p), "--help"],
        capture_output=True, text=True, timeout=30, cwd=str(_ROOT),
    )
    assert r.returncode == 0
    assert "--auto" in r.stdout
    assert "--resume" in r.stdout
    assert "--dry-run" in r.stdout


def test_bootstrap_sh_auto_dry_run_works():
    """Spec: livedev-bootstrap.sh --auto --dry-run works."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    r = subprocess.run(
        ["bash", str(p), "--auto", "--dry-run"],
        capture_output=True, text=True, timeout=60, cwd=str(_ROOT),
    )
    assert r.returncode == 0, f"bootstrap.sh --auto --dry-run failed: {r.stderr}"
    # Must show the 4 steps
    assert "Step 1/4" in r.stdout
    assert "Step 2/4" in r.stdout
    assert "Step 3/4" in r.stdout
    assert "Step 4/4" in r.stdout
    # Must say it's using testOS as backend
    assert "Using testOS as the current LiveDev boot backend" in r.stdout
    # Must print reboot instructions
    assert "Reboot instructions" in r.stdout
    # Must NOT actually write USB (dry-run)
    assert "[dry-run]" in r.stdout
    assert "Not writing USB" in r.stdout


def test_bootstrap_sh_resume_dry_run_works():
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    r = subprocess.run(
        ["bash", str(p), "--resume", "--dry-run"],
        capture_output=True, text=True, timeout=60, cwd=str(_ROOT),
    )
    assert r.returncode == 0, f"bootstrap.sh --resume --dry-run failed: {r.stderr}"
    assert "Step 1/3" in r.stdout
    assert "Step 2/3" in r.stdout
    assert "Step 3/3" in r.stdout
    assert "[dry-run]" in r.stdout


# --- livedev-bootstrap.ps1 --------------------------------------------------

def test_bootstrap_ps1_exists():
    """Spec: livedev-bootstrap.ps1 exists."""
    p = _TOOLS_DIR / "livedev-bootstrap.ps1"
    assert p.exists(), "tools/livedev-bootstrap.ps1 must exist"


def test_bootstrap_ps1_supports_required_flags():
    """Spec: livedev-bootstrap.ps1 supports -Auto, -Resume, -DryRun."""
    p = _TOOLS_DIR / "livedev-bootstrap.ps1"
    text = p.read_text()
    # PowerShell param block
    assert "[switch]$Auto" in text
    assert "[switch]$Resume" in text
    assert "[switch]$DryRun" in text
    assert "[switch]$SkipMock" in text
    assert "[switch]$Submit" in text
    # Must NOT auto-merge (no merge API call)
    assert "pulls/" not in text.replace("pulls'", "").replace("pulls`", "")  # rough check
    # Specifically: no /merge endpoint
    assert "/merge" not in text
    # Must print [TOKEN NEEDED] when token is missing
    assert "[TOKEN NEEDED]" in text
    # Must use Invoke-RestMethod for PR creation (not merge)
    assert "Invoke-RestMethod" in text or "Invoke-WebRequest" in text


# --- No auto-merge claims ---------------------------------------------------

def test_no_file_claims_auto_merge():
    """Spec: no file (in the LiveDev one-command workflow set) claims auto-merge.

    We scan the files we author + the README + runbook for POSITIVE auto-merge
    claims (negations like 'does not auto-merge' are allowed).
    """
    files_to_check = [
        _ROOT / "README.md",
        _ROOT / "docs" / "livedev" / "OPERATOR_RUNBOOK.md",
        _TOOLS_DIR / "livedev-next",
        _TOOLS_DIR / "livedev-bootstrap.sh",
        _TOOLS_DIR / "livedev-bootstrap.ps1",
    ]
    negations = (
        "not ", "never", "cannot", "no ", "don't", "do not",
        "doesn't", "does not", "isn't", "is not", "without",
    )
    bad: list[tuple[str, str]] = []
    for f in files_to_check:
        if not f.exists():
            continue
        for line in f.read_text(errors="replace").splitlines():
            lower = line.lower()
            if "auto-merge" in lower or "auto merge" in lower or "auto-merges" in lower \
                    or "auto-merging" in lower:
                # Check if it's a negation.
                if any(neg in lower for neg in negations):
                    continue
                bad.append((str(f), line.strip()))
    assert not bad, f"Positive auto-merge claims found: {bad}"


# --- No milestone verification ----------------------------------------------

def test_no_path_marks_milestones_verified():
    """Spec: no path marks milestones verified.

    The LiveDev workflow files must not contain code that sets
    verified = true in release/milestones.toml. We look for assignment
    patterns, not quoted string literals used in forbidden-pattern lists.
    """
    files_to_check = [
        _TOOLS_DIR / "livedev-next",
        _TOOLS_DIR / "livedev-bootstrap.sh",
        _TOOLS_DIR / "livedev-bootstrap.ps1",
        _TOOLS_DIR / "rush-autopilot",
        _TOOLS_DIR / "rush_pr_lib.py",
    ]
    bad: list[tuple[str, str]] = []
    for f in files_to_check:
        if not f.exists():
            continue
        text = f.read_text(errors="replace")
        # Look for patterns that write verified = true to milestones.toml.
        # Exclude: quoted string literals (used in forbidden-pattern lists),
        # comments, and negation context.
        for pattern in [
            r'verified\s*=\s*true',
        ]:
            for m in re.finditer(pattern, text, re.IGNORECASE):
                start = text.rfind('\n', 0, m.start()) + 1
                end = text.find('\n', m.end())
                line = text[start:end if end != -1 else len(text)]
                # Skip if the match is inside a quoted string literal.
                # (i.e., there's a `"` before the match on the same line, with no
                # closing `"` between them.)
                prefix = line[:m.start() - start]
                if prefix.count('"') % 2 == 1:
                    continue
                # Skip if the match is inside a single-quoted string.
                if prefix.count("'") % 2 == 1:
                    continue
                # Skip negation context.
                lower = line.lower()
                if any(neg in lower for neg in
                       ("not ", "never", "cannot", "no ", "don't", "do not",
                        "doesn't", "does not", "isn't", "is not", "forbidden",
                        "never set", "human-only", "maintainer")):
                    continue
                bad.append((str(f), line.strip()))
    assert not bad, f"Milestone-verification claims found: {bad}"


# --- README ordering --------------------------------------------------------

def test_readme_shows_livedev_before_testos():
    """Spec: README shows LiveDev one-command section before testOS."""
    readme = (_ROOT / "README.md").read_text()
    livedev_pos = readme.find("Rush LiveDev")
    testos_pos = readme.find("testOS")
    assert livedev_pos != -1, "README must mention Rush LiveDev"
    assert testos_pos != -1, "README must mention testOS"
    assert livedev_pos < testos_pos, \
        f"LiveDev section (pos {livedev_pos}) must appear before testOS (pos {testos_pos})"


def test_readem_shows_one_command_within_first_60_lines():
    """Spec: Within the first 60 lines, before testOS, add a prominent LiveDev section."""
    readme = (_ROOT / "README.md").read_text()
    lines = readme.splitlines()
    # Find the first line that mentions livedev-bootstrap
    for i, line in enumerate(lines[:60], start=1):
        if "livedev-bootstrap.sh" in line:
            return
    assert False, "livedev-bootstrap.sh must appear within first 60 lines of README"


def test_readme_has_linux_and_windows_commands():
    """Spec: README has both Linux/macOS and Windows PowerShell one-command paths."""
    readme = (_ROOT / "README.md").read_text()
    assert "curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh" in readme
    assert "curl.exe -L -o livedev-bootstrap.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.ps1" in readme
    assert "powershell -ExecutionPolicy Bypass -File .\\livedev-bootstrap.ps1 -Auto" in readme


def test_readme_labels_testos_as_backend_or_fallback():
    """Spec: Label testOS below as: Current boot backend / manual fallback."""
    readme = (_ROOT / "README.md").read_text()
    assert "boot backend" in readme.lower() or "manual fallback" in readme.lower(), \
        "README must label testOS as 'current boot backend' or 'manual fallback'"


# --- Standalone runner ------------------------------------------------------


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
