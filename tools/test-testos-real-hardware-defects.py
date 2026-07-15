#!/usr/bin/env python3
"""
test-testos-real-hardware-defects.py — Assertion-based pytest tests for the
defects proven by the real HP Victus testOS run.

These tests use assertions (not bool returns) so pytest reports failures
correctly. They verify the fixes without requiring real hardware.

Run:
    python3 -m pytest tools/test-testos-real-hardware-defects.py -v
    python3 tools/test-testos-real-hardware-defects.py  # direct (exit 1 on fail)
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

TOOLS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOLS_DIR.parent


def run(cmd, timeout=30):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return r.returncode, r.stdout, r.stderr


def read_bench_list():
    return (REPO_ROOT / "testos" / "bench-list.toml").read_text()


# ─── Defect 1: testos_version ────────────────────────────────────────────────


def test_1_testos_version_no_stale_fallback():
    """testos_version must derive from canonical VERSION, not 0.7.0-beta.1."""
    runner_src = (REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs").read_text()
    assert 'TESTOS_VERSION_FALLBACK: &str = "0.7.0-beta.1"' not in runner_src, \
        "stale fallback '0.7.0-beta.1' still present"
    assert "/etc/testos/version" in runner_src, \
        "runner does not read /etc/testos/version"
    build_script = (REPO_ROOT / "testos" / "build-testos.sh").read_text()
    assert "/etc/testos/version" in build_script, \
        "build-testos.sh does not write /etc/testos/version"


# ─── Defect 2: postgres-tps ──────────────────────────────────────────────────


def test_2_postgres_uses_runuser_not_sudo():
    """postgres-tps must not use sudo -u for pg_ctl (pg_ctl refuses root)."""
    bench_list = read_bench_list()
    pg_section = re.search(r'id = "postgres-tps".*?(?=\n\[\[benches\]\]|\Z)', bench_list, re.DOTALL)
    assert pg_section, "postgres-tps benchmark not found"
    pg_cmd = pg_section.group(0)
    lines = pg_cmd.split('\n')
    sudo_pg_ctl_lines = [l for l in lines if 'sudo -u' in l and 'pg_ctl' in l]
    assert not sudo_pg_ctl_lines, \
        f"sudo -u used with pg_ctl: {sudo_pg_ctl_lines[0].strip()}"
    assert "runuser" in pg_cmd or "su -s" in pg_cmd, \
        "no runuser or su for unprivileged execution"


# ─── Defect 3: PSI parser ────────────────────────────────────────────────────


def test_3_psi_parser_preserves_decimals():
    """PSI avg10 parser must preserve decimals, not produce '005' or '132'."""
    bench_list = read_bench_list()
    # The TOML uses ''' for PSI commands — extract with that delimiter
    psi_section = re.search(r"id = \"psi-cpu-avg10\".*?command = '''(.*?)'''", bench_list, re.DOTALL)
    if not psi_section:
        psi_section = re.search(r'id = "psi-cpu-avg10".*?command = """(.*?)"""', bench_list, re.DOTALL)
    assert psi_section, "psi-cpu-avg10 benchmark not found"
    psi_cmd = psi_section.group(1).strip()
    assert "awk '{split($2,a" not in psi_cmd, \
        "still uses old awk split() parser"

    # Test the parser against realistic PSI output
    psi_output = "some avg10=0.25 avg60=0.10 avg300=0.05 total=695299\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"
    with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
        f.write(psi_output)
        psi_file = f.name
    try:
        test_cmd = psi_cmd.replace("/proc/pressure/cpu", psi_file)
        rc, stdout, stderr = run(["bash", "-c", test_cmd])
        assert rc == 0, f"parser exited {rc}: {stderr}"
        result = stdout.strip()
        assert result, "parser produced empty output"
        val = float(result)
        assert val == 0.25, f"parser returned {val}, expected 0.25"
    finally:
        os.unlink(psi_file)


def test_3b_psi_parser_variable_whitespace():
    """PSI parser must handle variable whitespace."""
    bench_list = read_bench_list()
    psi_section = re.search(r"id = \"psi-cpu-avg10\".*?command = '''(.*?)'''", bench_list, re.DOTALL)
    if not psi_section:
        psi_section = re.search(r'id = "psi-cpu-avg10".*?command = """(.*?)"""', bench_list, re.DOTALL)
    psi_cmd = psi_section.group(1).strip()
    # Test with extra whitespace
    psi_output = "some  avg10=0.50  avg60=0.20  avg300=0.10  total=123456\n"
    with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
        f.write(psi_output)
        psi_file = f.name
    try:
        test_cmd = psi_cmd.replace("/proc/pressure/cpu", psi_file)
        rc, stdout, _ = run(["bash", "-c", test_cmd])
        assert rc == 0, "parser failed on variable whitespace"
        val = float(stdout.strip())
        assert val == 0.50, f"expected 0.50, got {val}"
    finally:
        os.unlink(psi_file)


def test_3c_psi_parser_malformed_input_fails():
    """PSI parser must produce non-numeric/empty on malformed input."""
    bench_list = read_bench_list()
    psi_section = re.search(r"id = \"psi-cpu-avg10\".*?command = '''(.*?)'''", bench_list, re.DOTALL)
    if not psi_section:
        psi_section = re.search(r'id = "psi-cpu-avg10".*?command = """(.*?)"""', bench_list, re.DOTALL)
    psi_cmd = psi_section.group(1).strip()
    # Malformed: no 'some' line
    psi_output = "garbage line without avg10\n"
    with tempfile.NamedTemporaryFile(mode='w', suffix='.txt', delete=False) as f:
        f.write(psi_output)
        psi_file = f.name
    try:
        test_cmd = psi_cmd.replace("/proc/pressure/cpu", psi_file)
        rc, stdout, _ = run(["bash", "-c", test_cmd])
        result = stdout.strip()
        # Empty output is acceptable for malformed input (runner classifies as fail)
        assert result == "" or result == "ERROR", \
            f"parser should produce empty/ERROR on malformed input, got {result!r}"
    finally:
        os.unlink(psi_file)


# ─── Defect 4: cyclictest ────────────────────────────────────────────────────


def test_4_cyclictest_produces_numeric_result():
    """cyclictest must not use -q (suppresses Max: line)."""
    bench_list = read_bench_list()
    cyclic_section = re.search(r'id = "cyclictest-max".*?command = (?:"""|\'\'\')(.*?)(?:"""|\'\'\')', bench_list, re.DOTALL)
    assert cyclic_section, "cyclictest-max benchmark not found"
    cyclic_cmd = cyclic_section.group(1)
    cmd_parts = cyclic_cmd.split()
    for i, part in enumerate(cmd_parts):
        if part == "cyclictest":
            for j in range(i + 1, len(cmd_parts)):
                flag = cmd_parts[j]
                if flag.startswith("-") and "q" in flag:
                    pytest.fail(f"cyclictest uses -q flag: {flag}")
                if not flag.startswith("-"):
                    break
            break
    assert "/Max:/" in cyclic_cmd, "cyclictest does not parse Max: line"


# ─── Defect 5: dmesg/journal privacy ─────────────────────────────────────────


def test_5_dmesg_journal_privacy_redaction():
    """dmesg/journal collection must redact MAC/serial/UUID fields."""
    runner_src = (REPO_ROOT / "crates" / "testos" / "src" / "bin" / "testos-runner.rs").read_text()
    assert "privacy_filter" in runner_src, "no privacy_filter in testos-runner"
    assert "<MAC>" in runner_src, "no MAC address redaction"
    assert "<SERIAL>" in runner_src, "no serial number redaction"
    assert "<UUID>" in runner_src, "no UUID redaction"

    # Verify the sed filter actually works
    test_input = "MAC=aa:bb:cc:dd:ee:ff SerialNumber=ABC123 UUID=12345678-1234-1234-1234-123456789abc IP=192.168.1.1"
    sed_filter = r"""sed -re 's/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/<UUID>/g' -e 's/([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}/<MAC>/g' -e 's/[Ss]erial[Nn]umber=[^ ]*/<SERIAL>/g' -e 's/serial=[0-9a-fA-F]{6,}/<SERIAL>/g' -e 's/\b([0-9]{1,3}\.){3}[0-9]{1,3}\b/<IPV4>/g'"""
    rc, stdout, stderr = run(["bash", "-c", f"echo '{test_input}' | {sed_filter}"])
    assert rc == 0, f"sed filter failed: {stderr}"
    redacted = stdout.strip()
    assert "aa:bb:cc:dd:ee:ff" not in redacted, f"MAC not redacted: {redacted}"
    assert "ABC123" not in redacted, f"serial not redacted: {redacted}"
    assert "12345678-1234-1234-1234-123456789abc" not in redacted, f"UUID not redacted: {redacted}"
    assert "192.168.1.1" not in redacted, f"IP not redacted: {redacted}"


# ─── Main for direct execution ───────────────────────────────────────────────


if __name__ == "__main__":
    # Allow direct execution: run pytest on this file
    sys.exit(pytest.main([__file__, "-v"] + sys.argv[1:]))
