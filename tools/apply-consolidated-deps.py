#!/usr/bin/env python3
"""Apply the exact Dependabot updates from PRs #339-#345 and self-remove."""

from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(args), cwd=ROOT, check=True, capture_output=True, text=True
    )


def from_main(path: str) -> str:
    return run("git", "show", f"refs/remotes/origin/main:{path}").stdout


def replace_required(text: str, old: str, new: str, path: str) -> str:
    if old not in text:
        raise SystemExit(f"{path}: required source value not found: {old!r}")
    updated = text.replace(old, new)
    if old in updated or new not in updated:
        raise SystemExit(f"{path}: replacement postcondition failed")
    return updated


def write_from_main(path: str, replacements: list[tuple[str, str]]) -> None:
    text = from_main(path)
    for old, new in replacements:
        text = replace_required(text, old, new, path)
    target = ROOT / path
    target.write_text(text, encoding="utf-8")


def main() -> None:
    run("git", "fetch", "--no-tags", "origin", "main:refs/remotes/origin/main")

    write_from_main(
        ".github/workflows/stale.yml",
        [("actions/stale@v9", "actions/stale@v10")],
    )

    write_from_main(
        "Cargo.lock",
        [
            (
                'name = "libc"\nversion = "0.2.186"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "68ab91017fe16c622486840e4c83c9a37afeff978bd239b5293d61ece587de66"',
                'name = "libc"\nversion = "0.2.189"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2"',
            ),
            (
                'name = "serde_json"\nversion = "1.0.150"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "e8014e44b4736ed0538adeecded0fce2a272f22dc9578a7eb6b2d9993c74cfb9"',
                'name = "serde_json"\nversion = "1.0.151"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"',
            ),
        ],
    )

    workflow_replacements: dict[str, list[tuple[str, str]]] = {
        ".github/workflows/ci.yml": [
            ("actions/checkout@v4", "actions/checkout@v7")
        ],
        ".github/workflows/docker-publish.yml": [
            ("actions/checkout@v4", "actions/checkout@v7"),
            ("docker/metadata-action@v5", "docker/metadata-action@v6"),
        ],
        ".github/workflows/maintenance.yml": [
            ("actions/checkout@v4", "actions/checkout@v7")
        ],
        ".github/workflows/pages.yml": [
            ("actions/checkout@v4", "actions/checkout@v7"),
            ("actions/deploy-pages@v4", "actions/deploy-pages@v5"),
        ],
        ".github/workflows/reassess.yml": [
            ("actions/checkout@v4", "actions/checkout@v7")
        ],
        ".github/workflows/release-testos.yml": [
            ("actions/checkout@v4", "actions/checkout@v7"),
            ("actions/download-artifact@v4", "actions/download-artifact@v8"),
        ],
    }
    for path, replacements in workflow_replacements.items():
        write_from_main(path, replacements)

    (ROOT / "tools/apply-consolidated-deps.py").unlink()
    run("git", "diff", "--check")


if __name__ == "__main__":
    main()
