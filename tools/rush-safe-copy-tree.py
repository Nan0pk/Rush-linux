#!/usr/bin/env python3
"""Fail-closed CLI wrapper around rush_path_safety.safe_copy_tree."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from rush_path_safety import safe_copy_tree


def main() -> int:
    parser = argparse.ArgumentParser(description="Safely copy an evidence tree.")
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    try:
        copied = safe_copy_tree(args.source, args.destination)
    except (OSError, ValueError) as exc:
        print(f"rush-safe-copy-tree: REFUSED: {exc}", file=sys.stderr)
        return 1
    print(f"rush-safe-copy-tree: copied {len(copied)} regular file(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
