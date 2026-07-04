#!/usr/bin/env python3
"""
pytest tests for the Rush LiveDev capture substrate (rush-exec + rush-capture).

Tests the 9 required scenarios:
  1. successful command capture
  2. failing command capture
  3. timeout capture
  4. stdout/stderr files written
  5. command-log entry appended
  6. redaction works
  7. event chain validates
  8. modified event chain fails
  9. summary generation works

Plus bonus unit tests for the shared library (redaction, event chain, snippets).

Run with:
  python3 -m pytest tools/test-rush-capture.py -v
  # or
  python3 tools/test-rush-capture.py  # standalone (no pytest required)
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

# ─── Import the shared library via importlib (matches test-rush-builder-unit.py) ─

_TOOLS_DIR = Path(__file__).resolve().parent

# A fake GitHub token used in redaction tests. Constructed by concatenation so
# the redaction regex still matches it in the emitted output, but the literal
# string does not appear as a single token in the source file.
_GHP = "ghp_" + "aBcDeFgHiJkLmNoPqRsTuVwXyZ" + "1234567890abcd"
_MAC = "00:" + "1A:2B:3C:4D:5E"


def _load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


lib = _load_module("rush_capture_lib", _TOOLS_DIR / "rush_capture_lib.py")

# ─── Helpers ──────────────────────────────────────────────────────────────────


def _run_tool(tool: str, *args: str) -> subprocess.CompletedProcess:
    """Run a capture-substrate tool and return the CompletedProcess."""
    return subprocess.run(
        ["python3", str(_TOOLS_DIR / tool), *args],
        capture_output=True,
        text=True,
        timeout=30,
    )


def _fresh_run_dir() -> Path:
    """Create a fresh temp run directory."""
    d = Path(tempfile.mkdtemp(prefix="rush-test-"))
    return d


def _start_session(run_dir: Path, repo_root: Path | None = None) -> int:
    args = ["start", "--run-dir", str(run_dir)]
    if repo_root:
        args.extend(["--repo-root", str(repo_root)])
    return _run_tool("rush-capture", *args).returncode


def _exec_command(run_dir: Path, *cmd: str, timeout: float | None = None, no_redact: bool = False) -> int:
    args = ["--run-dir", str(run_dir)]
    if timeout is not None:
        args.extend(["--timeout", str(timeout)])
    if no_redact:
        args.append("--no-redact")
    args.append("--")
    args.extend(cmd)
    return _run_tool("rush-exec", *args).returncode


def _finish_session(run_dir: Path) -> int:
    return _run_tool("rush-capture", "finish", "--run-dir", str(run_dir)).returncode


def _validate_chain(run_dir: Path) -> tuple[int, str]:
    r = _run_tool("rush-capture", "validate-chain", "--run-dir", str(run_dir))
    return r.returncode, r.stderr


def _read_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    out = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line:
                out.append(json.loads(line))
    return out


# ─── Test 1: successful command capture ───────────────────────────────────────


def test_successful_command_capture():
    """A command that exits 0 is captured with exit_code=0."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        rc = _exec_command(run_dir, "echo", "hello")
        assert rc == 0, f"echo should exit 0, got {rc}"

        commands = _read_jsonl(run_dir / "command-log.jsonl")
        assert len(commands) == 1
        assert commands[0]["exit_code"] == 0
        assert commands[0]["argv"] == ["echo", "hello"]
        assert commands[0]["timed_out"] is False
        assert commands[0]["duration_ms"] >= 0
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 2: failing command capture ──────────────────────────────────────────


def test_failing_command_capture():
    """A command that exits non-zero is captured with the correct exit_code."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        rc = _exec_command(run_dir, "false")
        assert rc != 0, f"false should exit non-zero, got {rc}"

        commands = _read_jsonl(run_dir / "command-log.jsonl")
        assert len(commands) == 1
        assert commands[0]["exit_code"] == 1
        assert commands[0]["timed_out"] is False
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 3: timeout capture ──────────────────────────────────────────────────


def test_timeout_capture():
    """A command that exceeds the timeout is killed and recorded as timed_out."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        rc = _exec_command(run_dir, "sleep", "10", timeout=1.0)
        assert rc == 124, f"timeout should produce exit 124, got {rc}"

        commands = _read_jsonl(run_dir / "command-log.jsonl")
        assert len(commands) == 1
        assert commands[0]["timed_out"] is True
        assert commands[0]["exit_code"] == 124
        assert commands[0]["timeout_sec"] == 1.0
        stderr_path = run_dir / "stderr" / f"{commands[0]['seq']}.txt"
        assert stderr_path.exists()
        stderr_text = stderr_path.read_text()
        assert "timed out" in stderr_text
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 4: stdout/stderr files written ──────────────────────────────────────


def test_stdout_stderr_files_written():
    """stdout and stderr are written to per-command files in the run directory."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        rc = _exec_command(
            run_dir,
            "python3",
            "-c",
            "import sys; print('out-line'); print('err-line', file=sys.stderr)",
        )
        assert rc == 0

        commands = _read_jsonl(run_dir / "command-log.jsonl")
        assert len(commands) == 1
        seq = commands[0]["seq"]
        stdout_path = run_dir / "stdout" / f"{seq}.txt"
        stderr_path = run_dir / "stderr" / f"{seq}.txt"
        assert stdout_path.exists(), f"stdout file missing: {stdout_path}"
        assert stderr_path.exists(), f"stderr file missing: {stderr_path}"
        assert "out-line" in stdout_path.read_text()
        assert "err-line" in stderr_path.read_text()
        assert commands[0]["stdout_path"] == f"stdout/{seq}.txt"
        assert commands[0]["stderr_path"] == f"stderr/{seq}.txt"
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 5: command-log entry appended ───────────────────────────────────────


def test_command_log_entry_appended():
    """Multiple commands produce multiple appended command-log entries."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        _exec_command(run_dir, "echo", "first")
        _exec_command(run_dir, "echo", "second")
        _exec_command(run_dir, "echo", "third")

        commands = _read_jsonl(run_dir / "command-log.jsonl")
        assert len(commands) == 3
        assert commands[0]["argv"] == ["echo", "first"]
        assert commands[1]["argv"] == ["echo", "second"]
        assert commands[2]["argv"] == ["echo", "third"]

        for c in commands:
            assert "argv" in c
            assert "cwd" in c
            assert "started_at" in c
            assert "finished_at" in c
            assert "duration_ms" in c
            assert "exit_code" in c
            assert "timed_out" in c
            assert "stdout_path" in c
            assert "stderr_path" in c
            assert "stdout_sha256" in c
            assert "stderr_sha256" in c
            assert "stdout_snippet" in c
            assert "stderr_snippet" in c
            assert "env_redacted" in c
            assert "redaction_status" in c
            assert "redaction_counts" in c
            assert "payload_sha256" in c
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 6: redaction works ──────────────────────────────────────────────────


def test_redaction_works():
    """GitHub tokens and MAC addresses are redacted in output."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        script = "print('token=" + _GHP + "'); print('mac=" + _MAC + "')"
        _exec_command(run_dir, "python3", "-c", script)

        commands = _read_jsonl(run_dir / "command-log.jsonl")
        assert len(commands) == 1
        seq = commands[0]["seq"]
        stdout_text = (run_dir / "stdout" / f"{seq}.txt").read_text()

        assert _GHP not in stdout_text
        assert "[REDACTED:github_token]" in stdout_text
        assert _MAC not in stdout_text
        assert "[REDACTED:mac_address]" in stdout_text

        counts = commands[0]["redaction_counts"]
        assert counts.get("github_token", 0) >= 1
        assert counts.get("mac_address", 0) >= 1

        snippet = commands[0]["stdout_snippet"]
        assert _GHP not in snippet
        assert _MAC not in snippet
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


def test_redaction_secret_env_var():
    """Secret env var names cause the value to be fully redacted in env_redacted."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        env = os.environ.copy()
        env["MY_GITHUB_TOKEN"] = _GHP
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "rush-exec"), "--run-dir", str(run_dir), "--", "printenv", "MY_GITHUB_TOKEN"],
            capture_output=True,
            text=True,
            env=env,
            timeout=10,
        )
        assert r.returncode == 0

        commands = _read_jsonl(run_dir / "command-log.jsonl")
        assert len(commands) == 1
        seq = commands[0]["seq"]
        stdout_text = (run_dir / "stdout" / f"{seq}.txt").read_text()

        assert _GHP not in stdout_text

        env_redacted = commands[0]["env_redacted"]
        assert env_redacted.get("MY_GITHUB_TOKEN") == "[REDACTED:secret_env]"

        counts = commands[0]["redaction_counts"]
        assert counts.get("secret_env", 0) >= 1
        assert counts.get("github_token", 0) >= 1
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 7: event chain validates ────────────────────────────────────────────


def test_event_chain_validates():
    """A fresh event chain with start + command + finish validates cleanly."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        _exec_command(run_dir, "echo", "hello")
        assert _finish_session(run_dir) == 0

        rc, stderr = _validate_chain(run_dir)
        assert rc == 0, f"chain should validate, got rc={rc} stderr={stderr}"

        events = _read_jsonl(run_dir / "events.jsonl")
        assert len(events) == 3
        assert events[0]["kind"] == "start"
        assert events[1]["kind"] == "command"
        assert events[2]["kind"] == "finish"

        ok, errors = lib.validate_chain(events)
        assert ok, f"chain validation failed: {errors}"
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 8: modified event chain fails ───────────────────────────────────────


def test_modified_event_chain_fails():
    """Editing an event's payload breaks the chain and validate-chain detects it."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        _exec_command(run_dir, "echo", "hello")
        assert _finish_session(run_dir) == 0

        events_path = run_dir / "events.jsonl"
        events = _read_jsonl(events_path)
        assert len(events) >= 2

        for e in events:
            if e.get("kind") == "command":
                e["payload"]["exit_code"] = 999
                break

        with open(events_path, "w") as f:
            for e in events:
                f.write(json.dumps(e, sort_keys=True, separators=(",", ":")))
                f.write("\n")

        rc, stderr = _validate_chain(run_dir)
        assert rc != 0, "modified chain should fail validation"
        assert "event_sha256 mismatch" in stderr or "prev_event_sha256 mismatch" in stderr
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


def test_deleted_event_chain_fails():
    """Deleting an event from the chain breaks the prev_event_sha256 link."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        _exec_command(run_dir, "echo", "hello")
        assert _finish_session(run_dir) == 0

        events_path = run_dir / "events.jsonl"
        events = _read_jsonl(events_path)
        events = [e for e in events if e.get("seq") != 1]
        with open(events_path, "w") as f:
            for e in events:
                f.write(json.dumps(e, sort_keys=True, separators=(",", ":")))
                f.write("\n")

        rc, stderr = _validate_chain(run_dir)
        assert rc != 0
        assert "seq mismatch" in stderr or "prev_event_sha256 mismatch" in stderr
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test 9: summary generation works ─────────────────────────────────────────


def test_summary_generation_works():
    """rush-capture finish + summarize produce a readable summary.md."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        _exec_command(run_dir, "echo", "hello")
        _exec_command(run_dir, "false")
        assert _finish_session(run_dir) == 0

        summary_path = run_dir / "summary.md"
        assert summary_path.exists(), "summary.md should be created by finish"
        text = summary_path.read_text()

        assert "# Capture Session Summary" in text
        assert "Run dir:" in text
        assert "Started:" in text
        assert "Finished:" in text
        assert "Host fingerprint:" in text
        assert "Events:" in text
        assert "Commands:" in text
        assert "## Event Chain" in text
        assert "## Commands" in text
        assert "## Privacy Report" in text
        assert "## Chain Validation" in text
        assert "Chain intact" in text

        assert "echo hello" in text
        assert "false" in text

        rc = _run_tool("rush-capture", "summarize", "--run-dir", str(run_dir)).returncode
        assert rc == 0
        text2 = summary_path.read_text()
        assert text2.startswith("# Capture Session Summary")
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Bonus unit tests for the shared library ──────────────────────────────────


def test_redact_github_token():
    report = lib.RedactionReport()
    out = lib.redact(_GHP, report)
    assert out == "[REDACTED:github_token]"
    assert report.counts["github_token"] == 1


def test_redact_mac_address():
    report = lib.RedactionReport()
    full_mac = _MAC
    out = lib.redact(f"MAC: {full_mac} and 01-23-45-67-89-AB", report)
    assert full_mac not in out
    assert "01-23-45-67-89-AB" not in out
    assert report.counts["mac_address"] == 2


def test_redact_bearer_token():
    report = lib.RedactionReport()
    out = lib.redact("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig", report)
    assert "eyJhbGciOiJIUzI1NiJ9.payload.sig" not in out
    assert "[REDACTED:bearer_token]" in out


def test_redact_private_ipv4():
    report = lib.RedactionReport()
    out = lib.redact("server at 192.168.1.100 and 10.0.0.1 and 172.16.0.5", report)
    assert "192.168.1.100" not in out
    assert "10.0.0.1" not in out
    assert "172.16.0.5" not in out
    out2 = lib.redact("public 8.8.8.8 is fine", report)
    assert "8.8.8.8" in out2


def test_redact_env_secret_name():
    report = lib.RedactionReport()
    env = {"HOME": "/home/z", "GITHUB_TOKEN": "ghp_secret", "MY_API_KEY": "key123"}
    out = lib.redact_env(env, report)
    assert out["HOME"] == "/home/z"
    assert out["GITHUB_TOKEN"] == "[REDACTED:secret_env]"
    assert out["MY_API_KEY"] == "[REDACTED:secret_env]"
    assert report.counts["secret_env"] == 2


def test_redact_env_value_redaction():
    """Non-secret env var names still have their VALUES redacted."""
    report = lib.RedactionReport()
    env = {"BANNER": _GHP}
    out = lib.redact_env(env, report)
    assert out["BANNER"] == "[REDACTED:github_token]"
    assert report.counts["github_token"] == 1


def test_event_chain_round_trip():
    """make_event + validate_chain round-trips cleanly."""
    e0 = lib.make_event(seq=0, kind="start", payload={"a": 1})
    e1 = lib.make_event(
        seq=1, kind="command", payload={"b": 2}, prev_event_sha256=e0["event_sha256"]
    )
    e2 = lib.make_event(
        seq=2, kind="finish", payload={}, prev_event_sha256=e1["event_sha256"]
    )
    ok, errors = lib.validate_chain([e0, e1, e2])
    assert ok, f"clean chain should validate: {errors}"


def test_event_chain_detects_reorder():
    """Reordering events breaks the chain."""
    e0 = lib.make_event(seq=0, kind="start")
    e1 = lib.make_event(seq=1, kind="command", prev_event_sha256=e0["event_sha256"])
    ok, errors = lib.validate_chain([e1, e0])
    assert not ok


def test_event_chain_detects_tamper():
    """Changing a payload after hashing breaks the chain."""
    e0 = lib.make_event(seq=0, kind="start", payload={"a": 1})
    e1 = lib.make_event(
        seq=1, kind="command", payload={"b": 2}, prev_event_sha256=e0["event_sha256"]
    )
    e0_tampered = dict(e0)
    e0_tampered["payload"] = {"a": 999}
    ok, errors = lib.validate_chain([e0_tampered, e1])
    assert not ok


def test_snippet_truncates_long_text():
    long_text = "x" * 10000
    s = lib.snippet(long_text, max_bytes=100)
    assert len(s) < 200
    assert "truncated" in s


def test_snippet_preserves_short_text():
    short = "hello"
    s = lib.snippet(short, max_bytes=100)
    assert s == "hello"


def test_sha256_str_stable():
    assert lib.sha256_str("hello") == lib.sha256_str("hello")
    assert lib.sha256_str("hello") != lib.sha256_str("world")


# ─── Test: rush-exec rejects shell strings ────────────────────────────────────


def test_rush_exec_rejects_sh_c():
    """rush-exec refuses 'sh -c ...' — the caller must construct argv explicitly."""
    run_dir = _fresh_run_dir()
    try:
        r = _run_tool("rush-exec", "--run-dir", str(run_dir), "--", "sh", "-c", "echo hello")
        assert r.returncode == 2
        assert "typed argv" in r.stderr or "shell string" in r.stderr
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Test: full lifecycle ─────────────────────────────────────────────────────


def test_full_lifecycle():
    """start -> exec -> exec -> event -> finish -> validate-chain -> summarize."""
    run_dir = _fresh_run_dir()
    try:
        assert _start_session(run_dir) == 0
        assert _exec_command(run_dir, "echo", "first") == 0
        assert _exec_command(run_dir, "echo", "second") == 0

        r = _run_tool(
            "rush-capture",
            "event",
            "--run-dir", str(run_dir),
            "--kind", "note",
            "--payload", '{"text": "midway checkpoint"}',
        )
        assert r.returncode == 0

        assert _finish_session(run_dir) == 0

        rc, _ = _validate_chain(run_dir)
        assert rc == 0

        events = _read_jsonl(run_dir / "events.jsonl")
        assert len(events) == 5
        kinds = [e["kind"] for e in events]
        assert kinds == ["start", "command", "command", "note", "finish"]

        assert (run_dir / "manifest.json").exists()
        assert (run_dir / "events.jsonl").exists()
        assert (run_dir / "command-log.jsonl").exists()
        assert (run_dir / "host.json").exists()
        assert (run_dir / "software.json").exists()
        assert (run_dir / "privacy-report.json").exists()
        assert (run_dir / "summary.md").exists()
        assert (run_dir / "stdout").is_dir()
        assert (run_dir / "stderr").is_dir()
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


# ─── Standalone runner (no pytest required) ───────────────────────────────────


def _run_all_tests() -> int:
    """Discover and run all test_* functions. Returns 0 on success, 1 on failure."""
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
