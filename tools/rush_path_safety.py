#!/usr/bin/env python3
"""
rush_path_safety.py — shared path-safety utilities for evidence collection.

All evidence-boundary code (collection, privacy scanning, submission)
must use these helpers to reject symlinks and non-regular files and to
prove path containment beneath the expected root.

Threat model:
    A hostile USB or compromised manifest may include:
    - Symlinks pointing outside the collection root (e.g., to /etc/shadow)
    - Absolute paths that escape via PathBuf::join / os.path.join
    - Traversal components (../..) in identifiers
    - Non-regular files (device nodes, FIFOs) that could hang reads

This module provides:
    safe_segment(s)      — validate a single path component
    is_strictly_under(child, parent) — canonicalized containment check
    is_regular_file(p)   — True only if p exists, is regular, and is not a symlink
    reject_symlinks(root) — scan a tree and return list of symlinks found
    safe_copy(src, dst)  — copy only if src is a regular file, reject symlinks
"""

import os
import shutil
from pathlib import Path


def safe_segment(s: str) -> bool:
    """Return True if s is safe to use as a single path component.

    Rejects: empty, '.', '..', leading dash, path separators (/ \\),
    NUL bytes, and any byte outside [A-Za-z0-9_.:-+].
    """
    if not s or s == "." or s == "..":
        return False
    if s.startswith("-"):
        return False
    if "/" in s or "\\" in s:
        return False
    if "\0" in s:
        return False
    for c in s:
        if not (c.isalnum() or c in "_.:-+"):
            return False
    return True


def is_strictly_under(child: Path, parent: Path) -> bool:
    """Return True if child is strictly under parent after canonicalization.

    Both paths are resolved (symlinks followed) and then checked with
    Path.relative_to. Returns False if either path cannot be resolved.
    """
    try:
        child_c = child.resolve(strict=True)
        parent_c = parent.resolve(strict=True)
    except (OSError, RuntimeError):
        return False
    try:
        child_c.relative_to(parent_c)
        return True
    except ValueError:
        return False


def is_regular_file(p: Path) -> bool:
    """Return True only if p is a regular file and NOT a symlink.

    Uses os.lstat to check the link itself (not its target), then
    stat.S_ISREG to verify the target is a regular file. This prevents
    following symlinks to external files.
    """
    try:
        st = os.lstat(p)
    except OSError:
        return False
    import stat
    if stat.S_ISLNK(st.st_mode):
        return False
    if not stat.S_ISREG(st.st_mode):
        return False
    return True


def reject_symlinks(root: Path) -> list[Path]:
    """Scan root recursively and return a list of all symlinks found.

    Does NOT follow symlinks. Returns paths of symlinks themselves
    (not their targets). The caller should abort if this list is non-empty.
    """
    symlinks: list[Path] = []
    try:
        for p in root.rglob("*"):
            try:
                st = os.lstat(p)
            except OSError:
                continue
            import stat
            if stat.S_ISLNK(st.st_mode):
                symlinks.append(p)
    except OSError:
        pass
    return symlinks


def safe_copy(src: Path, dst: Path) -> None:
    """Copy src to dst, rejecting symlinks and non-regular files.

    Raises ValueError if src is a symlink or non-regular file.
    Uses shutil.copy2 to preserve metadata. The dst parent is created
    if needed.
    """
    if not is_regular_file(src):
        raise ValueError(
            f"refusing to copy non-regular file or symlink: {src}"
        )
    dst.parent.mkdir(parents=True, exist_ok=True)
    # Verify dst is not a symlink either (don't overwrite through a symlink)
    if dst.exists() or dst.is_symlink():
        if dst.is_symlink():
            raise ValueError(
                f"refusing to write through symlink destination: {dst}"
            )
    shutil.copy2(src, dst)
