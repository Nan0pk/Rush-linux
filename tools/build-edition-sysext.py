#!/usr/bin/env python3
"""Build Rush Linux edition system extensions from canonical edition manifests."""

from __future__ import annotations

import argparse
import configparser
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST_DIR = REPO_ROOT / "distro" / "editions"
DEFAULT_PROFILE_DIR = REPO_ROOT / "mkosi" / "mkosi.profiles"
DEFAULT_VERSION_FILE = REPO_ROOT / "VERSION"
WORKSPACE_MARKER = ".rush-edition-sysext-workspace.json"
EDITION_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
PACKAGE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9@+._:-]*$")
ARCHITECTURE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
VERSION_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9.+_~-]*$")
ALLOWED_TOP_LEVEL = {"edition", "defaults", "packages"}
ALLOWED_EDITION_KEYS = {"name", "description"}
ALLOWED_PACKAGE_KEYS = {"required"}
SCALAR_TYPES = (str, int, bool, float)


class EditionError(RuntimeError):
    """A user-facing manifest or build error."""


@dataclass(frozen=True)
class EditionManifest:
    name: str
    description: str
    defaults: dict[str, str | int | bool | float]
    packages: tuple[str, ...]
    source: Path
    source_sha256: str


@dataclass(frozen=True)
class EditionProfile:
    name: str
    image_id: str
    packages: tuple[str, ...]
    source: Path
    source_sha256: str


@dataclass(frozen=True)
class ResolvedEdition:
    name: str
    description: str
    defaults: dict[str, str | int | bool | float]
    packages: tuple[str, ...]
    inheritance: tuple[str, ...]
    sources: tuple[EditionManifest, ...]


def sha256_path(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_version(path: Path) -> str:
    try:
        value = path.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise EditionError(f"cannot read version file {path}: {exc}") from exc
    if not value or not VERSION_RE.fullmatch(value):
        raise EditionError(
            f"invalid version in {path}; expected a single token safe for os-release"
        )
    return value


def base_version(version: str) -> str:
    return version.split("-", 1)[0]


def validate_edition_name(name: str, *, context: str) -> None:
    if not EDITION_RE.fullmatch(name):
        raise EditionError(
            f"{context}: edition name must use lowercase letters, digits, and single hyphens"
        )


def load_manifest(path: Path) -> EditionManifest:
    try:
        raw_bytes = path.read_bytes()
        data = tomllib.loads(raw_bytes.decode("utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise EditionError(f"cannot parse edition manifest {path}: {exc}") from exc

    unknown_top = sorted(set(data) - ALLOWED_TOP_LEVEL)
    if unknown_top:
        raise EditionError(f"{path}: unknown top-level keys: {', '.join(unknown_top)}")

    edition = data.get("edition")
    defaults = data.get("defaults", {})
    packages = data.get("packages")
    if not isinstance(edition, dict):
        raise EditionError(f"{path}: [edition] table is required")
    if not isinstance(defaults, dict):
        raise EditionError(f"{path}: [defaults] must be a table")
    if not isinstance(packages, dict):
        raise EditionError(f"{path}: [packages] table is required")

    unknown_edition = sorted(set(edition) - ALLOWED_EDITION_KEYS)
    if unknown_edition:
        raise EditionError(f"{path}: unknown [edition] keys: {', '.join(unknown_edition)}")
    unknown_packages = sorted(set(packages) - ALLOWED_PACKAGE_KEYS)
    if unknown_packages:
        raise EditionError(f"{path}: unknown [packages] keys: {', '.join(unknown_packages)}")

    name = edition.get("name")
    description = edition.get("description")
    if not isinstance(name, str) or not name:
        raise EditionError(f"{path}: edition.name must be a non-empty string")
    validate_edition_name(name, context=str(path))
    if path.stem != name:
        raise EditionError(f"{path}: filename must match edition.name={name!r}")
    if not isinstance(description, str) or not description.strip():
        raise EditionError(f"{path}: edition.description must be a non-empty string")

    clean_defaults: dict[str, str | int | bool | float] = {}
    for key, value in defaults.items():
        if not isinstance(key, str) or not EDITION_RE.fullmatch(key.replace("_", "-")):
            raise EditionError(f"{path}: invalid defaults key {key!r}")
        if not isinstance(value, SCALAR_TYPES):
            raise EditionError(f"{path}: defaults.{key} must be a scalar value")
        if isinstance(value, str) and not value.strip():
            raise EditionError(f"{path}: defaults.{key} must not be empty")
        clean_defaults[key] = value

    required = packages.get("required")
    if not isinstance(required, list) or not required:
        raise EditionError(f"{path}: packages.required must be a non-empty array")
    clean_packages: list[str] = []
    for package in required:
        if not isinstance(package, str) or not PACKAGE_RE.fullmatch(package):
            raise EditionError(f"{path}: invalid package name {package!r}")
        clean_packages.append(package)
    duplicates = sorted({item for item in clean_packages if clean_packages.count(item) > 1})
    if duplicates:
        raise EditionError(f"{path}: duplicate required packages: {', '.join(duplicates)}")

    return EditionManifest(
        name=name,
        description=description.strip(),
        defaults=clean_defaults,
        packages=tuple(clean_packages),
        source=path,
        source_sha256=hashlib.sha256(raw_bytes).hexdigest(),
    )


def load_profile(name: str, profile_dir: Path) -> EditionProfile:
    validate_edition_name(name, context="profile")
    path = profile_dir / name / "mkosi.conf"
    try:
        raw_bytes = path.read_bytes()
        text = raw_bytes.decode("utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        raise EditionError(f"cannot read mkosi profile {path}: {exc}") from exc

    parser = configparser.ConfigParser(
        interpolation=None,
        strict=True,
        empty_lines_in_values=True,
    )
    parser.optionxform = str
    try:
        parser.read_string(text, source=str(path))
    except configparser.Error as exc:
        raise EditionError(f"cannot parse mkosi profile {path}: {exc}") from exc

    image_id = parser.get("Output", "ImageId", fallback="").strip()
    expected_image_id = f"rush-linux-{name}"
    if image_id != expected_image_id:
        raise EditionError(
            f"{path}: Output.ImageId must be {expected_image_id!r}, found {image_id!r}"
        )

    raw_packages = parser.get("Content", "Packages", fallback="")
    packages = [item for item in raw_packages.split() if item]
    invalid = [item for item in packages if not PACKAGE_RE.fullmatch(item)]
    if invalid:
        raise EditionError(f"{path}: invalid profile package names: {', '.join(invalid)}")
    duplicates = sorted({item for item in packages if packages.count(item) > 1})
    if duplicates:
        raise EditionError(f"{path}: duplicate profile packages: {', '.join(duplicates)}")

    return EditionProfile(
        name=name,
        image_id=image_id,
        packages=tuple(sorted(packages)),
        source=path,
        source_sha256=hashlib.sha256(raw_bytes).hexdigest(),
    )


def resolve_edition(name: str, manifest_dir: Path) -> ResolvedEdition:
    validate_edition_name(name, context="command line")
    cache: dict[str, ResolvedEdition] = {}

    def resolve(current: str, stack: tuple[str, ...]) -> ResolvedEdition:
        if current in cache:
            return cache[current]
        if current in stack:
            cycle = " -> ".join((*stack, current))
            raise EditionError(f"edition inheritance cycle: {cycle}")

        manifest = load_manifest(manifest_dir / f"{current}.toml")
        inherited = manifest.defaults.get("inherits")
        if inherited is not None and not isinstance(inherited, str):
            raise EditionError(f"{manifest.source}: defaults.inherits must be a string")

        effective_defaults = dict(manifest.defaults)
        effective_defaults.pop("inherits", None)
        # packages.required is the complete payload for each edition. Inheritance
        # merges policy defaults only; unioning package sets would silently keep
        # replaced components (for example linux-adaptive beside linux-adaptive-rt).
        packages = list(manifest.packages)
        inheritance: list[str] = []
        sources: list[EditionManifest] = []

        if inherited:
            validate_edition_name(inherited, context=f"{manifest.source}: defaults.inherits")
            parent = resolve(inherited, (*stack, current))
            effective_defaults = {**parent.defaults, **effective_defaults}
            inheritance.extend(parent.inheritance)
            sources.extend(parent.sources)

        inheritance.append(current)
        sources.append(manifest)
        result = ResolvedEdition(
            name=manifest.name,
            description=manifest.description,
            defaults=dict(sorted(effective_defaults.items())),
            packages=tuple(sorted(set(packages))),
            inheritance=tuple(inheritance),
            sources=tuple(sources),
        )
        cache[current] = result
        return result

    return resolve(name, ())


def canonical_plan(
    edition: ResolvedEdition,
    profile: EditionProfile,
    *,
    manifest_dir: Path,
    profile_dir: Path,
    version: str,
    architecture: str,
) -> dict[str, Any]:
    if not ARCHITECTURE_RE.fullmatch(architecture):
        raise EditionError(f"invalid systemd architecture identifier: {architecture!r}")
    if edition.name != profile.name:
        raise EditionError(
            f"edition/profile mismatch: {edition.name!r} != {profile.name!r}"
        )
    extension_id = profile.image_id
    return {
        "schema_version": 1,
        "kind": "rush-linux-edition-sysext",
        "edition": {
            "name": edition.name,
            "description": edition.description,
            "inheritance": list(edition.inheritance),
            "effective_defaults": edition.defaults,
        },
        "extension": {
            "id": extension_id,
            "filename": f"{extension_id}.raw",
            "format": "sysext",
            "overlay": True,
            "signing_required_for_release": True,
        },
        "base_os": {
            "id": "rush-linux",
            "version_id": base_version(version),
            "architecture": architecture,
        },
        "edition_version": version,
        "packages": {
            "profile_mode": "complete-sysext-payload",
            "sysext": list(profile.packages),
            "edition_requirements": list(edition.packages),
        },
        "sources": {
            "edition_manifests": [
                {
                    "path": source.source.relative_to(manifest_dir).as_posix(),
                    "sha256": source.source_sha256,
                }
                for source in edition.sources
            ],
            "mkosi_profile": {
                "path": profile.source.relative_to(profile_dir).as_posix(),
                "sha256": profile.source_sha256,
            },
        },
    }


def render_shared_config() -> str:
    return """# Generated by tools/build-edition-sysext.py; do not edit.
[Output]
Format=none
OutputDirectory=output
CacheDirectory=cache
"""


def render_image_config(plan: dict[str, Any]) -> str:
    packages = "\n".join(f"    {item}" for item in plan["packages"]["sysext"])
    extension_id = plan["extension"]["id"]
    filename = plan["extension"]["filename"]
    return f"""# Generated by tools/build-edition-sysext.py; do not edit.
[Distribution]
Distribution=arch

[Output]
Format=sysext
Overlay=yes
ImageId={extension_id}
Output={filename}

[Content]
BaseTrees=base
ExtraTrees=tree
CleanPackageMetadata=no
Packages=
{packages}
"""


def render_extension_release(plan: dict[str, Any]) -> str:
    base = plan["base_os"]
    extension = plan["extension"]
    pretty = (
        f"Rush Linux {plan['edition']['name']} system extension "
        f"{plan['edition_version']}"
    )
    escaped_pretty = pretty.replace("\\", "\\\\").replace('"', '\\"')
    return (
        f"ID={base['id']}\n"
        f"VERSION_ID={base['version_id']}\n"
        f"ARCHITECTURE={base['architecture']}\n"
        f"SYSEXT_ID={extension['id']}\n"
        f"SYSEXT_VERSION_ID={plan['edition_version']}\n"
        f'PRETTY_NAME="{escaped_pretty}"\n'
    )


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


def ensure_workspace(workspace: Path, *, force: bool) -> None:
    if workspace.exists() and any(workspace.iterdir()):
        marker = workspace / WORKSPACE_MARKER
        if not force:
            raise EditionError(f"workspace is not empty: {workspace}; pass --force to replace it")
        if not marker.is_file():
            raise EditionError(
                f"refusing to replace unmarked directory {workspace}; "
                f"expected {WORKSPACE_MARKER}"
            )
        shutil.rmtree(workspace)
    workspace.mkdir(parents=True, exist_ok=True)


def prepare_workspace(
    *,
    plan: dict[str, Any],
    workspace: Path,
    base_tree: Path,
    force: bool,
) -> None:
    base_tree = base_tree.resolve()
    workspace = workspace.resolve()
    if not base_tree.exists():
        raise EditionError(f"base tree/image does not exist: {base_tree}")
    if workspace == Path("/"):
        raise EditionError("workspace must not be /")
    if base_tree == workspace or base_tree.is_relative_to(workspace):
        raise EditionError("base tree/image must not be inside the generated workspace")

    ensure_workspace(workspace, force=force)
    marker = {
        "schema_version": 1,
        "kind": "rush-linux-edition-sysext-workspace",
        "extension_id": plan["extension"]["id"],
    }
    write_json(workspace / WORKSPACE_MARKER, marker)
    write_json(workspace / "edition-plan.json", plan)
    (workspace / "mkosi.conf").write_text(render_shared_config(), encoding="utf-8")

    image_dir = workspace / "mkosi.images" / plan["extension"]["id"]
    image_dir.mkdir(parents=True)
    (image_dir / "mkosi.conf").write_text(render_image_config(plan), encoding="utf-8")

    tree = workspace / "tree"
    release_dir = tree / "usr" / "lib" / "extension-release.d"
    release_dir.mkdir(parents=True)
    release_path = release_dir / f"extension-release.{plan['extension']['id']}"
    release_path.write_text(render_extension_release(plan), encoding="utf-8")

    edition_payload = tree / "usr" / "lib" / "rush-linux" / "editions"
    write_json(edition_payload / f"{plan['edition']['name']}.json", plan)

    base_link = workspace / "base"
    base_link.symlink_to(base_tree, target_is_directory=base_tree.is_dir())


def temporary_signing_links(
    workspace: Path,
    key: Path | None,
    certificate: Path | None,
) -> tuple[list[Path], str | None]:
    if (key is None) != (certificate is None):
        raise EditionError("--key and --certificate must be supplied together")
    if key is None or certificate is None:
        return [], None

    key = key.resolve()
    certificate = certificate.resolve()
    if not key.is_file():
        raise EditionError(f"signing key does not exist: {key}")
    if not certificate.is_file():
        raise EditionError(f"signing certificate does not exist: {certificate}")

    links: list[Path] = []
    try:
        for name, target in (("mkosi.key", key), ("mkosi.crt", certificate)):
            link = workspace / name
            if link.exists() or link.is_symlink():
                raise EditionError(f"refusing to replace existing signing path: {link}")
            link.symlink_to(target)
            links.append(link)
    except Exception:
        for link in links:
            link.unlink(missing_ok=True)
        raise
    return links, sha256_path(certificate)


def run_build(
    *,
    workspace: Path,
    plan: dict[str, Any],
    mkosi: str,
    key: Path | None,
    certificate: Path | None,
    unsigned_development: bool,
) -> Path:
    if key is not None and unsigned_development:
        raise EditionError(
            "--unsigned-development cannot be combined with signing credentials"
        )
    if key is None and not unsigned_development:
        raise EditionError(
            "release builds require --key and --certificate; "
            "use --unsigned-development only for local non-release builds"
        )

    links, certificate_sha256 = temporary_signing_links(workspace, key, certificate)
    try:
        result = subprocess.run(
            [mkosi, "-f"],
            cwd=workspace,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            raise EditionError(f"mkosi failed with exit status {result.returncode}")
    except OSError as exc:
        raise EditionError(f"cannot execute mkosi command {mkosi!r}: {exc}") from exc
    finally:
        for link in links:
            link.unlink(missing_ok=True)

    artifact = workspace / "output" / plan["extension"]["filename"]
    if not artifact.is_file():
        raise EditionError(f"mkosi succeeded but expected artifact is missing: {artifact}")

    plan_path = workspace / "edition-plan.json"
    receipt = {
        "schema_version": 1,
        "kind": "rush-linux-edition-sysext-build",
        "extension_id": plan["extension"]["id"],
        "edition": plan["edition"]["name"],
        "edition_version": plan["edition_version"],
        "artifact": {
            "path": artifact.name,
            "size_bytes": artifact.stat().st_size,
            "sha256": sha256_path(artifact),
        },
        "plan_sha256": sha256_path(plan_path),
        "signed": key is not None,
        "certificate_sha256": certificate_sha256,
        "built_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    write_json(workspace / "output" / f"{plan['extension']['id']}.build.json", receipt)
    (workspace / "output" / f"{artifact.name}.sha256").write_text(
        f"{receipt['artifact']['sha256']}  {artifact.name}\n",
        encoding="utf-8",
    )
    return artifact


def edition_names(manifest_dir: Path, profile_dir: Path) -> list[str]:
    names: list[str] = []
    for path in sorted(manifest_dir.glob("*.toml")):
        manifest = load_manifest(path)
        load_profile(manifest.name, profile_dir)
        names.append(manifest.name)
    if not names:
        raise EditionError(f"no edition manifests found in {manifest_dir}")

    # Operational profiles such as livedev and testos are intentionally not
    # product editions and therefore do not require distro/editions manifests.
    return names

def add_common_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--edition", required=True)
    parser.add_argument("--manifest-dir", type=Path, default=DEFAULT_MANIFEST_DIR)
    parser.add_argument("--profile-dir", type=Path, default=DEFAULT_PROFILE_DIR)
    parser.add_argument("--version-file", type=Path, default=DEFAULT_VERSION_FILE)
    parser.add_argument("--architecture", default="x86-64")


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Build Rush Linux edition system extensions from canonical manifests."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    list_parser = subparsers.add_parser("list", help="List valid edition/profile pairs.")
    list_parser.add_argument("--manifest-dir", type=Path, default=DEFAULT_MANIFEST_DIR)
    list_parser.add_argument("--profile-dir", type=Path, default=DEFAULT_PROFILE_DIR)

    plan_parser = subparsers.add_parser("plan", help="Print a canonical resolved build plan.")
    add_common_arguments(plan_parser)
    plan_parser.add_argument("--output", type=Path)

    prepare_parser = subparsers.add_parser(
        "prepare", help="Generate a deterministic mkosi sysext workspace."
    )
    add_common_arguments(prepare_parser)
    prepare_parser.add_argument("--workspace", type=Path, required=True)
    prepare_parser.add_argument("--base-tree", type=Path, required=True)
    prepare_parser.add_argument("--force", action="store_true")

    build_parser = subparsers.add_parser(
        "build", help="Prepare and build a signed mkosi sysext image."
    )
    add_common_arguments(build_parser)
    build_parser.add_argument("--workspace", type=Path, required=True)
    build_parser.add_argument("--base-tree", type=Path, required=True)
    build_parser.add_argument("--force", action="store_true")
    build_parser.add_argument("--mkosi", default="mkosi")
    build_parser.add_argument("--key", type=Path)
    build_parser.add_argument("--certificate", type=Path)
    build_parser.add_argument("--unsigned-development", action="store_true")

    return parser


def resolved_plan(args: argparse.Namespace) -> dict[str, Any]:
    manifest_dir = args.manifest_dir.resolve()
    profile_dir = args.profile_dir.resolve()
    edition = resolve_edition(args.edition, manifest_dir)
    profile = load_profile(args.edition, profile_dir)
    version = read_version(args.version_file.resolve())
    return canonical_plan(
        edition,
        profile,
        manifest_dir=manifest_dir,
        profile_dir=profile_dir,
        version=version,
        architecture=args.architecture,
    )


def main(argv: list[str] | None = None) -> int:
    args = create_parser().parse_args(argv)
    try:
        if args.command == "list":
            for name in edition_names(
                args.manifest_dir.resolve(),
                args.profile_dir.resolve(),
            ):
                print(name)
            return 0

        plan = resolved_plan(args)
        if args.command == "plan":
            rendered = json.dumps(plan, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
            if args.output:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(rendered, encoding="utf-8")
            else:
                print(rendered, end="")
            return 0

        prepare_workspace(
            plan=plan,
            workspace=args.workspace.resolve(),
            base_tree=args.base_tree,
            force=args.force,
        )
        if args.command == "prepare":
            print(args.workspace.resolve())
            return 0

        artifact = run_build(
            workspace=args.workspace.resolve(),
            plan=plan,
            mkosi=args.mkosi,
            key=args.key,
            certificate=args.certificate,
            unsigned_development=args.unsigned_development,
        )
        print(artifact)
        return 0
    except EditionError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
