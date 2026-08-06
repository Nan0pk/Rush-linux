#!/usr/bin/env python3
"""Install, activate, list, and remove Rush Linux edition system extensions."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

STATE_KIND = "rush-linux-edition-sysext-install"
PLAN_KIND = "rush-linux-edition-sysext"
BUILD_KIND = "rush-linux-edition-sysext-build"
STATE_DIR = Path("var/lib/rush-linux/editions")
EXTENSION_DIR = Path("var/lib/extensions")
SYSTEMD_WANTS = Path("etc/systemd/system/sysinit.target.wants")
SYSTEMD_UNIT_CANDIDATES = (
    Path("usr/lib/systemd/system/systemd-sysext.service"),
    Path("lib/systemd/system/systemd-sysext.service"),
)


class DeployError(RuntimeError):
    """A user-facing deployment error."""


def sha256_path(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_json(path: Path, *, description: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise DeployError(f"cannot read {description} {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise DeployError(f"{description} {path} must contain a JSON object")
    return value


def require_string(mapping: dict[str, Any], key: str, *, context: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value:
        raise DeployError(f"{context}.{key} must be a non-empty string")
    return value


def parse_os_release(path: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeDecodeError) as exc:
        raise DeployError(f"cannot read host identity {path}: {exc}") from exc
    for raw in lines:
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {'"', "'"}:
            value = value[1:-1]
        result[key.strip()] = value
    return result


def host_identity(root: Path) -> dict[str, str]:
    for relative in (Path("etc/os-release"), Path("usr/lib/os-release")):
        candidate = root / relative
        if candidate.is_file():
            identity = parse_os_release(candidate)
            if identity.get("ID") and identity.get("VERSION_ID"):
                return identity
            raise DeployError(f"{candidate} must define ID and VERSION_ID")
    raise DeployError(f"no os-release found below target root {root}")


def validate_inputs(
    *,
    plan_path: Path,
    receipt_path: Path,
    artifact_override: Path | None,
    root: Path,
    allow_unsigned: bool,
) -> tuple[dict[str, Any], dict[str, Any], Path, str, str]:
    plan = load_json(plan_path, description="edition plan")
    receipt = load_json(receipt_path, description="build receipt")

    if plan.get("schema_version") != 1 or plan.get("kind") != PLAN_KIND:
        raise DeployError(f"unsupported edition plan schema in {plan_path}")
    if receipt.get("schema_version") != 1 or receipt.get("kind") != BUILD_KIND:
        raise DeployError(f"unsupported build receipt schema in {receipt_path}")

    extension = plan.get("extension")
    edition = plan.get("edition")
    base_os = plan.get("base_os")
    artifact_info = receipt.get("artifact")
    if not all(isinstance(item, dict) for item in (extension, edition, base_os, artifact_info)):
        raise DeployError("plan/receipt is missing required object sections")

    extension_id = require_string(extension, "id", context="plan.extension")
    filename = require_string(extension, "filename", context="plan.extension")
    if filename != f"{extension_id}.raw":
        raise DeployError("plan extension filename must match extension id")
    if Path(filename).name != filename:
        raise DeployError("plan extension filename must not contain a directory")

    receipt_extension = require_string(receipt, "extension_id", context="receipt")
    if receipt_extension != extension_id:
        raise DeployError("receipt extension_id does not match plan")
    edition_name = require_string(edition, "name", context="plan.edition")
    if receipt.get("edition") != edition_name:
        raise DeployError("receipt edition does not match plan")
    if receipt.get("edition_version") != plan.get("edition_version"):
        raise DeployError("receipt edition version does not match plan")

    expected_plan_hash = require_string(receipt, "plan_sha256", context="receipt")
    if sha256_path(plan_path) != expected_plan_hash:
        raise DeployError("edition plan hash does not match build receipt")

    signed = receipt.get("signed")
    if not isinstance(signed, bool):
        raise DeployError("receipt.signed must be a boolean")
    if not signed and not allow_unsigned:
        raise DeployError(
            "refusing unsigned edition artifact; pass --allow-unsigned-development "
            "only for non-release roots"
        )

    artifact_name = require_string(artifact_info, "path", context="receipt.artifact")
    artifact = artifact_override or (receipt_path.parent / artifact_name)
    artifact = artifact.resolve()
    if not artifact.is_file():
        raise DeployError(f"edition artifact does not exist: {artifact}")
    expected_hash = require_string(artifact_info, "sha256", context="receipt.artifact")
    if sha256_path(artifact) != expected_hash:
        raise DeployError("edition artifact SHA-256 does not match build receipt")
    expected_size = artifact_info.get("size_bytes")
    if not isinstance(expected_size, int) or expected_size < 0:
        raise DeployError("receipt.artifact.size_bytes must be a non-negative integer")
    if artifact.stat().st_size != expected_size:
        raise DeployError("edition artifact size does not match build receipt")

    identity = host_identity(root)
    expected_id = require_string(base_os, "id", context="plan.base_os")
    expected_version = require_string(base_os, "version_id", context="plan.base_os")
    if identity["ID"] != expected_id:
        raise DeployError(
            f"target root ID={identity['ID']!r} does not match extension ID={expected_id!r}"
        )
    if identity["VERSION_ID"] != expected_version:
        raise DeployError(
            "target root VERSION_ID="
            f"{identity['VERSION_ID']!r} does not match extension VERSION_ID={expected_version!r}"
        )

    return plan, receipt, artifact, extension_id, filename


def fsync_directory(path: Path) -> None:
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def atomic_copy(source: Path, destination: Path, *, mode: int = 0o644) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{destination.name}.", suffix=".tmp", dir=destination.parent
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as output, source.open("rb") as input_file:
            shutil.copyfileobj(input_file, output, length=1024 * 1024)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, destination)
        fsync_directory(destination.parent)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def atomic_write_json(destination: Path, payload: dict[str, Any]) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{destination.name}.", suffix=".tmp", dir=destination.parent
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True, ensure_ascii=False)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary, 0o644)
        os.replace(temporary, destination)
        fsync_directory(destination.parent)
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def enable_boot_activation(root: Path) -> tuple[Path, bool]:
    unit_relative: Path | None = None
    for candidate in SYSTEMD_UNIT_CANDIDATES:
        if (root / candidate).is_file():
            unit_relative = candidate
            break
    if unit_relative is None:
        raise DeployError(
            "target root does not contain systemd-sysext.service under /usr/lib or /lib"
        )

    wants = root / SYSTEMD_WANTS
    wants.mkdir(parents=True, exist_ok=True)
    link = wants / "systemd-sysext.service"
    expected = Path("/") / unit_relative
    if link.is_symlink() and Path(os.readlink(link)) == expected:
        return link, False
    if link.exists() or link.is_symlink():
        raise DeployError(f"refusing to replace existing boot activation path: {link}")
    link.symlink_to(expected)
    fsync_directory(wants)
    return link, True


def run_refresh(root: Path, command: str) -> None:
    try:
        completed = subprocess.run(
            [command, f"--root={root}", "refresh"],
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise DeployError(f"cannot execute {command!r}: {exc}") from exc
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()
        suffix = f": {detail}" if detail else ""
        raise DeployError(
            f"systemd-sysext refresh failed with status {completed.returncode}{suffix}"
        )


def install_extension(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    if not root.is_dir():
        raise DeployError(f"target root is not a directory: {root}")
    plan_path = args.plan.resolve()
    receipt_path = args.receipt.resolve()
    plan, receipt, artifact, extension_id, filename = validate_inputs(
        plan_path=plan_path,
        receipt_path=receipt_path,
        artifact_override=args.artifact.resolve() if args.artifact else None,
        root=root,
        allow_unsigned=args.allow_unsigned_development,
    )

    destination = root / EXTENSION_DIR / filename
    state_path = root / STATE_DIR / f"{extension_id}.json"
    prior_artifact: bytes | None = None
    prior_state: bytes | None = None
    if destination.exists():
        current_hash = sha256_path(destination)
        new_hash = receipt["artifact"]["sha256"]
        if current_hash == new_hash:
            prior_artifact = destination.read_bytes()
        elif not args.force:
            raise DeployError(
                f"different extension artifact already installed at {destination}; pass --force"
            )
        else:
            prior_artifact = destination.read_bytes()
    if state_path.exists():
        prior_state = state_path.read_bytes()

    activation_link: Path | None = None
    activation_link_created = False
    try:
        atomic_copy(artifact, destination)
        activation_link, activation_link_created = enable_boot_activation(root)
        state = {
            "schema_version": 1,
            "kind": STATE_KIND,
            "extension_id": extension_id,
            "edition": plan["edition"]["name"],
            "edition_version": plan["edition_version"],
            "base_os": plan["base_os"],
            "artifact": {
                "path": f"/{EXTENSION_DIR.as_posix()}/{filename}",
                "size_bytes": destination.stat().st_size,
                "sha256": sha256_path(destination),
            },
            "signed": receipt["signed"],
            "certificate_sha256": receipt.get("certificate_sha256"),
            "plan_sha256": receipt["plan_sha256"],
            "installed_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "boot_activation": "/etc/systemd/system/sysinit.target.wants/systemd-sysext.service",
        }
        atomic_write_json(state_path, state)
        if args.activate:
            run_refresh(root, args.systemd_sysext)
        return state
    except Exception:
        if prior_artifact is None:
            destination.unlink(missing_ok=True)
        else:
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(prior_artifact)
        if prior_state is None:
            state_path.unlink(missing_ok=True)
        else:
            state_path.parent.mkdir(parents=True, exist_ok=True)
            state_path.write_bytes(prior_state)
        if activation_link_created and activation_link is not None:
            activation_link.unlink(missing_ok=True)
        raise


def remove_extension(args: argparse.Namespace) -> dict[str, Any]:
    root = args.root.resolve()
    state_path = root / STATE_DIR / f"{args.extension_id}.json"
    state = load_json(state_path, description="installed extension state")
    if state.get("kind") != STATE_KIND or state.get("extension_id") != args.extension_id:
        raise DeployError(f"invalid installed extension state: {state_path}")
    artifact_info = state.get("artifact")
    if not isinstance(artifact_info, dict):
        raise DeployError(f"installed extension state lacks artifact metadata: {state_path}")
    raw_path = require_string(artifact_info, "path", context="state.artifact")
    destination = root / raw_path.lstrip("/")
    if not destination.is_file():
        raise DeployError(f"installed extension artifact is missing: {destination}")

    artifact_bytes = destination.read_bytes()
    state_bytes = state_path.read_bytes()
    try:
        destination.unlink()
        state_path.unlink()
        if args.activate:
            run_refresh(root, args.systemd_sysext)
    except Exception:
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(artifact_bytes)
        state_path.parent.mkdir(parents=True, exist_ok=True)
        state_path.write_bytes(state_bytes)
        raise

    return {
        "extension_id": args.extension_id,
        "removed": True,
        "artifact": raw_path,
    }


def list_extensions(root: Path) -> list[dict[str, Any]]:
    state_dir = root.resolve() / STATE_DIR
    if not state_dir.exists():
        return []
    result: list[dict[str, Any]] = []
    for path in sorted(state_dir.glob("*.json")):
        state = load_json(path, description="installed extension state")
        if state.get("kind") != STATE_KIND:
            continue
        result.append(state)
    return result


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Deploy Rush Linux edition system extensions into a target root."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    install = subparsers.add_parser("install", help="Atomically install an edition extension.")
    install.add_argument("--plan", type=Path, required=True)
    install.add_argument("--receipt", type=Path, required=True)
    install.add_argument("--artifact", type=Path)
    install.add_argument("--root", type=Path, default=Path("/"))
    install.add_argument("--force", action="store_true")
    install.add_argument("--activate", action="store_true")
    install.add_argument("--systemd-sysext", default="systemd-sysext")
    install.add_argument("--allow-unsigned-development", action="store_true")

    remove = subparsers.add_parser("remove", help="Remove an installed edition extension.")
    remove.add_argument("--extension-id", required=True)
    remove.add_argument("--root", type=Path, default=Path("/"))
    remove.add_argument("--activate", action="store_true")
    remove.add_argument("--systemd-sysext", default="systemd-sysext")

    listing = subparsers.add_parser("list", help="List installed Rush edition extensions.")
    listing.add_argument("--root", type=Path, default=Path("/"))
    listing.add_argument("--json", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = create_parser().parse_args(argv)
    try:
        if args.command == "install":
            payload: Any = install_extension(args)
        elif args.command == "remove":
            payload = remove_extension(args)
        else:
            payload = list_extensions(args.root)
            if not args.json:
                for item in payload:
                    print(
                        f"{item['extension_id']}\t{item['edition_version']}\t"
                        f"signed={str(item['signed']).lower()}"
                    )
                return 0
        print(json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False))
        return 0
    except DeployError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
