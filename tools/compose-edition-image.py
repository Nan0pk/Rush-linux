#!/usr/bin/env python3
"""Compose one bootable Rush Linux edition image from one common base plus a sysext."""

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

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BASE_BUILDER = REPO_ROOT / "tools" / "build-mkosi-image.sh"
DEFAULT_SYSEXT_BUILDER = REPO_ROOT / "tools" / "build-edition-sysext.py"
DEFAULT_DEPLOYER = REPO_ROOT / "tools" / "deploy-edition-sysext.py"
DEFAULT_BUILD_DIR = REPO_ROOT / "build"
COMPOSE_KIND = "rush-linux-composed-edition-image"


class ComposeError(RuntimeError):
    """A user-facing image composition error."""


def sha256_path(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_checked(command: list[str], *, cwd: Path | None = None) -> None:
    try:
        completed = subprocess.run(command, cwd=cwd, text=True, check=False)
    except OSError as exc:
        raise ComposeError(f"cannot execute {command[0]!r}: {exc}") from exc
    if completed.returncode != 0:
        rendered = " ".join(command)
        raise ComposeError(f"command failed with status {completed.returncode}: {rendered}")


def load_json(path: Path, *, description: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ComposeError(f"cannot read {description} {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise ComposeError(f"{description} {path} must contain a JSON object")
    return value


def require_string(mapping: dict[str, Any], key: str, *, context: str) -> str:
    value = mapping.get(key)
    if not isinstance(value, str) or not value:
        raise ComposeError(f"{context}.{key} must be a non-empty string")
    return value


def resolve_base_image(args: argparse.Namespace) -> tuple[Path, bool]:
    if args.base_image:
        base = args.base_image.resolve()
        if not base.is_file():
            raise ComposeError(f"base image does not exist: {base}")
        return base, False

    builder = args.base_builder.resolve()
    if not builder.is_file():
        raise ComposeError(f"base image builder does not exist: {builder}")
    command = ["bash", str(builder), "--edition", "server"]
    if args.clean_base:
        command.append("--clean")
    run_checked(command, cwd=REPO_ROOT)

    # The unprofiled common base is canonical. Keep the legacy server-named
    # artifact as a compatibility fallback only, so a stale pre-sysext build
    # cannot override the base that was just produced.
    candidates = (
        args.build_dir.resolve() / "rush-linux.raw",
        args.build_dir.resolve() / "rush-linux-server.raw",
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate.resolve(), True
    expected = ", ".join(str(item) for item in candidates)
    raise ComposeError(f"base builder completed but no base image was found; expected {expected}")


def build_extension(
    args: argparse.Namespace, base_image: Path
) -> tuple[dict[str, Any], Path, Path, Path]:
    builder = args.sysext_builder.resolve()
    if not builder.is_file():
        raise ComposeError(f"edition sysext builder does not exist: {builder}")

    workspace = args.workspace.resolve()
    command = [
        sys.executable,
        str(builder),
        "build",
        "--edition",
        args.edition,
        "--workspace",
        str(workspace),
        "--base-tree",
        str(base_image),
        "--mkosi",
        args.mkosi,
        "--force",
    ]
    if args.key or args.certificate:
        if not args.key or not args.certificate:
            raise ComposeError("--key and --certificate must be supplied together")
        command.extend(["--key", str(args.key.resolve())])
        command.extend(["--certificate", str(args.certificate.resolve())])
    elif args.unsigned_development:
        command.append("--unsigned-development")
    else:
        raise ComposeError(
            "release composition requires --key and --certificate; "
            "use --unsigned-development only for local non-release images"
        )

    run_checked(command, cwd=REPO_ROOT)

    plan_path = workspace / "edition-plan.json"
    plan = load_json(plan_path, description="edition plan")
    extension = plan.get("extension")
    if not isinstance(extension, dict):
        raise ComposeError("edition plan lacks extension metadata")
    extension_id = require_string(extension, "id", context="plan.extension")
    filename = require_string(extension, "filename", context="plan.extension")
    artifact = workspace / "output" / filename
    receipt = workspace / "output" / f"{extension_id}.build.json"
    if not artifact.is_file():
        raise ComposeError(f"sysext builder did not produce artifact: {artifact}")
    if not receipt.is_file():
        raise ComposeError(f"sysext builder did not produce receipt: {receipt}")
    return plan, plan_path, artifact, receipt


def copy_base_image(base: Path, temporary_output: Path) -> None:
    temporary_output.parent.mkdir(parents=True, exist_ok=True)
    command = ["cp", "--reflink=auto", "--sparse=always", str(base), str(temporary_output)]
    run_checked(command)
    if not temporary_output.is_file():
        raise ComposeError(f"base image copy did not appear: {temporary_output}")


def deploy_into_image(
    *,
    args: argparse.Namespace,
    image: Path,
    plan_path: Path,
    artifact: Path,
    receipt: Path,
) -> None:
    deployer = args.deployer.resolve()
    if not deployer.is_file():
        raise ComposeError(f"edition deployer does not exist: {deployer}")

    mount_parent = args.mount_parent.resolve() if args.mount_parent else image.parent
    mount_parent.mkdir(parents=True, exist_ok=True)
    mount_dir = Path(tempfile.mkdtemp(prefix=".rush-edition-mount-", dir=mount_parent))
    mount_completed = False
    mounted = False
    failure: Exception | None = None
    try:
        run_checked(
            [args.systemd_dissect, "--mount", str(image), str(mount_dir)]
        )
        mount_completed = True
        mounted = True
        deploy = [
            sys.executable,
            str(deployer),
            "install",
            "--plan",
            str(plan_path),
            "--receipt",
            str(receipt),
            "--artifact",
            str(artifact),
            "--root",
            str(mount_dir),
        ]
        if args.unsigned_development:
            deploy.append("--allow-unsigned-development")
        run_checked(deploy, cwd=REPO_ROOT)
    except Exception as exc:
        failure = exc

    if mounted:
        try:
            run_checked([args.systemd_dissect, "--umount", str(mount_dir)])
            mounted = False
        except ComposeError as exc:
            if failure is not None:
                raise ComposeError(
                    f"{failure}; additionally failed to unmount composed image: {exc}"
                ) from failure
            raise ComposeError(f"failed to unmount composed image: {exc}") from exc

    try:
        if mount_completed:
            shutil.rmtree(mount_dir)
        else:
            mount_dir.rmdir()
    except OSError as exc:
        cleanup = ComposeError(f"failed to remove mount directory {mount_dir}: {exc}")
        if failure is not None:
            raise ComposeError(f"{failure}; additionally {cleanup}") from failure
        raise cleanup from exc

    if failure is not None:
        raise failure


def write_receipt(
    *,
    args: argparse.Namespace,
    output: Path,
    base_image: Path,
    base_was_built: bool,
    plan: dict[str, Any],
    plan_path: Path,
    artifact: Path,
    build_receipt: Path,
) -> Path:
    extension = plan["extension"]
    receipt = {
        "schema_version": 1,
        "kind": COMPOSE_KIND,
        "edition": plan["edition"]["name"],
        "edition_version": plan["edition_version"],
        "extension_id": extension["id"],
        "base": {
            "path": str(base_image),
            "built_by_composer": base_was_built,
            "size_bytes": base_image.stat().st_size,
            "sha256": sha256_path(base_image),
        },
        "extension": {
            "artifact": artifact.name,
            "artifact_sha256": sha256_path(artifact),
            "plan_sha256": sha256_path(plan_path),
            "build_receipt_sha256": sha256_path(build_receipt),
        },
        "output": {
            "path": output.name,
            "size_bytes": output.stat().st_size,
            "sha256": sha256_path(output),
        },
        "unsigned_development": args.unsigned_development,
        "composed_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    receipt_path = output.with_suffix(output.suffix + ".compose.json")
    temporary = receipt_path.with_name(f".{receipt_path.name}.{os.getpid()}.tmp")
    temporary.write_text(
        json.dumps(receipt, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, receipt_path)
    return receipt_path


def compose(args: argparse.Namespace) -> tuple[Path, Path]:
    output = args.output.resolve()
    if output.exists() and not args.force:
        raise ComposeError(f"output already exists: {output}; pass --force to replace it")

    base_image, base_was_built = resolve_base_image(args)
    if output == base_image:
        raise ComposeError("output must not overwrite the common base image")

    plan, plan_path, artifact, build_receipt = build_extension(args, base_image)

    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{output.name}.", suffix=".tmp", dir=output.parent
    )
    os.close(descriptor)
    temporary_output = Path(temporary_name)
    temporary_output.unlink()
    try:
        copy_base_image(base_image, temporary_output)
        deploy_into_image(
            args=args,
            image=temporary_output,
            plan_path=plan_path,
            artifact=artifact,
            receipt=build_receipt,
        )
        os.replace(temporary_output, output)
        receipt = write_receipt(
            args=args,
            output=output,
            base_image=base_image,
            base_was_built=base_was_built,
            plan=plan,
            plan_path=plan_path,
            artifact=artifact,
            build_receipt=build_receipt,
        )
        return output, receipt
    except Exception:
        temporary_output.unlink(missing_ok=True)
        raise


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Build one common Rush Linux base, build one edition sysext, and "
            "compose them into a bootable edition image."
        )
    )
    parser.add_argument("--edition", required=True)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--workspace", type=Path)
    parser.add_argument("--base-image", type=Path)
    parser.add_argument("--base-builder", type=Path, default=DEFAULT_BASE_BUILDER)
    parser.add_argument("--sysext-builder", type=Path, default=DEFAULT_SYSEXT_BUILDER)
    parser.add_argument("--deployer", type=Path, default=DEFAULT_DEPLOYER)
    parser.add_argument("--build-dir", type=Path, default=DEFAULT_BUILD_DIR)
    parser.add_argument("--mount-parent", type=Path)
    parser.add_argument("--mkosi", default="mkosi")
    parser.add_argument("--systemd-dissect", default="systemd-dissect")
    parser.add_argument("--key", type=Path)
    parser.add_argument("--certificate", type=Path)
    parser.add_argument("--unsigned-development", action="store_true")
    parser.add_argument("--clean-base", action="store_true")
    parser.add_argument("--force", action="store_true")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = create_parser().parse_args(argv)
    if args.output is None:
        args.output = args.build_dir / f"rush-linux-{args.edition}.raw"
    if args.workspace is None:
        args.workspace = args.build_dir / f"edition-{args.edition}"
    try:
        output, receipt = compose(args)
        print(output)
        print(receipt)
        return 0
    except ComposeError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
