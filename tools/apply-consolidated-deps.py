#!/usr/bin/env python3
"""Apply the exact Dependabot updates from PRs #339-#345 and self-remove."""

from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def from_main(path: str) -> str:
    result = subprocess.run(
        ["git", "show", f"origin/main:{path}"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def replace_exact(text: str, old: str, new: str, expected: int, path: str) -> str:
    actual = text.count(old)
    if actual != expected:
        raise SystemExit(
            f"{path}: expected {expected} occurrence(s) of {old!r}, found {actual}"
        )
    return text.replace(old, new)


def write_from_main(path: str, replacements: list[tuple[str, str, int]]) -> None:
    text = from_main(path)
    for old, new, expected in replacements:
        text = replace_exact(text, old, new, expected, path)
    target = ROOT / path
    target.write_text(text, encoding="utf-8")


def main() -> None:
    write_from_main(
        ".github/workflows/stale.yml",
        [("actions/stale@v9", "actions/stale@v10", 1)],
    )

    write_from_main(
        "Cargo.lock",
        [
            (
                'name = "libc"\nversion = "0.2.186"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "68ab91017fe16c622486840e4c83c9a37afeff978bd239b5293d61ece587de66"',
                'name = "libc"\nversion = "0.2.189"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2"',
                1,
            ),
            (
                'name = "serde_json"\nversion = "1.0.150"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "e8014e44b4736ed0538adeecded0fce2a272f22dc9578a7eb6b2d9993c74cfb9"',
                'name = "serde_json"\nversion = "1.0.151"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"',
                1,
            ),
        ],
    )

    for path, count in [
        (".github/workflows/ci.yml", 4),
        (".github/workflows/docker-publish.yml", 1),
        (".github/workflows/maintenance.yml", 2),
        (".github/workflows/pages.yml", 1),
        (".github/workflows/reassess.yml", 1),
        (".github/workflows/release-testos.yml", 3),
    ]:
        replacements: list[tuple[str, str, int]] = [
            ("actions/checkout@v4", "actions/checkout@v7", count)
        ]
        if path == ".github/workflows/docker-publish.yml":
            replacements.append(("docker/metadata-action@v5", "docker/metadata-action@v6", 1))
        if path == ".github/workflows/pages.yml":
            replacements.append(("actions/deploy-pages@v4", "actions/deploy-pages@v5", 1))
        if path == ".github/workflows/release-testos.yml":
            replacements.append(
                ("actions/download-artifact@v4", "actions/download-artifact@v8", 2)
            )
        write_from_main(path, replacements)

    script = ROOT / "tools/apply-consolidated-deps.py"
    script.unlink()

    subprocess.run(["git", "diff", "--check"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
