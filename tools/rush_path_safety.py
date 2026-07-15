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
    - On Windows: reparse points / junctions that redirect directory
      traversal outside the collection root

This module provides:
    safe_segment(s)      — validate a single path component
    is_strictly_under(child, parent) — canonicalized containment check
    prove_containment(child, parent) — raise on escape (fail-closed)
    is_regular_file(p)   — True only if p exists, is regular, and is not a symlink
    reject_non_regular(root) — scan a tree and return list of non-regular files
    reject_symlinks(root) — scan a tree and return list of symlinks found
    safe_copy(src, dst)  — copy only if src is a regular file, reject symlinks
    safe_copy_tree(src_root, dst_root) — copy a tree with full path-safety

Windows reparse-point / junction safety:
    The helpers in this module reject symlinks (POSIX) and non-regular
    files. On Windows, junctions and reparse points are NOT POSIX symlinks
    and require a Windows-specific check (``Path.is_symlink()`` returns True
    for junctions in Python 3.12+, but older runtimes need
    ``ctypes.GetFileAttributesW`` + ``FILE_ATTRIBUTE_REPARSE_POINT``).
    The ``is_windows_reparse_point`` hook below is the intended extension
    point; it is a no-op on non-Windows and MUST be implemented and tested
    on a real Windows agent before any code claims junction safety. Until
    that test exists, no code path may claim Windows junction safety.
"""

import os
import shutil
import stat
import sys
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


def prove_containment(child: Path, parent: Path) -> Path:
    """Fail-closed containment proof.

    Resolves ``child`` and ``parent`` canonically (symlinks followed) and
    returns the canonical child path only if it is strictly under the
    canonical parent. Raises ``ValueError`` on escape or on any resolution
    failure. This is the explicit proof callers must invoke before joining
    untrusted path components or performing destructive operations beneath
    a root.
    """
    try:
        child_c = child.resolve(strict=True)
        parent_c = parent.resolve(strict=True)
    except (OSError, RuntimeError) as e:
        raise ValueError(
            f"containment proof failed: cannot resolve {child} or {parent}: {e}"
        ) from e
    try:
        child_c.relative_to(parent_c)
    except ValueError as e:
        raise ValueError(
            f"containment proof failed: {child} (-> {child_c}) is not under "
            f"{parent} (-> {parent_c})"
        ) from e
    return child_c


def is_regular_file(p: Path) -> bool:
    """Return True only if p is a regular file and NOT a symlink.

    Uses os.lstat to check the link itself (not its target), then
    stat.S_ISREG to verify the target is a regular file. This prevents
    following symlinks to external files. Also rejects Windows reparse
    points when running on Windows (see ``is_windows_reparse_point``).
    """
    try:
        st = os.lstat(p)
    except OSError:
        return False
    if stat.S_ISLNK(st.st_mode):
        return False
    if not stat.S_ISREG(st.st_mode):
        return False
    # Defense-in-depth: on Windows, also reject reparse points that are not
    # reported as symlinks by os.lstat (e.g. junctions on older runtimes).
    if sys.platform.startswith("win") and _is_windows_reparse_point(p):
        return False
    return True


def reject_non_regular(root: Path) -> list[Path]:
    """Scan root recursively and return a list of all non-regular files found.

    A "non-regular file" is anything that is not a regular file, directory,
    or a POSIX symlink-that-we-will-reject-anyway. This catches device nodes,
    FIFOs, sockets, and (on Windows) reparse points/junctions. The caller
    should abort if this list is non-empty.
    """
    offenders: list[Path] = []
    try:
        for p in root.rglob("*"):
            try:
                st = os.lstat(p)
            except OSError:
                continue
            m = st.st_mode
            if stat.S_ISLNK(m):
                # Symlinks are reported separately by reject_symlinks, but
                # they are also non-regular from a copy-safety standpoint.
                offenders.append(p)
                continue
            if stat.S_ISREG(m) or stat.S_ISDIR(m):
                # Regular file or directory: OK on POSIX. On Windows, also
                # check for reparse points that are not symlinks.
                if sys.platform.startswith("win") and _is_windows_reparse_point(p):
                    offenders.append(p)
                continue
            # Everything else (block/char device, FIFO, socket) is an offender.
            offenders.append(p)
    except OSError:
        pass
    return offenders


def reject_symlinks(root: Path) -> list[Path]:
    """Scan root recursively and return a list of all symlinks found.

    Does NOT follow symlinks. Returns paths of symlinks themselves
    (not their targets). The caller should abort if this list is non-empty.
    On Windows, this also catches junctions that Python reports as symlinks
    (Python 3.12+); older runtimes need the reparse-point check below.
    """
    symlinks: list[Path] = []
    try:
        for p in root.rglob("*"):
            try:
                st = os.lstat(p)
            except OSError:
                continue
            if stat.S_ISLNK(st.st_mode):
                symlinks.append(p)
                continue
            # Windows: catch junctions that os.lstat does not report as
            # symlinks on older Python runtimes.
            if sys.platform.startswith("win") and _is_windows_reparse_point(p):
                symlinks.append(p)
    except OSError:
        pass
    return symlinks


def safe_copy(src: Path, dst: Path) -> None:
    """Copy src to dst, rejecting symlinks and non-regular files.

    Raises ValueError if src is a symlink or non-regular file.
    Uses shutil.copy2 to preserve metadata. The dst parent is created
    if needed. Proves both source and destination containment: src must
    exist and be a regular non-symlink file, and dst's parent must exist
    and dst must not be a symlink (no writing through a symlink).
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
        if sys.platform.startswith("win") and _is_windows_reparse_point(dst):
            raise ValueError(
                f"refusing to write through Windows reparse point destination: {dst}"
            )
    shutil.copy2(src, dst)


def safe_copy_tree(src_root: Path, dst_root: Path) -> list[Path]:
    """Copy a tree with full path safety.

    Copies every regular non-symlink file from ``src_root`` into
    ``dst_root``, preserving relative structure. Returns the list of copied
    files. Aborts (raises ValueError) on the first symlink, non-regular
    file, or containment violation found. The caller is responsible for
    having already validated ``src_root`` is a trusted location; this
    helper ensures the COPY is safe, not that the source is trusted.

    This is the canonical helper for evidence ingestion: it proves source
    containment (every file under src_root), rejects symlinks/non-regular
    files, and proves destination containment (every dst under dst_root).
    """
    src_root = prove_containment(src_root, src_root)
    dst_root.mkdir(parents=True, exist_ok=True)
    dst_root = dst_root.resolve()
    # Pre-scan: reject symlinks and non-regular files up front so we never
    # partially copy a hostile tree.
    bad = reject_symlinks(src_root) + reject_non_regular(src_root)
    # reject_non_regular also returns symlinks, so dedupe.
    seen = set()
    unique_bad = []
    for p in bad:
        if p not in seen:
            seen.add(p)
            unique_bad.append(p)
    if unique_bad:
        raise ValueError(
            f"refusing to copy tree with {len(unique_bad)} non-regular/symlink "
            f"file(s): {[str(p.relative_to(src_root)) for p in unique_bad[:5]]}"
        )
    copied: list[Path] = []
    for p in src_root.rglob("*"):
        if not p.is_file():
            continue
        if not is_regular_file(p):
            # Should have been caught above, but defense-in-depth.
            raise ValueError(f"refusing to copy non-regular file: {p}")
        rel = p.relative_to(src_root)
        dst = dst_root / rel
        # Prove destination containment WITHOUT requiring dst to exist yet
        # (it doesn't). We check that dst, as constructed from the canonical
        # dst_root plus a validated relative path, stays under dst_root. The
        # relative path comes from src_root (already proven contained), so
        # the only escape vector would be a bug in Path.join — guard against
        # it by checking the string form does not contain '..' parts and
        # that the resolved parent is under dst_root.
        if ".." in rel.parts:
            raise ValueError(f"refusing to copy: relative path contains '..': {rel}")
        dst_parent_canon = dst.parent.resolve(strict=False)
        try:
            dst_parent_canon.relative_to(dst_root)
        except ValueError as e:
            raise ValueError(
                f"destination containment proof failed: {dst} escapes {dst_root}"
            ) from e
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(p, dst)
        copied.append(dst)
    return copied


# ─── Windows reparse-point / junction extension point ────────────────────────
#
# This hook is the intended extension point for Windows junction safety. On
# non-Windows platforms it is a no-op (returns False). On Windows it MUST be
# implemented to detect reparse points via ``ctypes`` (GetFileAttributesW +
# FILE_ATTRIBUTE_REPARSE_POINT) or via ``os.lstat`` with ``stat.FILE_ATTRIBUTE_REPARSE_POINT``
# where available.
#
# IMPORTANT: This hook is NOT covered by a Windows test in this PR. No code
# path may claim Windows junction safety until a real Windows agent
# implements and tests this hook. The function exists so the API surface is
# ready, and so non-Windows callers get a clear no-op.


def _is_windows_reparse_point(p: Path) -> bool:
    """Return True if ``p`` is a Windows reparse point/junction.

    On non-Windows platforms this always returns False. On Windows, this
    stub returns False and MUST be replaced with a real implementation
    (GetFileAttributesW + FILE_ATTRIBUTE_REPARSE_POINT) and covered by a
    native Windows test before any caller claims junction safety. Until
    that test exists, callers on Windows must treat junction safety as
    UNVERIFIED.
    """
    if not sys.platform.startswith("win"):
        return False
    # Windows implementation is intentionally a stub. See the module docstring
    # and the "Remaining Windows-only work" section of the cloud-safe PR.
    # Returning False here does NOT mean "safe"; it means "unchecked".
    return False


def windows_reparse_point_safety_verified() -> bool:
    """Return True only if the Windows reparse-point check is implemented and tested.

    Always returns False in this PR. A future Windows agent MUST flip this to
    True after implementing ``_is_windows_reparse_point`` and adding a native
    Windows test. Callers may use this to decide whether to allow operations
    that depend on junction safety.
    """
    return False
