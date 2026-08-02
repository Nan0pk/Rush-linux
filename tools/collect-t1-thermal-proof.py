#!/usr/bin/env python3
"""Collect a privacy-safe, read-only T1 thermal verification bundle.

The collector does not certify T1 and never writes thermal, fan, powercap, or
firmware controls. It records the exact source commit, repository checks,
individual mapped T1 acceptance tests, sanitized physical sensor observations,
and the thermal-only portion of optid's production status surface.

A bundle is completion-ready only when an accepted threshold decision is
supplied explicitly. Missing hardware, missing production status, failed checks,
or an absent threshold decision remain visible in ``manifest.json``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Sequence

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / "docs/plans/optid-package-status.toml"
DEFAULT_STATUS_FILE = Path("/run/optid/status")
TEMP_MIN_MILLIC = -40_000
TEMP_MAX_MILLIC = 150_000
SAFE_TOKEN = re.compile(r"[^A-Za-z0-9_.:+-]+")

BASE_COMMANDS: tuple[tuple[str, ...], ...] = (
    ("git", "diff", "--check"),
    ("cargo", "fmt", "--all", "--", "--check"),
    ("cargo", "check", "--workspace", "--all-targets", "--all-features"),
    ("cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"),
    ("cargo", "test", "--workspace"),
    ("python3", "tools/validate-current-work.py"),
    ("python3", "tools/validate-optid-packages.py"),
    ("python3", "tools/validate-optid-packages.py", "--base", "origin/main"),
    ("python3", "tools/render-frontpage.py", "--check"),
    ("bash", "tools/finish-work.sh", "--dry-run"),
    ("bash", "tools/checks.sh", "--ci", "--changed-base", "origin/main"),
)


@dataclass(frozen=True)
class CommandResult:
    command: list[str]
    returncode: int
    stdout: str
    stderr: str

    @property
    def passed(self) -> bool:
        return self.returncode == 0


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def safe_read(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8", errors="replace").strip()
    except OSError:
        return None


def read_int(path: Path) -> int | None:
    text = safe_read(path)
    if text is None:
        return None
    try:
        return int(text)
    except ValueError:
        return None


def sanitize_token(value: str, fallback: str = "unknown") -> str:
    cleaned = SAFE_TOKEN.sub("_", value.strip()).strip("_")
    return cleaned[:120] or fallback


def plausible_temp_millic(value: int | None) -> bool:
    return value is not None and TEMP_MIN_MILLIC <= value <= TEMP_MAX_MILLIC


def sys_path(sys_root: Path, absolute: str) -> Path:
    return sys_root / absolute.lstrip("/")


def _device_token(hwmon: Path) -> str:
    device = hwmon / "device"
    candidate = ""
    if device.exists() or device.is_symlink():
        try:
            candidate = device.resolve(strict=False).name
        except OSError:
            candidate = ""
    return sanitize_token(candidate or safe_read(hwmon / "name") or "device")


def collect_hwmon(sys_root: Path) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    temperatures: list[dict[str, Any]] = []
    fans: list[dict[str, Any]] = []
    hwmon_root = sys_path(sys_root, "/sys/class/hwmon")
    if not hwmon_root.is_dir():
        return temperatures, fans

    for hwmon in sorted(hwmon_root.glob("hwmon*"), key=lambda item: item.name):
        chip = sanitize_token(safe_read(hwmon / "name") or "unknown")
        device = _device_token(hwmon)
        for input_path in sorted(hwmon.glob("temp*_input"), key=lambda item: item.name):
            channel = input_path.name.removesuffix("_input")
            value = read_int(input_path)
            label = sanitize_token(safe_read(hwmon / f"{channel}_label") or channel)
            record: dict[str, Any] = {
                "source": "hwmon",
                "stable_id": f"hwmon:{device}:{chip}:{label}",
                "chip": chip,
                "channel": channel,
                "label": label,
                "readable": value is not None,
                "plausible": plausible_temp_millic(value),
                "temp_millic": value,
                "crit_millic": read_int(hwmon / f"{channel}_crit"),
                "alarm": read_int(hwmon / f"{channel}_alarm"),
                "fault": read_int(hwmon / f"{channel}_fault"),
            }
            temperatures.append(record)

        for input_path in sorted(hwmon.glob("fan*_input"), key=lambda item: item.name):
            channel = input_path.name.removesuffix("_input")
            label = sanitize_token(safe_read(hwmon / f"{channel}_label") or channel)
            fans.append(
                {
                    "source": "hwmon",
                    "stable_id": f"hwmon:{device}:{chip}:{label}",
                    "chip": chip,
                    "channel": channel,
                    "label": label,
                    "rpm": read_int(input_path),
                    "alarm": read_int(hwmon / f"{channel}_alarm"),
                    "fault": read_int(hwmon / f"{channel}_fault"),
                }
            )

    temperatures.sort(key=lambda item: item["stable_id"])
    fans.sort(key=lambda item: item["stable_id"])
    return temperatures, fans


def collect_thermal_zones(sys_root: Path) -> list[dict[str, Any]]:
    zones: list[dict[str, Any]] = []
    thermal_root = sys_path(sys_root, "/sys/class/thermal")
    if not thermal_root.is_dir():
        return zones

    for zone in sorted(thermal_root.glob("thermal_zone*"), key=lambda item: item.name):
        zone_type = sanitize_token(safe_read(zone / "type") or "unknown")
        trips: list[dict[str, Any]] = []
        for trip_type_path in sorted(zone.glob("trip_point_*_type"), key=lambda item: item.name):
            prefix = trip_type_path.name.removesuffix("_type")
            trips.append(
                {
                    "type": sanitize_token(safe_read(trip_type_path) or "unknown"),
                    "temp_millic": read_int(zone / f"{prefix}_temp"),
                }
            )
        zones.append(
            {
                "source": "thermal_zone",
                "stable_id": f"thermal_zone:{zone_type}",
                "type": zone_type,
                "readable": read_int(zone / "temp") is not None,
                "plausible": plausible_temp_millic(read_int(zone / "temp")),
                "temp_millic": read_int(zone / "temp"),
                "trips": trips,
            }
        )
    zones.sort(key=lambda item: item["stable_id"])
    return zones


def collect_observation(sys_root: Path, sample_number: int) -> dict[str, Any]:
    temperatures, fans = collect_hwmon(sys_root)
    return {
        "sample": sample_number,
        "captured_at_utc": utc_now(),
        "monotonic_seconds": round(time.monotonic(), 6),
        "temperatures": temperatures,
        "fans": fans,
        "thermal_zones": collect_thermal_zones(sys_root),
    }


def extract_thermal_status(text: str) -> str:
    selected: list[str] = []
    in_reasons = False
    for raw_line in text.splitlines():
        line = raw_line.rstrip()
        if line == "thermal_reasons:":
            selected.append(line)
            in_reasons = True
            continue
        if in_reasons and line.startswith("- "):
            selected.append(line)
            continue
        if in_reasons:
            in_reasons = False
        if line.startswith("thermal_"):
            selected.append(line)
    return "\n".join(selected) + ("\n" if selected else "")


def t1_acceptance_test_names(ledger_path: Path = LEDGER) -> list[str]:
    with ledger_path.open("rb") as handle:
        ledger = tomllib.load(handle)
    for package in ledger.get("package", []):
        if package.get("id") == "T1":
            mapping = package.get("acceptance_tests", {})
            if not isinstance(mapping, dict) or not mapping:
                raise ValueError("T1 acceptance_tests mapping is missing")
            return [str(value) for value in mapping.values()]
    raise ValueError("T1 package is missing from the ledger")


def t1_acceptance_commands(ledger_path: Path = LEDGER) -> list[list[str]]:
    return [
        [
            "cargo",
            "test",
            "-p",
            "optid",
            "--bin",
            "optid",
            f"thermal::tests::{name}",
            "--",
            "--exact",
        ]
        for name in t1_acceptance_test_names(ledger_path)
    ]


def run_command(command: Sequence[str], cwd: Path) -> CommandResult:
    try:
        result = subprocess.run(
            list(command),
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=3600,
            check=False,
        )
        return CommandResult(list(command), result.returncode, result.stdout, result.stderr)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return CommandResult(list(command), 127, "", str(exc))


def git_output(repo_root: Path, *args: str) -> str:
    result = run_command(["git", *args], repo_root)
    return result.stdout.strip() if result.passed else ""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def validate_privacy(output_dir: Path) -> list[str]:
    errors: list[str] = []
    forbidden = (
        re.compile(r"/home/[^/\s]+"),
        re.compile(r"/Users/[^/\s]+"),
        re.compile(r"(?i)serial(?:_number)?[\s\"':=]+[A-Za-z0-9-]{6,}"),
        re.compile(r"(?i)hostname[\s\"':=]+\S+"),
        re.compile(r"(?i)(?:[0-9a-f]{2}:){5}[0-9a-f]{2}"),
    )
    for path in sorted(output_dir.iterdir()):
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for pattern in forbidden:
            if pattern.search(text):
                errors.append(f"privacy rule matched in {path.name}")
    return errors


def sanitize_command_output(text: str, repo_root: Path) -> str:
    """Redact machine-specific absolute paths from recorded command output."""
    sanitized = text.replace(str(repo_root), "<repo>")
    sanitized = re.sub(r"/home/[^/\s]+", "<home>", sanitized)
    sanitized = re.sub(r"/Users/[^/\s]+", "<home>", sanitized)
    return sanitized


def command_results_json(
    results: Sequence[CommandResult], repo_root: Path
) -> list[dict[str, Any]]:
    return [
        {
            "command": result.command,
            "returncode": result.returncode,
            "passed": result.passed,
            "stdout": sanitize_command_output(result.stdout, repo_root),
            "stderr": sanitize_command_output(result.stderr, repo_root),
        }
        for result in results
    ]


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="New evidence directory")
    parser.add_argument("--samples", type=int, default=5, help="Physical sensor samples")
    parser.add_argument(
        "--interval-seconds", type=float, default=2.0, help="Delay between samples"
    )
    parser.add_argument("--sys-root", type=Path, default=Path("/"), help=argparse.SUPPRESS)
    parser.add_argument("--repo-root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    parser.add_argument("--status-file", type=Path, default=DEFAULT_STATUS_FILE)
    parser.add_argument(
        "--threshold-decision",
        type=Path,
        help="Accepted threshold decision reviewed separately from this collector",
    )
    parser.add_argument(
        "--require-completion-ready",
        action="store_true",
        help="Fail unless hardware, production status, checks, and threshold decision are present",
    )
    parser.add_argument(
        "--skip-command-checks",
        action="store_true",
        help="Developer/test use only; creates an explicitly incomplete bundle",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    repo_root = args.repo_root.resolve()
    output = args.output.resolve()
    if output.exists():
        print(f"ERROR: output path already exists: {output}", file=sys.stderr)
        return 2
    if args.samples < 1 or args.samples > 60:
        print("ERROR: --samples must be between 1 and 60", file=sys.stderr)
        return 2
    if args.interval_seconds < 0 or args.interval_seconds > 60:
        print("ERROR: --interval-seconds must be between 0 and 60", file=sys.stderr)
        return 2

    output.mkdir(parents=True)
    unresolved: list[str] = []
    source_commit = git_output(repo_root, "rev-parse", "HEAD")
    dirty = bool(git_output(repo_root, "status", "--porcelain"))
    if not source_commit:
        unresolved.append("source commit could not be resolved")
    if dirty:
        unresolved.append("checkout is dirty")

    observations: list[dict[str, Any]] = []
    for index in range(args.samples):
        observations.append(collect_observation(args.sys_root, index + 1))
        if index + 1 < args.samples and args.interval_seconds:
            time.sleep(args.interval_seconds)
    observations_path = output / "thermal-observations.jsonl"
    observations_path.write_text(
        "".join(json.dumps(item, sort_keys=True) + "\n" for item in observations),
        encoding="utf-8",
    )

    usable_temperatures = sum(
        1
        for observation in observations
        for sensor in [*observation["temperatures"], *observation["thermal_zones"]]
        if sensor.get("readable") and sensor.get("plausible")
    )
    if usable_temperatures == 0:
        unresolved.append("no plausible physical temperature observation was captured")

    status_text = safe_read(args.status_file)
    thermal_status = extract_thermal_status(status_text or "")
    (output / "optid-thermal-status.txt").write_text(thermal_status, encoding="utf-8")
    required_status_keys = (
        "thermal_state=",
        "thermal_derating_ratio=",
        "thermal_die_sensor=",
    )
    if not thermal_status or any(key not in thermal_status for key in required_status_keys):
        unresolved.append("production optid thermal status is missing or incomplete")

    threshold: dict[str, Any] = {"provided": False}
    if args.threshold_decision:
        decision = args.threshold_decision.resolve()
        if not decision.is_file():
            unresolved.append("threshold decision path does not exist")
        else:
            try:
                decision_rel = decision.relative_to(repo_root)
            except ValueError:
                unresolved.append("threshold decision must be committed inside the repository")
            else:
                tracked = run_command(
                    ["git", "ls-files", "--error-unmatch", "--", str(decision_rel)],
                    repo_root,
                ).passed
                if not tracked:
                    unresolved.append("threshold decision is not tracked by git")
                else:
                    threshold = {
                        "provided": True,
                        "path": decision_rel.as_posix(),
                        "sha256": sha256_file(decision),
                    }
    else:
        unresolved.append("reviewed threshold acceptance is not supplied")
    write_json(output / "threshold-decision-reference.json", threshold)

    command_results: list[CommandResult] = []
    if args.skip_command_checks:
        unresolved.append("repository and mapped acceptance checks were skipped")
    else:
        commands = [list(command) for command in BASE_COMMANDS]
        commands.extend(t1_acceptance_commands(repo_root / LEDGER.relative_to(ROOT)))
        for command in commands:
            result = run_command(command, repo_root)
            command_results.append(result)
            if not result.passed:
                unresolved.append("command failed: " + " ".join(command))
    write_json(
        output / "command-results.json", command_results_json(command_results, repo_root)
    )

    manifest: dict[str, Any] = {
        "schema_version": 1,
        "package": "T1",
        "collector": "tools/collect-t1-thermal-proof.py",
        "captured_at_utc": utc_now(),
        "source_commit": source_commit,
        "checkout_dirty": dirty,
        "kernel_release": platform.release(),
        "sample_count": len(observations),
        "usable_temperature_observations": usable_temperatures,
        "production_status_captured": bool(thermal_status),
        "threshold_decision": threshold,
        "checks_run": len(command_results),
        "checks_passed": sum(result.passed for result in command_results),
        "unresolved": sorted(set(unresolved)),
        "result": "pass" if not unresolved else "incomplete",
        "safety": {
            "read_only": True,
            "hardware_writes": False,
            "fan_writes": False,
            "powercap_writes": False,
        },
    }
    write_json(output / "manifest.json", manifest)

    privacy_errors = validate_privacy(output)
    if privacy_errors:
        manifest["unresolved"] = sorted(set([*manifest["unresolved"], *privacy_errors]))
        manifest["result"] = "incomplete"
        write_json(output / "manifest.json", manifest)

    if args.require_completion_ready and manifest["result"] != "pass":
        for finding in manifest["unresolved"]:
            print(f"BLOCKED: {finding}", file=sys.stderr)
        return 1

    print(f"T1 thermal proof bundle: {output}")
    print(f"Result: {manifest['result']}")
    for finding in manifest["unresolved"]:
        print(f"Unresolved: {finding}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
