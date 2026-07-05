#!/usr/bin/env python3
"""
rush-livedev-state — persistent test-intent state for Rush LiveDev.

This module owns the canonical state file that survives reboot and tells the
guest-side test runner what to do. It is the single source of truth for
"should this boot run tests, and if so, which?".

State file location (guest):
    /RUSH-DATA/state/livedev-state.json

The state file is written atomically (write-temp + fsync + rename) so that
a crash mid-write never leaves a partial file. Reads validate the schema
before returning; an invalid file is rejected loudly.

Why /RUSH-DATA/state/?
    - /RUSH-DATA is the persistent partition created by
      packaging/systemd/rush-livedev-tmpfiles.conf.
    - It survives reboot (it is a real ext4 mount, not tmpfs/overlay).
    - It is already the documented home for "rush-capture/rush-autopilot
      state files" per the tmpfiles.conf comment.

Usage (host):
    from rush_livedev_state import LiveDevState, StateStore
    state = LiveDevState.new(run_id="...", test_command="...", ...)
    StateStore("/RUSH-DATA/state/livedev-state.json").write(state)

Usage (guest):
    store = StateStore("/RUSH-DATA/state/livedev-state.json")
    state = store.read()
    if state.mode == "livedev-test" and state.status == "pending":
        state.status = "running"
        store.update(state)
        ... run tests ...
        state.status = "passed" if rc == 0 else "failed"
        state.exit_code = rc
        store.update(state)

This module deliberately has NO non-stdlib imports so it can be vendored
into the guest image without dependencies.
"""

from __future__ import annotations

import json
import os
import re
import tempfile
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

# Canonical state file path on the guest. The host may use a different path
# when pre-seeding the file (e.g. staging it on a USB before first boot).
DEFAULT_STATE_PATH = "/RUSH-DATA/state/livedev-state.json"

# Schema version. Bump when the shape changes; old code refuses newer schemas.
STATE_SCHEMA_VERSION = 1

# Allowed values for the enumerated fields.
ALLOWED_MODES = {"livedev-test", "idle"}
ALLOWED_STATUS = {"pending", "running", "passed", "failed", "timeout", "skipped"}
ALLOWED_SUBMIT = {"none", "local", "github", "http", "auto"}


_RUN_ID_RE = re.compile(r"^[A-Za-z0-9._-]{1,128}$")


class StateError(Exception):
    """Raised when the state file is missing, malformed, or invalid."""


@dataclass
class LiveDevState:
    """The persistent livedev test-intent record.

    All fields are JSON-serializable. The dataclass is intentionally flat
    so the on-disk shape is stable and easy to read with `jq`.
    """

    schema_version: int = STATE_SCHEMA_VERSION
    mode: str = "idle"  # "livedev-test" | "idle"
    run_id: str = ""
    test_command: str = ""  # command to run after reboot (string, run via sh -c on guest)
    suite: str = "default"  # logical suite name (for labeling)
    artifacts_guest_path: str = "/RUSH-DATA/results/livedev/<run_id>"
    artifacts_host_path: str = ""  # filled by host orchestrator
    submit_mode: str = "local"  # none|local|github|http|auto
    created_at: str = ""  # ISO 8601 UTC
    updated_at: str = ""  # ISO 8601 UTC
    attempt_count: int = 0
    max_attempts: int = 1
    debug: bool = False
    ci: bool = False
    status: str = "pending"  # pending|running|passed|failed|timeout|skipped
    exit_code: int | None = None
    failure_reason: str = ""
    kernel_cmdline_marker: str = ""  # optional: kernel cmdline value to gate on
    extra: dict[str, Any] = field(default_factory=dict)

    # --- constructors -------------------------------------------------------

    @classmethod
    def new(
        cls,
        run_id: str,
        test_command: str,
        suite: str = "default",
        artifacts_guest_path: str = "",
        artifacts_host_path: str = "",
        submit_mode: str = "local",
        debug: bool = False,
        ci: bool = False,
        max_attempts: int = 1,
        extra: dict[str, Any] | None = None,
    ) -> "LiveDevState":
        """Create a fresh pending state for a new livedev run."""
        if not _RUN_ID_RE.match(run_id):
            raise StateError(
                f"invalid run_id {run_id!r}: must match {_RUN_ID_RE.pattern}"
            )
        if not test_command or not test_command.strip():
            raise StateError("test_command must be a non-empty string")
        if submit_mode not in ALLOWED_SUBMIT:
            raise StateError(
                f"invalid submit_mode {submit_mode!r}: must be one of {sorted(ALLOWED_SUBMIT)}"
            )
        now = _now_iso()
        if not artifacts_guest_path:
            artifacts_guest_path = f"/RUSH-DATA/results/livedev/{run_id}"
        return cls(
            schema_version=STATE_SCHEMA_VERSION,
            mode="livedev-test",
            run_id=run_id,
            test_command=test_command,
            suite=suite,
            artifacts_guest_path=artifacts_guest_path,
            artifacts_host_path=artifacts_host_path,
            submit_mode=submit_mode,
            created_at=now,
            updated_at=now,
            attempt_count=0,
            max_attempts=max(1, int(max_attempts)),
            debug=bool(debug),
            ci=bool(ci),
            status="pending",
            exit_code=None,
            extra=dict(extra) if extra else {},
        )

    # --- serialization ------------------------------------------------------

    def to_dict(self) -> dict[str, Any]:
        d = asdict(self)
        # extra is the only nested dict; keep it as-is.
        return d

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "LiveDevState":
        """Parse a dict into a LiveDevState, validating schema and fields."""
        if not isinstance(d, dict):
            raise StateError("state document is not a JSON object")
        sv = d.get("schema_version")
        if sv != STATE_SCHEMA_VERSION:
            raise StateError(
                f"schema_version mismatch: file has {sv!r}, code expects {STATE_SCHEMA_VERSION}"
            )
        mode = d.get("mode", "idle")
        if mode not in ALLOWED_MODES:
            raise StateError(f"invalid mode {mode!r}: must be one of {sorted(ALLOWED_MODES)}")
        status = d.get("status", "pending")
        if status not in ALLOWED_STATUS:
            raise StateError(
                f"invalid status {status!r}: must be one of {sorted(ALLOWED_STATUS)}"
            )
        submit_mode = d.get("submit_mode", "local")
        if submit_mode not in ALLOWED_SUBMIT:
            raise StateError(
                f"invalid submit_mode {submit_mode!r}: must be one of {sorted(ALLOWED_SUBMIT)}"
            )
        run_id = d.get("run_id", "")
        if not _RUN_ID_RE.match(run_id):
            raise StateError(f"invalid run_id {run_id!r}")
        # Construct via __init__ to keep field ordering stable, then validate.
        try:
            obj = cls(
                schema_version=int(d.get("schema_version", STATE_SCHEMA_VERSION)),
                mode=mode,
                run_id=run_id,
                test_command=str(d.get("test_command", "")),
                suite=str(d.get("suite", "default")),
                artifacts_guest_path=str(d.get("artifacts_guest_path", "")),
                artifacts_host_path=str(d.get("artifacts_host_path", "")),
                submit_mode=submit_mode,
                created_at=str(d.get("created_at", "")),
                updated_at=str(d.get("updated_at", "")),
                attempt_count=int(d.get("attempt_count", 0)),
                max_attempts=int(d.get("max_attempts", 1)),
                debug=bool(d.get("debug", False)),
                ci=bool(d.get("ci", False)),
                status=status,
                exit_code=d.get("exit_code"),  # may be None
                failure_reason=str(d.get("failure_reason", "")),
                kernel_cmdline_marker=str(d.get("kernel_cmdline_marker", "")),
                extra=dict(d.get("extra", {})) if isinstance(d.get("extra"), dict) else {},
            )
        except TypeError as e:
            raise StateError(f"state document has wrong shape: {e}") from e
        return obj

    # --- validation ---------------------------------------------------------

    def validate_for_run(self) -> list[str]:
        """Return a list of human-readable validation errors. Empty = OK."""
        errors: list[str] = []
        if self.mode != "livedev-test":
            errors.append(f"mode is {self.mode!r}, expected 'livedev-test'")
        if not self.run_id:
            errors.append("run_id is empty")
        if not self.test_command.strip():
            errors.append("test_command is empty")
        if self.status not in ("pending", "running"):
            errors.append(
                f"status is {self.status!r}, expected 'pending' or 'running' "
                f"(already-terminal runs must not be re-executed)"
            )
        if self.attempt_count >= self.max_attempts:
            errors.append(
                f"attempt_count ({self.attempt_count}) >= max_attempts ({self.max_attempts}); "
                f"refusing to run again"
            )
        return errors

    def is_terminal(self) -> bool:
        return self.status in ("passed", "failed", "timeout", "skipped")


# ─── Atomic store ────────────────────────────────────────────────────────────


class StateStore:
    """Atomic read/write of LiveDevState to a JSON file.

    Writes go to a sibling temp file, are fsync'd, then renamed into place.
    This guarantees that a crash never leaves a partially-written state file.
    """

    def __init__(self, path: str | Path):
        self.path = Path(path)

    def exists(self) -> bool:
        return self.path.exists()

    def read(self) -> LiveDevState:
        """Read and validate the state file. Raises StateError on any problem."""
        if not self.path.exists():
            raise StateError(f"state file does not exist: {self.path}")
        try:
            raw = self.path.read_text(encoding="utf-8")
        except OSError as e:
            raise StateError(f"cannot read state file {self.path}: {e}") from e
        try:
            d = json.loads(raw)
        except json.JSONDecodeError as e:
            raise StateError(f"state file {self.path} is not valid JSON: {e}") from e
        return LiveDevState.from_dict(d)

    def read_or_none(self) -> LiveDevState | None:
        """Like read(), but returns None if the file does not exist."""
        try:
            return self.read()
        except StateError as e:
            if "does not exist" in str(e):
                return None
            raise

    def write(self, state: LiveDevState) -> None:
        """Atomically write state to disk.

        Steps:
          1. Ensure parent dir exists.
          2. Serialize to JSON.
          3. Write to a sibling temp file in the same directory.
          4. fsync the temp file.
          5. Rename temp -> target (atomic on POSIX).
          6. fsync the parent directory (best-effort).
        """
        state.updated_at = _now_iso()
        payload = json.dumps(state.to_dict(), indent=2, sort_keys=True) + "\n"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp_fd, tmp_path = tempfile.mkstemp(
            prefix=f".{self.path.name}.",
            suffix=".tmp",
            dir=str(self.path.parent),
        )
        try:
            with os.fdopen(tmp_fd, "w", encoding="utf-8") as f:
                f.write(payload)
                f.flush()
                os.fsync(f.fileno())
            os.replace(tmp_path, self.path)
            _fsync_dir(self.path.parent)
        except Exception:
            # Best-effort cleanup of the temp file on failure.
            try:
                os.unlink(tmp_path)
            except OSError:
                pass
            raise

    def update(self, mutator) -> LiveDevState:
        """Read-modify-write atomically.

        `mutator` is called with the current LiveDevState and may mutate it
        in place (or return a new one). The result is written atomically.

        This is NOT safe against concurrent writers — there is only one
        livedev runner per guest, and the host only writes before boot.
        """
        state = self.read()
        new_state = mutator(state)
        if new_state is None:
            new_state = state
        self.write(new_state)
        return new_state

    def delete(self) -> None:
        """Remove the state file. Used after a successful run to mark idle."""
        try:
            self.path.unlink()
        except FileNotFoundError:
            pass


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _now_iso() -> str:
    """Return current UTC time as ISO 8601 with 'Z' suffix."""
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def _fsync_dir(path: Path) -> None:
    """Best-effort fsync of a directory (for rename durability)."""
    try:
        fd = os.open(str(path), os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    except (OSError, AttributeError):
        # Not fatal — rename is still atomic, just less durable on crash.
        pass


# ─── CLI (for debugging / one-shot state inspection) ─────────────────────────


def _main() -> int:
    import argparse
    parser = argparse.ArgumentParser(
        prog="rush-livedev-state",
        description="Inspect or create livedev test-intent state.",
    )
    parser.add_argument("--path", default=DEFAULT_STATE_PATH,
                        help=f"state file path (default: {DEFAULT_STATE_PATH})")
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("show", help="print the current state as JSON")
    sub.add_parser("validate", help="validate the current state; exit 0 if valid")
    p_new = sub.add_parser("new", help="create a new pending state (overwrites!)")
    p_new.add_argument("--run-id", required=True)
    p_new.add_argument("--test-command", required=True)
    p_new.add_argument("--suite", default="default")
    p_new.add_argument("--submit", default="local", choices=sorted(ALLOWED_SUBMIT))
    p_new.add_argument("--debug", action="store_true")
    p_new.add_argument("--ci", action="store_true")
    p_new.add_argument("--artifacts-host-path", default="")
    sub.add_parser("clear", help="delete the state file (marks the system idle)")
    ns = parser.parse_args()

    store = StateStore(ns.path)
    if ns.cmd == "show":
        try:
            print(json.dumps(store.read().to_dict(), indent=2, sort_keys=True))
            return 0
        except StateError as e:
            print(f"state error: {e}", flush=True)
            return 1
    if ns.cmd == "validate":
        try:
            s = store.read()
        except StateError as e:
            print(f"state error: {e}", flush=True)
            return 1
        errs = s.validate_for_run()
        if errs:
            print("state is invalid for run:")
            for e in errs:
                print(f"  - {e}")
            return 1
        print("state is valid for run")
        return 0
    if ns.cmd == "new":
        s = LiveDevState.new(
            run_id=ns.run_id,
            test_command=ns.test_command,
            suite=ns.suite,
            submit_mode=ns.submit,
            debug=ns.debug,
            ci=ns.ci,
            artifacts_host_path=ns.artifacts_host_path,
        )
        store.write(s)
        print(f"wrote {ns.path} (run_id={s.run_id})")
        return 0
    if ns.cmd == "clear":
        store.delete()
        print(f"cleared {ns.path}")
        return 0
    return 2


if __name__ == "__main__":
    raise SystemExit(_main())
