#!/usr/bin/env python3
"""
rush-capture shared library — redaction, tamper-evident event chain, run-dir
management, and host/software fingerprinting for the Rush LiveDev capture
substrate.

This module is imported by tools/rush-exec and tools/rush-capture. It is also
importable by pytest tests via importlib.

Design constraints (per docs/plans/livedev-transition-plan.md Phase 2 and
docs/automation-human-interface.md):
  - No shell strings. Commands are typed argv arrays.
  - Every event is tamper-evident (SHA-256 chain).
  - Redaction is applied to stdout, stderr, env, and event payloads before
    anything is written to disk.
  - The run directory layout is fixed and documented.
  - No AI calls. No network. No real hardware required.

Run directory layout (created by capture_start):
  <run-dir>/
    manifest.json          — session metadata (schema, start time, run-dir path)
    events.jsonl           — tamper-evident event chain (one JSON object per line)
    command-log.jsonl      — per-command execution records (one JSON object per line)
    host.json              — host fingerprint (kernel, CPU, DMI board, battery)
    software.json          — software fingerprint (git commit, project version, rustc)
    privacy-report.json    — redaction summary (what was redacted, how many)
    summary.md             — human-readable session summary
    stdout/                — per-command stdout files (<seq>.txt)
    stderr/                — per-command stderr files (<seq>.txt)
"""

from __future__ import annotations

import hashlib
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# ─── Schema version ──────────────────────────────────────────────────────────

SCHEMA_VERSION = 1

# ─── Redaction ───────────────────────────────────────────────────────────────
#
# Redaction is applied to every string that may contain operator secrets
# before it is written to the run directory. The redactors are applied in
# order; each replaces the match with a placeholder that preserves the kind
# of secret (so the operator can see "a GitHub token was here" without seeing
# the token itself).
#
# The redactors are conservative: they may over-redact (replace a string that
# looks like a token but isn't). This is the safe failure mode. The privacy
# report records how many replacements each redactor made so the operator can
# audit.

_REDACT_PLACEHOLDER = "[REDACTED:{kind}]"


class RedactionReport:
    """Tracks how many replacements each redactor made during a session."""

    def __init__(self) -> None:
        self.counts: dict[str, int] = {}

    def record(self, kind: str, count: int) -> None:
        self.counts[kind] = self.counts.get(kind, 0) + count

    def to_dict(self) -> dict[str, Any]:
        return {
            "schema_version": SCHEMA_VERSION,
            "redactors": sorted(self.counts.keys()),
            "counts": dict(sorted(self.counts.items())),
            "total": sum(self.counts.values()),
        }


# Each redactor is (kind, compiled_regex, replacement_template). The regex
# matches the secret; the replacement is _REDACT_PLACEHOLDER.format(kind=kind).
_REDACTORS: list[tuple[str, re.Pattern[str], str]] = []


def _register(kind: str, pattern: str, flags: int = 0) -> None:
    _REDACTORS.append(
        (kind, re.compile(pattern, flags), _REDACT_PLACEHOLDER.format(kind=kind))
    )


# GitHub tokens: ghp_<36>, gho_<36>, ghs_<36>, ghu_<36>, ghr_<76>, github_pat_<82>
_register("github_token", r"gh[pousr]_[A-Za-z0-9]{36,}")
_register("github_pat", r"github_pat_[A-Za-z0-9_]{82}")

# Generic API keys: <prefix>_<40+ alphanum> where prefix suggests a key.
# Conservative — only matches when preceded by a known key-name indicator.
_register(
    "api_key",
    r"(?i)(api[_-]?key|secret[_-]?key|access[_-]?token|auth[_-]?token)[\"\s:=]+([A-Za-z0-9_\-]{32,})",
)
# Bearer tokens in Authorization headers.
_register("bearer_token", r"(?i)bearer\s+[A-Za-z0-9_\-\.=]{20,}")

# AWS access key IDs (AKIA + 16 chars) and secret keys (40 base64-ish).
# The secret key pattern requires a key-name prefix to avoid false-positives
# on git SHA hashes (which are also 40 hex chars) and other 40-char strings.
_register("aws_access_key_id", r"AKIA[0-9A-Z]{16}")
_register(
    "aws_secret_key",
    r"(?i)(aws[_-]?secret[_-]?access[_-]?key|aws[_-]?secret[_-]?key)[\"\s:=]+([A-Za-z0-9/+]{40})",
)

# Common secret env var names — the value is redacted when the name matches.
# This is applied per-env-var in _redact_env, not via the regex list above.
SECRET_ENV_PATTERNS = [
    re.compile(r"(?i).*(token|secret|password|passwd|api[_-]?key|private[_-]?key|access[_-]?key|client[_-]?secret|bearer).*"),
]

# MAC addresses: xx:xx:xx:xx:xx:xx or xx-xx-xx-xx-xx-xx
_register("mac_address", r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b")

# Obvious serial fields: "serial=<value>" or "Serial Number: <value>"
_register(
    "serial_field",
    r"(?i)(serial[_\s]*(?:number|no\.?)?|s/n)\s*[:=]\s*([A-Za-z0-9\-]{6,})",
)

# Private SSH key opening lines (the whole key block is redacted by the
# caller; this just catches the header in case it appears in a snippet).
_register("ssh_private_key", r"-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----")

# IPv4 addresses in private ranges (10.x, 172.16-31.x, 192.168.x). Public IPs
# are NOT redacted — they may be load-bearing in evidence (e.g., a benchmark
# server address). The operator can override.
_register(
    "private_ipv4",
    r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3})\b",
)


def redact(text: str, report: RedactionReport | None = None) -> str:
    """Apply all redactors to ``text``. Returns the redacted string.

    If ``report`` is provided, each redactor's replacement count is recorded
    so the caller can produce a privacy report.
    """
    if not isinstance(text, str):
        return text
    for kind, regex, replacement in _REDACTORS:
        if kind in ("api_key", "serial_field"):
            # These redactors capture a group; the replacement must preserve
            # the key-name prefix and redact only the value.
            def _repl(m: re.Match[str], k: str = kind, r: str = replacement) -> str:
                # Group 1 = the key name, group 2 = the secret value.
                return m.group(1) + " " + r if m.lastindex and m.lastindex >= 2 else r

            new_text, n = regex.subn(_repl, text)
        else:
            new_text, n = regex.subn(replacement, text)
        if n > 0 and report is not None:
            report.record(kind, n)
        text = new_text
    return text


def redact_env(
    env: dict[str, str], report: RedactionReport | None = None
) -> dict[str, str]:
    """Redact environment variables.

    Two layers:
      1. If the variable NAME matches a secret pattern (token, secret,
         password, api_key, private_key, etc.), the VALUE is fully redacted.
      2. Otherwise, the VALUE is passed through the general redact() function
         (which catches tokens, MACs, etc. that happen to appear in non-secret
         env vars).
    """
    out: dict[str, str] = {}
    for name, value in env.items():
        is_secret_name = any(p.fullmatch(name) or p.match(name) for p in SECRET_ENV_PATTERNS)
        if is_secret_name:
            out[name] = _REDACT_PLACEHOLDER.format(kind="secret_env")
            if report is not None:
                report.record("secret_env", 1)
        else:
            out[name] = redact(value, report)
    return out


def redact_dict(obj: Any, report: RedactionReport | None = None) -> Any:
    """Recursively redact all strings in a nested dict/list structure."""
    if isinstance(obj, str):
        return redact(obj, report)
    if isinstance(obj, dict):
        return {k: redact_dict(v, report) for k, v in obj.items()}
    if isinstance(obj, list):
        return [redact_dict(v, report) for v in obj]
    return obj


# ─── Tamper-evident event chain ──────────────────────────────────────────────
#
# Each event is a JSON object with:
#   seq:                0-indexed monotonic sequence number
#   timestamp:          ISO 8601 UTC
#   kind:               "start" | "event" | "command" | "finish"
#   payload:            inline JSON (when small) OR
#   payload_path:       relative path to a file in the run dir (when large)
#   prev_event_sha256:  SHA-256 of the previous event's canonical JSON
#                       (all-zeros for seq=0)
#   event_sha256:       SHA-256 of THIS event's canonical JSON (computed
#                       over the event WITHOUT the event_sha256 field, then
#                       the field is set to the computed value)
#
# The chain is tamper-evident: editing any event changes its event_sha256,
# which breaks the prev_event_sha256 link in the next event. Deleting an
# event breaks the link. Reordering events breaks the link. Inserting an
# event requires recomputing all subsequent hashes.


def _canonical_json(obj: Any) -> str:
    """Canonical JSON encoding: sorted keys, no extra whitespace."""
    return json.dumps(obj, sort_keys=True, separators=(",", ":"))


def compute_event_sha256(event: dict[str, Any]) -> str:
    """Compute the SHA-256 of an event, excluding the event_sha256 field."""
    stripped = {k: v for k, v in event.items() if k != "event_sha256"}
    return hashlib.sha256(_canonical_json(stripped).encode("utf-8")).hexdigest()


def make_event(
    seq: int,
    kind: str,
    payload: Any | None = None,
    payload_path: str | None = None,
    prev_event_sha256: str = "0" * 64,
) -> dict[str, Any]:
    """Construct a tamper-evident event with a computed event_sha256."""
    event: dict[str, Any] = {
        "seq": seq,
        "timestamp": _now_iso(),
        "kind": kind,
        "prev_event_sha256": prev_event_sha256,
    }
    if payload is not None:
        event["payload"] = payload
    if payload_path is not None:
        event["payload_path"] = payload_path
    event["event_sha256"] = compute_event_sha256(event)
    return event


def validate_chain(events: list[dict[str, Any]]) -> tuple[bool, list[str]]:
    """Validate a tamper-evident event chain.

    Returns (ok, errors). ok is True when the chain is intact:
      - seq numbers are 0, 1, 2, ... contiguous
      - prev_event_sha256 of event N == event_sha256 of event N-1
      - event_sha256 of every event matches a recomputation
      - the first event's prev_event_sha256 is all-zeros
    """
    errors: list[str] = []
    prev_sha = "0" * 64
    for i, event in enumerate(events):
        if event.get("seq") != i:
            errors.append(
                f"event {i}: seq mismatch (expected {i}, got {event.get('seq')})"
            )
        actual_prev = event.get("prev_event_sha256", "")
        if actual_prev != prev_sha:
            errors.append(
                f"event {i}: prev_event_sha256 mismatch "
                f"(expected {prev_sha[:12]}…, got {actual_prev[:12]}…)"
            )
        recomputed = compute_event_sha256(event)
        actual_sha = event.get("event_sha256", "")
        if actual_sha != recomputed:
            errors.append(
                f"event {i}: event_sha256 mismatch "
                f"(expected {recomputed[:12]}…, got {actual_sha[:12]}…)"
            )
        prev_sha = actual_sha
    return (len(errors) == 0, errors)


# ─── Run directory management ────────────────────────────────────────────────


def init_run_dir(run_dir: Path) -> None:
    """Create the run directory and its subdirectories."""
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / "stdout").mkdir(exist_ok=True)
    (run_dir / "stderr").mkdir(exist_ok=True)


def append_jsonl(path: Path, obj: dict[str, Any]) -> None:
    """Append a JSON object as a single line to a .jsonl file."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a", encoding="utf-8") as f:
        f.write(json.dumps(obj, sort_keys=True, separators=(",", ":")))
        f.write("\n")
        f.flush()
        os.fsync(f.fileno())


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    """Read a .jsonl file into a list of dicts. Empty list if file missing."""
    if not path.exists():
        return []
    out: list[dict[str, Any]] = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                out.append(json.loads(line))
    return out


def last_event_sha256(events_path: Path) -> str:
    """Return the event_sha256 of the last event in the chain, or all-zeros."""
    events = read_jsonl(events_path)
    if not events:
        return "0" * 64
    return events[-1].get("event_sha256", "0" * 64)


def next_seq(events_path: Path) -> int:
    """Return the next seq number for the event chain."""
    events = read_jsonl(events_path)
    if not events:
        return 0
    return events[-1].get("seq", -1) + 1


# ─── Host and software fingerprinting ────────────────────────────────────────


def capture_host() -> dict[str, Any]:
    """Capture a host fingerprint. Best-effort; missing files yield 'unknown'."""
    def _read(path: str) -> str:
        try:
            with open(path) as f:
                return f.read().strip()
        except (OSError, IOError):
            return "unknown"

    kernel = _read("/proc/sys/kernel/osrelease") or platform.release()
    cpu_model = "unknown"
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    cpu_model = line.split(":", 1)[1].strip()
                    break
    except (OSError, IOError):
        pass

    board_vendor = _read("/sys/class/dmi/id/board_vendor")
    board_name = _read("/sys/class/dmi/id/board_name")
    dmi_board = f"{board_vendor} {board_name}".strip() or "unknown"

    battery_design_uwh = 0
    for bat in ("BAT0", "BAT1", "BATT"):
        p = f"/sys/class/power_supply/{bat}/energy_full_design"
        v = _read(p)
        if v and v != "unknown":
            try:
                battery_design_uwh = int(v)
                break
            except ValueError:
                pass

    fingerprint_input = f"{kernel}|{cpu_model}|{dmi_board}|{battery_design_uwh}"
    fingerprint = hashlib.sha256(fingerprint_input.encode("utf-8")).hexdigest()[:16]

    return {
        "schema_version": SCHEMA_VERSION,
        "kernel": kernel,
        "cpu_model": cpu_model,
        "dmi_board": dmi_board,
        "battery_design_uwh": battery_design_uwh,
        "hostname": "unknown",  # redacted by default; set by caller if needed
        "fingerprint": fingerprint,
    }


def capture_software(repo_root: Path | None = None) -> dict[str, Any]:
    """Capture a software fingerprint: git commit, project version, toolchain."""
    sw: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "python_version": sys.version.split()[0],
        "platform": platform.platform(),
    }

    if repo_root and (repo_root / "VERSION").exists():
        try:
            sw["project_version"] = (repo_root / "VERSION").read_text().strip()
        except OSError:
            sw["project_version"] = "unknown"
        git_commit = _git_rev_parse(repo_root)
        if git_commit:
            sw["git_commit"] = git_commit

    # rustc version (best-effort)
    try:
        r = subprocess.run(
            ["rustc", "--version"], capture_output=True, text=True, timeout=5
        )
        if r.returncode == 0:
            sw["rustc_version"] = r.stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        pass

    return sw


def _git_rev_parse(repo_root: Path) -> str | None:
    try:
        r = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if r.returncode == 0:
            return r.stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        pass
    return None


# ─── Timestamp helpers ───────────────────────────────────────────────────────


def _now_iso() -> str:
    """ISO 8601 UTC timestamp with second precision."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def now_unix_ms() -> int:
    """Unix epoch milliseconds (for duration calculations)."""
    return int(time.time() * 1000)


def duration_ms(start_unix_ms: int, end_unix_ms: int) -> int:
    return end_unix_ms - start_unix_ms


# ─── Snippet extraction ──────────────────────────────────────────────────────


def snippet(text: str, max_bytes: int = 4096) -> str:
    """Return a bounded snippet of ``text``. If truncated, appends a marker."""
    if len(text.encode("utf-8")) <= max_bytes:
        return text
    truncated = text.encode("utf-8")[:max_bytes].decode("utf-8", errors="ignore")
    return truncated + f"\n... [truncated, {len(text)} bytes total]"


# ─── Payload hashing ─────────────────────────────────────────────────────────


def sha256_file(path: Path) -> str:
    """SHA-256 of a file's contents."""
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_str(text: str) -> str:
    """SHA-256 of a string."""
    return hashlib.sha256(text.encode("utf-8")).hexdigest()
