#!/usr/bin/env python3
"""Build an atomic set of Rush Linux edition images from one common base."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BASE_BUILDER = REPO_ROOT / "tools" / "build-mkosi-image.sh"
DEFAULT_COMPOSER = REPO_ROOT / "tools" / "compose-edition-image.py"
DEFAULT_SYSEXT_BUILDER = REPO_ROOT / "tools" / "build-edition-sysext.py"
DEFAULT_DEPLOYER = REPO_ROOT / "tools" / "deploy-edition-sysext.py"
DEFAULT_BUILD_DIR = REPO_ROOT / "build"
EDITION_RE = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")
SET_KIND = "rush-linux-edition-image-set"


class SetBuildError(RuntimeError):
    """A user-facing edition-set build failure."""


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
        raise SetBuildError(f"cannot execute {command[0]!r}: {exc}") from exc
    if completed.returncode != 0:
        raise SetBuildError(
            f"command failed with status {completed.returncode}: {' '.join(command)}"
        )


def run_capture(command: list[str], *, cwd: Path | None = None) -> str:
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise SetBuildError(f"cannot execute {command[0]!r}: {exc}") from exc
    if completed.returncode != 0:
        detail = completed.stderr.strip()
        suffix = f": {detail}" if detail else ""
        raise SetBuildError(
            f"command failed with status {completed.returncode}: "
            f"{' '.join(command)}{suffix}"
        )
    return completed.stdout


def load_json(path: Path, *, description: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise SetBuildError(f"cannot read {description} {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise SetBuildError(f"{description} {path} must contain a JSON object")
    return value


def discover_editions(args: argparse.Namespace) -> list[str]:
    if args.editions:
        names = list(args.editions)
    else:
        builder = args.sysext_builder.resolve()
        if not builder.is_file():
            raise SetBuildError(f"edition sysext builder does not exist: {builder}")
        output = run_capture([sys.executable, str(builder), "list"], cwd=REPO_ROOT)
        names = [line.strip() for line in output.splitlines() if line.strip()]

    if not names:
        raise SetBuildError("no product editions were selected or discovered")
    invalid = [name for name in names if not EDITION_RE.fullmatch(name)]
    if invalid:
        raise SetBuildError(f"invalid edition names: {', '.join(invalid)}")
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        raise SetBuildError(f"duplicate edition names: {', '.join(duplicates)}")
    return names


def resolve_base_image(args: argparse.Namespace) -> tuple[Path, bool]:
    if args.base_image:
        base = args.base_image.resolve()
        if not base.is_file():
            raise SetBuildError(f"base image does not exist: {base}")
        return base, False

    builder = args.base_builder.resolve()
    if not builder.is_file():
        raise SetBuildError(f"base image builder does not exist: {builder}")
    command = ["bash", str(builder), "--edition", "server"]
    if args.clean_base:
        command.append("--clean")
    run_checked(command, cwd=REPO_ROOT)

    # Prefer the canonical unprofiled common base. The legacy server-named
    # artifact remains only as a compatibility fallback for older builders.
    candidates = (
        args.build_dir.resolve() / "rush-linux.raw",
        args.build_dir.resolve() / "rush-linux-server.raw",
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate.resolve(), True
    raise SetBuildError(
        "base builder completed without producing rush-linux.raw or "
        "rush-linux-server.raw"
    )


def composer_command(
    *,
    args: argparse.Namespace,
    edition: str,
    base_image: Path,
    output: Path,
    workspace: Path,
) -> list[str]:
    composer = args.composer.resolve()
    if not composer.is_file():
        raise SetBuildError(f"edition image composer does not exist: {composer}")
    command = [
        sys.executable,
        str(composer),
        "--edition",
        edition,
        "--base-image",
        str(base_image),
        "--output",
        str(output),
        "--workspace",
        str(workspace),
        "--sysext-builder",
        str(args.sysext_builder.resolve()),
        "--deployer",
        str(args.deployer.resolve()),
        "--mkosi",
        args.mkosi,
        "--systemd-dissect",
        args.systemd_dissect,
    ]
    if args.key or args.certificate:
        if not args.key or not args.certificate:
            raise SetBuildError("--key and --certificate must be supplied together")
        command.extend(["--key", str(args.key.resolve())])
        command.extend(["--certificate", str(args.certificate.resolve())])
    elif args.unsigned_development:
        command.append("--unsigned-development")
    else:
        raise SetBuildError(
            "release edition sets require --key and --certificate; "
            "use --unsigned-development only for local non-release sets"
        )
    return command


def build_set(args: argparse.Namespace) -> tuple[Path, Path]:
    editions = discover_editions(args)
    output_dir = args.output_dir.resolve()
    if output_dir.exists() and not args.force:
        raise SetBuildError(
            f"output directory already exists: {output_dir}; pass --force to replace it"
        )

    base_image, base_was_built = resolve_base_image(args)
    workspace_root = args.workspace_root.resolve()
    workspace_root.mkdir(parents=True, exist_ok=True)

    output_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(prefix=f".{output_dir.name}.", suffix=".tmp", dir=output_dir.parent)
    )
    try:
        images: list[dict[str, Any]] = []
        edition_version: str | None = None
        for edition in editions:
            output = staging / f"rush-linux-{edition}.raw"
            workspace = workspace_root / edition
            command = composer_command(
                args=args,
                edition=edition,
                base_image=base_image,
                output=output,
                workspace=workspace,
            )
            run_checked(command, cwd=REPO_ROOT)

            compose_receipt_path = output.with_suffix(output.suffix + ".compose.json")
            compose_receipt = load_json(
                compose_receipt_path, description="composition receipt"
            )
            if compose_receipt.get("kind") != "rush-linux-composed-edition-image":
                raise SetBuildError(
                    f"unexpected composition receipt kind for edition {edition}"
                )
            current_version = compose_receipt.get("edition_version")
            if not isinstance(current_version, str) or not current_version:
                raise SetBuildError(
                    f"composition receipt lacks edition_version for {edition}"
                )
            if edition_version is None:
                edition_version = current_version
            elif edition_version != current_version:
                raise SetBuildError(
                    f"edition version mismatch: {edition_version} != {current_version}"
                )
            if not output.is_file():
                raise SetBuildError(f"composer did not produce edition image: {output}")
            images.append(
                {
                    "edition": edition,
                    "filename": output.name,
                    "size_bytes": output.stat().st_size,
                    "sha256": sha256_path(output),
                    "composition_receipt": compose_receipt_path.name,
                    "composition_receipt_sha256": sha256_path(compose_receipt_path),
                }
            )

        assert edition_version is not None
        index = {
            "schema_version": 1,
            "kind": SET_KIND,
            "edition_version": edition_version,
            "base": {
                "path": str(base_image),
                "built_by_set_builder": base_was_built,
                "size_bytes": base_image.stat().st_size,
                "sha256": sha256_path(base_image),
            },
            "unsigned_development": args.unsigned_development,
            "images": images,
            "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        }
        index_path = staging / "edition-set.json"
        index_path.write_text(
            json.dumps(index, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )

        if output_dir.exists():
            backup = output_dir.with_name(f".{output_dir.name}.{os.getpid()}.previous")
            if backup.exists():
                shutil.rmtree(backup)
            os.replace(output_dir, backup)
            try:
                os.replace(staging, output_dir)
            except Exception:
                os.replace(backup, output_dir)
                raise
            shutil.rmtree(backup)
        else:
            os.replace(staging, output_dir)
        return output_dir, output_dir / "edition-set.json"
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Build all selected Rush Linux editions from one common base image."
    )
    parser.add_argument("--editions", nargs="+")
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--workspace-root", type=Path)
    parser.add_argument("--base-image", type=Path)
    parser.add_argument("--base-builder", type=Path, default=DEFAULT_BASE_BUILDER)
    parser.add_argument("--composer", type=Path, default=DEFAULT_COMPOSER)
    parser.add_argument("--sysext-builder", type=Path, default=DEFAULT_SYSEXT_BUILDER)
    parser.add_argument("--deployer", type=Path, default=DEFAULT_DEPLOYER)
    parser.add_argument("--build-dir", type=Path, default=DEFAULT_BUILD_DIR)
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
    if args.output_dir is None:
        args.output_dir = args.build_dir / "edition-set"
    if args.workspace_root is None:
        args.workspace_root = args.build_dir / "edition-workspaces"
    try:
        output_dir, index = build_set(args)
        print(output_dir)
        print(index)
        return 0
    except SetBuildError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
