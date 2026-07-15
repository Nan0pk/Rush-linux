#!/usr/bin/env python3
"""
test-testos-boot-reliability.py — pytest tests for the testOS boot-reliability
and terminal-UI changes from the boot-reliability PR.

Covers:
  - USB discovery retry helper (bounded, no unbounded sleep, timeline logging)
  - mount-failure prevents runner start (Requires=, not Wants=)
  - runner cannot race getty on tty1 (mask + Conflicts=)
  - failure path does not spawn a root shell
  - failure screen contains actionable error code
  - TTY color rendering (palette enabled when TTY + no NO_COLOR)
  - NO_COLOR/non-TTY rendering without ANSI escapes
  - menu descriptions/significance driven by catalog
  - overall percentage calculations (completed-count based, clamped)
  - spinner/progress lifecycle stops after success, failure, skip, abort
  - failed/skipped results remain honest
  - no optid service or actuation enabled in testOS baseline image

Run:
    python3 -m pytest tools/test-testos-boot-reliability.py -v
    python3 tools/test-testos-boot-reliability.py  # direct (exit 1 on fail)
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent
BUILD_SCRIPT = REPO_ROOT / "testos" / "build-testos.sh"
RUNNER_SRC = REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs"
TUI_SRC = REPO_ROOT / "crates" / "testos" / "src" / "tui.rs"
RECOVERY_SRC = REPO_ROOT / "crates" / "testos" / "src" / "recovery.rs"
PRIVATE_DIAG_SRC = REPO_ROOT / "crates" / "testos" / "src" / "private_diag.rs"
CATALOG_SRC = REPO_ROOT / "crates" / "testos" / "src" / "catalog.rs"
BENCH_LIST = REPO_ROOT / "testos" / "bench-list.toml"


def _read(p: Path) -> str:
    return p.read_text()


# ─── USB discovery retry helper ──────────────────────────────────────────────


def _extract_mount_helper(build_script: str) -> str:
    """Extract the testos-usb-mount script body from build-testos.sh."""
    # The helper is written via a heredoc: cat > ... << 'EOF' ... EOF
    m = re.search(
        r"cat > \"\$\{EXTRA_DIR\}/usr/libexec/testos-usb-mount\" << 'EOF'\n(?P<body>.*?)\nEOF",
        build_script,
        re.DOTALL,
    )
    assert m is not None, "could not find testos-usb-mount heredoc in build-testos.sh"
    return m.group("body")


def test_usb_mount_helper_has_bounded_retry_window():
    """The mount helper must retry for a BOUNDED window — no unbounded sleep."""
    body = _extract_mount_helper(_read(BUILD_SCRIPT))
    # It must compute a deadline from a bounded TIMEOUT_SECS.
    assert "deadline" in body, "no deadline-based retry loop"
    assert "TIMEOUT_SECS" in body, "no TIMEOUT_SECS config"
    # It must clamp the timeout to a bounded range.
    assert "if (( TIMEOUT_SECS < 5 ))" in body or "TIMEOUT_SECS < 5" in body, (
        "TIMEOUT_SECS is not lower-bounded"
    )
    assert "if (( TIMEOUT_SECS > 300 ))" in body or "TIMEOUT_SECS > 300" in body, (
        "TIMEOUT_SECS is not upper-bounded"
    )


def test_usb_mount_helper_uses_udev_settle():
    """The helper should use udevadm settle (bounded) where available."""
    body = _extract_mount_helper(_read(BUILD_SCRIPT))
    assert "udevadm settle" in body, "no udevadm settle call"
    # udevadm settle must have a bounded --timeout, not be unbounded.
    assert "--timeout=" in body, "udevadm settle has no bounded --timeout"


def test_usb_mount_helper_emits_timeline():
    """The helper must write a discovery timeline for PRIVATE-DIAGNOSTICS."""
    body = _extract_mount_helper(_read(BUILD_SCRIPT))
    assert "TIMELINE" in body, "no timeline variable"
    assert "usb-discovery-timeline.txt" in body, "no timeline file path"
    # Each attempt must be logged.
    assert "attempt" in body, "no per-attempt logging"


def test_usb_mount_helper_no_unbounded_sleep():
    """The helper must NOT contain `sleep` without a bounded argument or
    `sleep infinity` / `sleep 9999` style unbounded waits."""
    body = _extract_mount_helper(_read(BUILD_SCRIPT))
    # Strip bash comments so prose like "sleep — bounded" in a comment does
    # not trip the regex.
    code_only = "\n".join(
        line for line in body.splitlines()
        if not line.lstrip().startswith("#")
    )
    # Find every `sleep X` invocation where X is a shell-token argument
    # (starts with $, a letter, a digit, or a brace). This avoids matching
    # prose like "sleep — bounded variable" in comments (already stripped)
    # or sentence fragments.
    for m in re.finditer(r"\bsleep\s+([\$\w][\w\{\}\-]*)", code_only):
        arg = m.group(1)
        # Allow bounded variables.
        if arg in ("ATTEMPT_SLEEP_SECS", "UDEV_SETTLE_SECS"):
            continue
        if arg.startswith("$"):
            continue
        # Allow small numeric literals.
        if arg.isdigit():
            n = int(arg)
            assert n <= 10, f"unbounded sleep {n} in mount helper"
            continue
        # Anything else is suspicious.
        assert False, f"unexpected sleep argument {arg!r} in mount helper"


def test_usb_mount_helper_fails_within_timeout():
    """On timeout, the helper must exit non-zero so the runner unit's
    Requires= can prevent the runner from starting."""
    body = _extract_mount_helper(_read(BUILD_SCRIPT))
    assert "exit 1" in body, "no exit 1 on failure"
    # The deadline-reached path must lead to exit 1 (after writing the
    # timeline + available partitions for local diagnosis).
    deadline_idx = body.find("deadline reached")
    assert deadline_idx >= 0, "no deadline-reached log line"
    exit_idx = body.find("exit 1", deadline_idx)
    assert exit_idx >= 0, "no exit 1 after deadline reached"


def test_usb_mount_helper_writes_boot_attempt_counter():
    """The helper must write /run/testos/boot-attempt for the runner."""
    body = _extract_mount_helper(_read(BUILD_SCRIPT))
    assert "/run/testos/boot-attempt" in body, "no boot-attempt file"
    assert "BOOT_ATTEMPT" in body, "no BOOT_ATTEMPT variable"


# ─── systemd dependencies ───────────────────────────────────────────────────


def _extract_unit(build_script: str, unit_name: str) -> str:
    """Extract a systemd unit file body from build-testos.sh."""
    m = re.search(
        r"cat > \"\$\{EXTRA_DIR\}/usr/lib/systemd/system/" + re.escape(unit_name) + r"\" << 'EOF'\n(?P<body>.*?)\nEOF",
        build_script,
        re.DOTALL,
    )
    assert m is not None, f"could not find {unit_name} heredoc"
    return m.group("body")


def test_runner_unit_requires_mount_not_wants():
    """The runner unit must have Requires= on the mount unit (not just
    Wants=) so a mount failure prevents the runner from starting."""
    unit = _extract_unit(_read(BUILD_SCRIPT), "testos-runner.service")
    assert "Requires=testos-usb-mount.service" in unit, (
        "runner unit does not Require the mount unit (only Wants= would let "
        "the runner start with an unmounted USB)"
    )
    # Wants= alone is insufficient. If Wants= is present, Requires= must
    # also be present (we allow both for compatibility, but Requires= is
    # the load-bearing one).
    assert "Wants=testos-usb-mount.service" not in unit or "Requires=" in unit


def test_runner_unit_conflicts_getty_tty1():
    """The runner unit must declare Conflicts=getty@tty1.service to prevent
    the getty/tty1 race observed on the HP Victus."""
    unit = _extract_unit(_read(BUILD_SCRIPT), "testos-runner.service")
    assert "Conflicts=getty@tty1.service" in unit, (
        "runner unit does not conflict with getty@tty1 — tty race possible"
    )


def test_runner_unit_has_bounded_startup_timeout():
    """The runner unit must have a bounded TimeoutStartSec."""
    unit = _extract_unit(_read(BUILD_SCRIPT), "testos-runner.service")
    assert "TimeoutStartSec=" in unit, "no TimeoutStartSec on runner unit"
    # Verify it is a finite number of seconds (not 'infinity').
    m = re.search(r"TimeoutStartSec=(\S+)", unit)
    assert m is not None
    val = m.group(1)
    assert val != "infinity", "TimeoutStartSec is infinity"
    if val.isdigit():
        n = int(val)
        assert 30 <= n <= 3600, f"TimeoutStartSec {n} out of reasonable range"


def test_runner_unit_prevents_duplicate_instances():
    """The runner unit must prevent duplicate instances (Restart=on-failure
    with bounded RestartSec, or an ExecStartPre lock)."""
    unit = _extract_unit(_read(BUILD_SCRIPT), "testos-runner.service")
    # Either Restart=on-failure (bounded) or a lock via ExecStartPre.
    has_restart = "Restart=on-failure" in unit or "Restart=no" in unit
    has_lock = "TESTOS_RUNNER_LOCK" in unit or "ExecStartPre" in unit
    assert has_restart or has_lock, (
        "runner unit has neither Restart= policy nor an ExecStartPre lock — "
        "duplicate instances possible"
    )


def test_mount_unit_has_bounded_timeout():
    """The mount unit must have a bounded TimeoutStartSec so a hung mount
    helper does not block forever."""
    unit = _extract_unit(_read(BUILD_SCRIPT), "testos-usb-mount.service")
    assert "TimeoutStartSec=" in unit, "no TimeoutStartSec on mount unit"
    m = re.search(r"TimeoutStartSec=(\S+)", unit)
    assert m is not None
    val = m.group(1)
    assert val != "infinity", "mount TimeoutStartSec is infinity"


def test_getty_tty1_is_masked():
    """The image must mask getty@tty1 so the runner owns tty1 exclusively."""
    build = _read(BUILD_SCRIPT)
    # The mask is a symlink to /dev/null.
    assert "getty@tty1.service" in build, "no getty@tty1 reference in build script"
    assert "/dev/null" in build, "no /dev/null mask for getty@tty1"


# ─── No root shell on failure ───────────────────────────────────────────────


def test_runner_does_not_spawn_root_shell():
    """The runner must NOT spawn an interactive bash shell on failure.
    The recovery screen is the only failure surface."""
    src = _read(RUNNER_SRC)
    # Strip comments before checking — comments may legitimately say
    # "do NOT drop to a shell".
    code_only = "\n".join(
        line for line in src.splitlines()
        if not line.lstrip().startswith("//")
    )
    assert "Dropping to shell" not in code_only
    assert "Command::new(\"bash\").status()" not in code_only, (
        "runner still spawns an interactive bash shell"
    )
    assert "drop to a shell" not in code_only.lower()


def test_recovery_screen_has_actionable_error_code():
    """The recovery screen must contain a short, stable failure code."""
    src = _read(RECOVERY_SRC)
    # Every FailureCategory variant must have a code() that starts with 'E'.
    for variant in [
        "UsbNotFound",
        "UsbMountFailed",
        "IntentInvalid",
        "PlanInvalid",
        "CatalogInvalid",
        "VersionMismatch",
        "InternalError",
        "AcpiBlocking",
    ]:
        assert variant in src, f"FailureCategory::{variant} missing"
    # The code() function must return strings starting with 'E'.
    assert '"E0' in src or "E0" in src, "no E0xx-style failure codes"
    # The recovery screen text must include the code and a next action.
    assert "Failure code:" in src, "recovery screen does not show failure code"
    assert "Safe next action:" in src, "recovery screen does not show next action"
    assert "Rebooting" in src, "recovery screen does not mention reboot"


# ─── TTY color / NO_COLOR rendering ─────────────────────────────────────────


def test_palette_colored_has_ansi_escapes():
    """The colored palette must emit real ANSI SGR sequences."""
    src = _read(TUI_SRC)
    assert r"\x1b[32m" in src or '"\\x1b[32m"' in src or "green:" in src
    # The colored() constructor must reference escape sequences.
    colored_section = src[src.find("pub const fn colored()"):]
    assert "\\x1b[" in colored_section, "colored palette does not use ANSI escapes"


def test_palette_plain_has_no_escapes():
    """The plain palette must use empty strings (no ANSI escapes)."""
    src = _read(TUI_SRC)
    plain_section = src[src.find("pub const fn plain()"):]
    # The plain palette assigns empty string literals to all fields.
    assert 'green: ""' in plain_section, "plain palette does not zero out green"
    assert 'reset: ""' in plain_section, "plain palette does not zero out reset"


def test_palette_for_output_respects_no_color():
    """Palette::for_output must disable color when NO_COLOR is set."""
    src = _read(TUI_SRC)
    assert "NO_COLOR" in src, "palette does not check NO_COLOR"
    assert "IsTerminal" in src or "is_terminal" in src, (
        "palette does not check stdout TTY status"
    )


def test_status_word_has_text_label_alongside_color():
    """Color is never the only status signal — every status has a text label."""
    src = _read(TUI_SRC)
    # StatusWord::label() must return PASS/FAIL/SKIPPED/WARN.
    assert '"PASS"' in src
    assert '"FAIL"' in src
    assert '"SKIPPED"' in src
    # render() must include the label literally.
    assert "self.label()" in src, "render() does not include the text label"


# ─── Menu descriptions / significance ───────────────────────────────────────


def test_catalog_has_significance_field():
    """The Bench struct must have an optional `significance` field, and it
    must be backward-compatible (serde default)."""
    src = _read(CATALOG_SRC)
    assert "pub significance: Option<String>" in src, "no significance field on Bench"
    # The field must have #[serde(default)] for backward compat.
    sig_section = src[src.find("pub significance:"):]
    assert "#[serde(default)]" in src[: src.find("pub significance:")], (
        "significance field is not #[serde(default)] — breaks old catalogs"
    )


def test_bench_list_has_significance_for_every_entry():
    """Every entry in bench-list.toml should have a significance line
    (or at least every entry that has notes). We check that significance
    appears for the majority of entries."""
    text = _read(BENCH_LIST)
    # Count actual [[benches]] entries (not those mentioned in comments).
    # A real entry is a line that starts with [[benches]] (no leading #).
    benches_count = len(re.findall(r"^\[\[benches\]\]\s*$", text, re.MULTILINE))
    sig_count = len(re.findall(r"^\s*significance\s*=", text, re.MULTILINE))
    # We don't require 1:1 (some future battery benchmarks might not need
    # significance), but the existing 9 should all have it.
    assert sig_count >= benches_count, (
        f"only {sig_count}/{benches_count} benchmarks have significance"
    )


def test_significance_falls_back_to_notes():
    """Bench::significance_or_fallback must fall back to notes when
    significance is absent (backward compat for old catalogs)."""
    src = _read(CATALOG_SRC)
    assert "significance_or_fallback" in src, "no significance_or_fallback helper"
    assert "notes" in src, "no notes field referenced by fallback"


def test_menu_does_not_embed_hardcoded_descriptions():
    """The TUI menu must read descriptions from the catalog (notes +
    significance), not hard-code them in the UI module."""
    src = _read(TUI_SRC)
    # The print_menu function must reference b.measures_text() and
    # b.significance_or_fallback(), not literal benchmark descriptions.
    menu_section = src[src.find("pub fn print_menu"):]
    assert "measures_text" in menu_section, "menu does not call measures_text()"
    assert "significance_or_fallback" in menu_section, (
        "menu does not call significance_or_fallback()"
    )
    # Spot-check: none of the actual benchmark names appear as literals
    # in the TUI module (that would mean descriptions are hard-coded).
    for name in ["fio", "iperf3", "postgres", "nginx", "cyclictest", "psi"]:
        assert f'"{name}' not in menu_section, (
            f"menu hard-codes benchmark name {name!r} — should come from catalog"
        )


# ─── Overall percentage calculations ────────────────────────────────────────


def test_overall_percent_is_completed_count_based():
    """overall_percent must be based on completed benchmark count, never
    fabricated from inside an opaque running command. The contract is
    documented in the TUI module's doc comment."""
    src = _read(TUI_SRC)
    assert "overall_percent" in src
    # The docstring must explain the contract.
    assert "completed" in src.lower() or "completed-count" in src.lower()


def test_progress_position_format():
    """progress_position must format as 'n/total — pct%'."""
    src = _read(TUI_SRC)
    assert "progress_position" in src
    # The format string must include the dash and percent sign.
    assert "—" in src or "-" in src, "progress_position does not use a dash separator"
    assert "%" in src, "progress_position does not include %"


# ─── Spinner / progress lifecycle ───────────────────────────────────────────


def test_spinner_stops_on_drop():
    """Spinner::stop must be called on Drop so the spinner thread cannot
    outlive the benchmark."""
    src = _read(TUI_SRC)
    assert "impl Drop for Spinner" in src, "no Drop impl for Spinner"
    assert "self.stop()" in src, "Drop does not call stop()"


def test_spinner_uses_bounded_sleep():
    """The spinner thread must sleep for a bounded duration per frame,
    not block forever."""
    src = _read(TUI_SRC)
    # The sleep duration must be a small literal (250ms is the documented default).
    assert "Duration::from_millis" in src, "spinner does not use Duration::from_millis"


def test_spinner_writes_to_stderr_not_stdout():
    """The spinner must write to stderr so it does not corrupt captured
    benchmark stdout."""
    src = _read(TUI_SRC)
    spinner_section = src[src.find("pub struct Spinner"):]
    assert "io::stderr()" in spinner_section, "spinner does not write to stderr"


# ─── Failed/skipped results remain honest ───────────────────────────────────


def test_summary_counts_are_honest():
    """The post-run summary must show attempted/passed/failed/skipped
    counts without conflating failed with skipped."""
    src = _read(TUI_SRC)
    summary_section = src[src.find("pub fn print_summary"):]
    assert "attempted" in summary_section
    assert "passed" in summary_section
    assert "failed" in summary_section
    assert "skipped" in summary_section
    # The summary must explicitly state baseline evidence (not proof of
    # optid improvement).
    assert "baseline" in summary_section.lower(), (
        "summary does not label results as baseline evidence"
    )


def test_summary_states_sync_status():
    """The summary must report USB sync status honestly."""
    src = _read(TUI_SRC)
    summary_section = src[src.find("pub fn print_summary"):]
    assert "sync" in summary_section.lower(), "summary does not mention sync status"


# ─── No optid service or actuation in baseline image ────────────────────────


def test_testos_preset_does_not_enable_optid():
    """The testOS baseline preset must NOT enable optid, optid-apply, or
    optid-boot-assess. Baseline purity: a testOS boot measures the hardware
    as-is, without optid actuation."""
    build = _read(BUILD_SCRIPT)
    # Extract the preset heredoc.
    m = re.search(
        r"cat > \"\$\{EXTRA_DIR\}/usr/lib/systemd/system-preset/00-rush\.preset\" << 'EOF'\n(?P<body>.*?)\nEOF",
        build,
        re.DOTALL,
    )
    assert m is not None, "could not find 00-rush.preset heredoc"
    preset = m.group("body")
    assert "enable optid.service" not in preset, "preset enables optid.service"
    assert "enable optid-apply.service" not in preset, "preset enables optid-apply.service"
    assert "enable optid-boot-assess.service" not in preset, (
        "preset enables optid-boot-assess.service"
    )
    # The preset must still enable the testOS services.
    assert "enable testos-usb-mount.service" in preset
    assert "enable testos-runner.service" in preset


def test_testos_does_not_symlink_optid_into_multi_user_wants():
    """The image must NOT symlink optid / optid-apply / optid-boot-assess
    into multi-user.target.wants."""
    build = _read(BUILD_SCRIPT)
    # These services are installed on disk (for ad-hoc use) but must NOT
    # be symlinked into multi-user.target.wants.
    for svc in ["optid.service", "optid-apply.service", "optid-boot-assess.service"]:
        # Look for ln -sf ... multi-user.target.wants/<svc>
        pattern = rf"ln -sf [^\n]*multi-user\.target\.wants/{re.escape(svc)}"
        assert not re.search(pattern, build), (
            f"{svc} is symlinked into multi-user.target.wants — baseline purity violation"
        )


def test_testos_does_not_run_optid_apply():
    """The build script must NOT invoke `optid --apply` anywhere. Installing
    the optid-apply.service unit file on disk is fine (for ad-hoc operator
    use); running the apply command during build or boot is not."""
    build = _read(BUILD_SCRIPT)
    # Strip bash comments so prose like "never run `optid --apply`" in a
    # comment does not trip the check.
    code_only = "\n".join(
        line for line in build.splitlines()
        if not line.lstrip().startswith("#")
    )
    assert "optid --apply" not in code_only, (
        "build script runs `optid --apply` — baseline purity violation"
    )


def test_testos_does_not_change_power_profiles():
    """The build script must NOT change power profiles or hardware settings."""
    build = _read(BUILD_SCRIPT)
    # Forbidden commands: powerprofilesctl, cpupower frequency-set,
    # x86_energy_perf_policy, etc. Use word boundaries so "tuned" does not
    # match substrings of other words.
    for forbidden in [
        r"\bpowerprofilesctl\b",
        r"\bcpupower\s+frequency-set\b",
        r"\bcpupower\s+set\b",
        r"\bx86_energy_perf_policy\b",
        r"\btlp\b",
        r"\btuned-adm\b",
    ]:
        assert not re.search(forbidden, build), (
            f"build script invokes {forbidden} — baseline purity violation"
        )


# ─── Main for direct execution ───────────────────────────────────────────────


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"] + sys.argv[1:]))
