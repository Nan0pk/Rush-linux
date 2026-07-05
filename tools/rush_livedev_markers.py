#!/usr/bin/env python3
"""
rush-livedev-markers — the guest↔host console marker protocol.

The guest emits single-line markers on the serial console. The host parses
the console stream and drives its state machine off these markers.

Marker grammar
==============

Every marker is a single line of the form:

    RUSH_LIVEDEV_<KIND> key1=value1 key2=value2 ...

`<KIND>` is one of:

    BOOT_READY       — guest reached multi-user.target and the runner woke up
    TEST_START       — runner is about to execute the test command
    TEST_PASS        — test command exited 0
    TEST_FAIL        — test command exited nonzero (exit_code=<N>)
    ARTIFACTS_READY  — artifacts directory is fully populated (path=<path>)
    SHUTDOWN         — guest is about to power off cleanly
    DEBUG_SHELL      — guest is intentionally dropping to an interactive shell
                       (only emitted when --debug is explicitly set)

The host ALSO recognizes these UNINTENDED guest patterns as failures:

    kernel panic/Oops/oops
    emergency mode / rescue mode / "Give root password for maintenance"
    a login prompt (`login:`) appearing BEFORE TEST_START
    a root shell prompt (`# `, `~#`, `bash-`) appearing BEFORE TEST_START

Parsing rules
=============- A marker line MUST start with `RUSH_LIVEDEV_`. Other lines are ignored.
- Keys are `key=value` with no spaces; values are URL-safe tokens
  (matches `[A-Za-z0-9._/=@:+-]+`).
- The host reads the console line-by-line and matches each line against
  the marker regex ONCE. Multiple markers per line are not allowed.

This module has no non-stdlib dependencies so it can be vendored into the
guest image and imported by both sides.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from typing import Optional

# Marker prefix. All guest-emitted markers start with this.
MARKER_PREFIX = "RUSH_LIVEDEV_"

# Kinds. Keep in sync with the docstring above.
KIND_BOOT_READY = "BOOT_READY"
KIND_TEST_START = "TEST_START"
KIND_TEST_PASS = "TEST_PASS"
KIND_TEST_FAIL = "TEST_FAIL"
KIND_ARTIFACTS_READY = "ARTIFACTS_READY"
KIND_SHUTDOWN = "SHUTDOWN"
KIND_DEBUG_SHELL = "DEBUG_SHELL"

ALL_KINDS = frozenset({
    KIND_BOOT_READY,
    KIND_TEST_START,
    KIND_TEST_PASS,
    KIND_TEST_FAIL,
    KIND_ARTIFACTS_READY,
    KIND_SHUTDOWN,
    KIND_DEBUG_SHELL,
})

# Terminal kinds = the run is over after this marker.
TERMINAL_KINDS = frozenset({
    KIND_TEST_PASS,
    KIND_TEST_FAIL,
    KIND_SHUTDOWN,
    KIND_DEBUG_SHELL,
})

# Marker regex. Intentionally strict: a marker line is the marker prefix,
# then a KIND, then zero or more ` key=value` pairs.
_MARKER_RE = re.compile(
    r"^(?P<prefix>" + re.escape(MARKER_PREFIX) + r")"
    r"(?P<kind>[A-Z_]+)"
    r"(?P<args>(?:\s+[A-Za-z_][A-Za-z0-9_]*=[^\s]+)*)"
    r"\s*$"
)
_ARG_RE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)=([^\s]+)")

# Failure patterns the host detects on the console. These are NOT emitted by
# the runner — they are kernel/systemd/emergency-shell patterns that indicate
# the guest fell into a state the host must treat as failure.
#
# Each entry is (description, regex). The regex is matched case-insensitively
# against each console line.
FAILURE_PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    ("kernel_panic",
     re.compile(r"Kernel panic|not syncing|BUG: kernel|Call Trace:", re.IGNORECASE)),
    ("emergency_mode",
     re.compile(r"You are in emergency mode|You are in rescue mode|"
                r"Welcome to emergency mode|Welcome to rescue mode|"
                r"emergency\.target|rescue\.target", re.IGNORECASE)),
    ("maintenance_prompt",
     re.compile(r"Give root password for maintenance", re.IGNORECASE)),
    ("login_prompt_before_test",
     re.compile(r"^\s*(login:|Password:)\s*$", re.IGNORECASE)),
    ("root_shell",
     re.compile(r"^(root|rush)@[a-zA-Z0-9._-]+:[^#]*#\s*$|"
                r"^\s*bash-[0-9.]+#\s*$|"
                r"^\s*~#\s*$")),
    ("systemd_failed_unit",
     re.compile(r"SYSTEMD_FAILED_UNIT|FAILED.*\.service|"
                r"Job for .+\.service failed", re.IGNORECASE)),
]


@dataclass(frozen=True)
class Marker:
    """A parsed guest marker line."""

    kind: str
    run_id: str = ""
    exit_code: int | None = None
    path: str = ""
    raw: str = ""

    @property
    def is_terminal(self) -> bool:
        return self.kind in TERMINAL_KINDS

    @property
    def is_pass(self) -> bool:
        return self.kind == KIND_TEST_PASS

    @property
    def is_fail(self) -> bool:
        return self.kind in (KIND_TEST_FAIL, KIND_DEBUG_SHELL)

    def to_line(self) -> str:
        """Re-serialize the marker to a single console line."""
        parts = [MARKER_PREFIX + self.kind]
        if self.run_id:
            parts.append(f"run_id={self.run_id}")
        if self.exit_code is not None:
            parts.append(f"exit_code={self.exit_code}")
        if self.path:
            parts.append(f"path={self.path}")
        return " ".join(parts)


def parse_marker(line: str) -> Optional[Marker]:
    """Parse one console line into a Marker, or None if it is not a marker.

    Returns None (not raises) on a malformed marker line so the host can keep
    streaming the console without crashing on a single bad line. A malformed
    marker that DID start with the prefix is logged by the caller.
    """
    if not line:
        return None
    line = line.rstrip("\r\n")
    if MARKER_PREFIX not in line:
        return None
    m = _MARKER_RE.match(line)
    if not m:
        return None
    kind = m.group("kind")
    if kind not in ALL_KINDS:
        return None
    args_str = m.group("args") or ""
    kv: dict[str, str] = {}
    for k, v in _ARG_RE.findall(args_str):
        kv[k] = v
    exit_code: int | None = None
    if "exit_code" in kv:
        try:
            exit_code = int(kv["exit_code"])
        except ValueError:
            exit_code = None
    return Marker(
        kind=kind,
        run_id=kv.get("run_id", ""),
        exit_code=exit_code,
        path=kv.get("path", ""),
        raw=line,
    )


def detect_failure(line: str) -> Optional[str]:
    """Return a failure-class description if the line matches a failure pattern.

    The host treats this as a hard failure: tests are not running, the guest
    is in an unintended state. Returns None if the line is benign.
    """
    if not line:
        return None
    line = line.rstrip("\r\n")
    for desc, pat in FAILURE_PATTERNS:
        if pat.search(line):
            return desc
    return None


# ─── Emitter (guest-side helper) ─────────────────────────────────────────────


def emit(kind: str, run_id: str = "", exit_code: int | None = None,
         path: str = "", file=None) -> str:
    """Emit a marker to stdout (or `file`). Returns the line written.

    Guest scripts should call this with file=sys.stdout or sys.stderr (the
    autostart service wires StandardOutput=journal+console so the marker
    lands on the serial console the host is watching).
    """
    if kind not in ALL_KINDS:
        raise ValueError(f"unknown marker kind: {kind!r}")
    line = Marker(kind=kind, run_id=run_id, exit_code=exit_code, path=path).to_line()
    print(line, file=file, flush=True)
    return line


# ─── CLI (smoke-test the parser from the shell) ──────────────────────────────


def _main() -> int:
    import sys
    if len(sys.argv) > 1 and sys.argv[1] == "emit":
        kind = sys.argv[2] if len(sys.argv) > 2 else "TEST_START"
        kv = {}
        for arg in sys.argv[3:]:
            if "=" in arg:
                k, v = arg.split("=", 1)
                kv[k] = v
        emit(kind, run_id=kv.get("run_id", ""),
             exit_code=int(kv["exit_code"]) if "exit_code" in kv else None,
             path=kv.get("path", ""))
        return 0
    # Default: parse stdin line-by-line and print matches.
    for line in sys.stdin:
        m = parse_marker(line)
        if m:
            print(f"MARKER kind={m.kind} run_id={m.run_id} exit_code={m.exit_code} path={m.path}")
            continue
        f = detect_failure(line)
        if f:
            print(f"FAILURE {f}: {line.rstrip()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
