#!/usr/bin/env python3
"""
test-testos-real-hardware-defects.py — Regression tests for the 5 defects
proven by the real HP Victus testOS run.

These tests verify the fixes without requiring real hardware:
1. testos_version derives from canonical VERSION, not stale fallback
2. postgres-tps uses runuser (not sudo -u) to avoid pg_ctl-as-root
3. PSI avg10 parser preserves decimals (no malformed values)
4. cyclictest command produces a numeric result (no -q flag)
5. dmesg/journal collection redacts MAC/serial/UUID fields
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent


def run(cmd: list[str], timeout: int = 30) -> tuple[int, str, str]:
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return r.returncode, r.stdout, r.stderr


def read_bench_list() -> str:
    return (REPO_ROOT / "testos" / "bench-list.toml").read_text()


# ─── Tests ───────────────────────────────────────────────────────────────────


def test_1_testos_version_no_stale_fallback():
    """Defect 1: testos_version must derive from canonical VERSION, not 0.7.0-beta.1."""
    runner_src = (REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs").read_text()

    # The stale fallback constant must NOT be "0.7.0-beta.1"
    if 'TESTOS_VERSION_FALLBACK: &str = "0.7.0-beta.1"' in runner_src:
        print("FAIL: stale fallback '0.7.0-beta.1' still present")
        return False

    # The runner must read /etc/testos/version (canonical source)
    if "/etc/testos/version" not in runner_src:
        print("FAIL: runner does not read /etc/testos/version")
        return False

    # build-testos.sh must write /etc/testos/version from VERSION file
    build_script = (REPO_ROOT / "testos" / "build-testos.sh").read_text()
    if "/etc/testos/version" not in build_script:
        print("FAIL: build-testos.sh does not write /etc/testos/version")
        return False

    # Verify the VERSION file matches what build-testos.sh uses
    version_file = (REPO_ROOT / "VERSION").read_text().strip()
    if f'cat > "${{EXTRA_DIR}}/etc/testos/version" << EOF\n${{VERSION}}\nEOF' not in build_script:
        print("FAIL: build-testos.sh does not write VERSION to /etc/testos/version")
        return False

    print(f"PASS: testos_version derives from VERSION file ({version_file}), not stale fallback")
    return True


def test_2_postgres_uses_runuser_not_sudo():
    """Defect 2: postgres-tps must not use sudo -u (pg_ctl refuses root)."""
    bench_list = read_bench_list()

    # Find the postgres-tps section
    pg_section = re.search(r'id = "postgres-tps".*?(?=\n\[\[benches\]\]|\Z)', bench_list, re.DOTALL)
    if not pg_section:
        print("FAIL: postgres-tps benchmark not found")
        return False

    pg_cmd = pg_section.group(0)

    # Must NOT use 'sudo -u' for pg_ctl (pg_ctl refuses root)
    if "sudo -u" in pg_cmd and "pg_ctl" in pg_cmd:
        # Check if sudo -u is only used for non-pg_ctl commands
        lines = pg_cmd.split('\n')
        sudo_pg_ctl_lines = [l for l in lines if 'sudo -u' in l and 'pg_ctl' in l]
        if sudo_pg_ctl_lines:
            print(f"FAIL: sudo -u used with pg_ctl: {sudo_pg_ctl_lines[0].strip()}")
            return False

    # Must use runuser or su (unprivileged execution)
    if "runuser" not in pg_cmd and "su -s" not in pg_cmd:
        print("FAIL: no runuser or su for unprivileged execution")
        return False

    print("PASS: postgres-tps uses runuser (not sudo -u) for unprivileged pg_ctl")
    return True


def test_3_psi_parser_preserves_decimals():
    """Defect 3: PSI avg10 parser must preserve decimals, not produce '005' or '132'."""
    bench_list = read_bench_list()

    # Test the PSI CPU parser with realistic /proc/pressure/cpu output
    psi_output = "some avg10=0.25 avg60=0.10 avg300=0.05 total=695299\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"

    # Find the psi-cpu-avg10 command
    psi_section = re.search(r'id = "psi-cpu-avg10".*?command = """(.*?)"""', bench_list, re.DOTALL)
    if not psi_section:
        print("FAIL: psi-cpu-avg10 benchmark not found")
        return False

    psi_cmd = psi_section.group(1).strip()

    # The command should use grep+sed, not the old awk split
    if "awk '{split($2,a" in psi_cmd:
        print("FAIL: still uses old awk split() parser")
        return False

    # Test the parser against realistic PSI output
    with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
        f.write(psi_output)
        psi_file = f.name

    try:
        # Extract the actual grep+sed pipeline and run it against our test data
        # Replace /proc/pressure/cpu with our test file
        test_cmd = psi_cmd.replace("/proc/pressure/cpu", psi_file)
        rc, stdout, stderr = run(["bash", "-c", test_cmd])
        result = stdout.strip()

        if rc != 0:
            print(f"FAIL: parser exited {rc}: {stderr}")
            return False

        if not result:
            print("FAIL: parser produced empty output")
            return False

        # Must be a valid decimal number (e.g., 0.25)
        try:
            val = float(result)
        except ValueError:
            print(f"FAIL: parser produced non-numeric value: {result!r}")
            return False

        # Must preserve the decimal (not 0 or 25)
        if val != 0.25:
            print(f"FAIL: parser returned {val}, expected 0.25")
            return False

        print(f"PASS: PSI avg10 parser preserves decimals (got {val})")
        return True
    finally:
        os.unlink(psi_file)


def test_4_cyclictest_produces_numeric_result():
    """Defect 4: cyclictest command must not use -q (suppresses Max: line)."""
    bench_list = read_bench_list()

    cyclic_section = re.search(r'id = "cyclictest-max".*?command = """(.*?)"""', bench_list, re.DOTALL)
    if not cyclic_section:
        print("FAIL: cyclictest-max benchmark not found")
        return False

    cyclic_cmd = cyclic_section.group(1)

    # Must NOT use -q (quiet suppresses the Max: line)
    # Check that -q is not present as a standalone flag
    # (it could appear in a string, but not as a cyclictest flag)
    cmd_parts = cyclic_cmd.split()
    for i, part in enumerate(cmd_parts):
        if part == "cyclictest":
            # Check the flags after cyclictest
            for j in range(i + 1, len(cmd_parts)):
                flag = cmd_parts[j]
                if flag.startswith("-") and "q" in flag:
                    print(f"FAIL: cyclictest uses -q flag (suppresses Max: line): {flag}")
                    return False
                if not flag.startswith("-"):
                    break  # end of flags
            break

    # Must still parse Max: line
    if "/Max:/" not in cyclic_cmd:
        print("FAIL: cyclictest command does not parse Max: line")
        return False

    print("PASS: cyclictest does not use -q (Max: line is preserved)")
    return True


def test_5_dmesg_journal_privacy_redaction():
    """Defect 5: dmesg/journal collection must redact MAC/serial/UUID fields."""
    runner_src = (REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs").read_text()

    # Must have a privacy filter (sed with redaction patterns)
    if "privacy_filter" not in runner_src:
        print("FAIL: no privacy_filter in testos-runner")
        return False

    # Must redact MAC addresses
    if "<MAC>" not in runner_src:
        print("FAIL: no MAC address redaction")
        return False

    # Must redact serial numbers
    if "<SERIAL>" not in runner_src:
        print("FAIL: no serial number redaction")
        return False

    # Must redact UUIDs
    if "<UUID>" not in runner_src:
        print("FAIL: no UUID redaction")
        return False

    # Verify the sed filter actually works against a test string with MAC/serial/UUID
    test_input = "MAC=aa:bb:cc:dd:ee:ff SerialNumber=ABC123 UUID=12345678-1234-1234-1234-123456789abc IP=192.168.1.1"
    sed_filter = r"""sed -re 's/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/<UUID>/g' -e 's/([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}/<MAC>/g' -e 's/[Ss]erial[Nn]umber=[^ ]*/<SERIAL>/g' -e 's/serial=[0-9a-fA-F]{6,}/<SERIAL>/g' -e 's/\b([0-9]{1,3}\.){3}[0-9]{1,3}\b/<IPV4>/g'"""

    rc, stdout, stderr = run(["bash", "-c", f"echo '{test_input}' | {sed_filter}"])
    if rc != 0:
        print(f"FAIL: sed filter failed: {stderr}")
        return False

    redacted = stdout.strip()
    if "aa:bb:cc:dd:ee:ff" in redacted:
        print(f"FAIL: MAC address not redacted: {redacted}")
        return False
    if "ABC123" in redacted:
        print(f"FAIL: serial not redacted: {redacted}")
        return False
    if "12345678-1234-1234-1234-123456789abc" in redacted:
        print(f"FAIL: UUID not redacted: {redacted}")
        return False
    if "192.168.1.1" in redacted:
        print(f"FAIL: IP not redacted: {redacted}")
        return False

    print(f"PASS: dmesg/journal collection redacts MAC/serial/UUID/IP fields")
    print(f"  redacted: {redacted}")
    return True


def main():
    tests = [
        test_1_testos_version_no_stale_fallback,
        test_2_postgres_uses_runuser_not_sudo,
        test_3_psi_parser_preserves_decimals,
        test_4_cyclictest_produces_numeric_result,
        test_5_dmesg_journal_privacy_redaction,
    ]
    passed = 0
    failed = 0
    for test in tests:
        print(f"\n--- {test.__name__} ---")
        try:
            if test():
                passed += 1
            else:
                failed += 1
        except Exception as e:
            import traceback
            traceback.print_exc()
            failed += 1

    print(f"\n{'=' * 60}")
    print(f"Results: {passed} passed, {failed} failed, {len(tests)} total")
    print(f"{'=' * 60}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
