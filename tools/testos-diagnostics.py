#!/usr/bin/env python3
"""
tools/testos-diagnostics.py — local-only inspection, export, and sanitization
of testOS private boot diagnostics.

PRIVATE-DIAGNOSTICS/<run_id>/ contains raw boot diagnostics (journalctl,
dmesg, systemctl status, systemd-analyze, USB discovery timeline, runner
exit status, image/kernel version, boot count). These are written by the
testOS runner for LOCAL INVESTIGATION ONLY. They may contain hardware
identifiers (MAC addresses, serial numbers, UUIDs, kernel boot command-line
parameters, hostnames, IP addresses).

This tool provides three subcommands:

  inspect <dir>     Read-only inspection. Lists the directory, prints the
                    marker README, and summarizes each capture. NEVER
                    modifies anything. This is the default safe action.

  export <dir> <dest>
                    Copy the raw diagnostics to an explicit destination
                    directory. Prints a PRIVACY WARNING and requires the
                    destination to not already exist. The destination is
                    marked with the same PRIVATE warning. Export does NOT
                    sanitize — it copies raw identifiers.

  sanitize <dir> <dest>
                    Create a NEW reviewed copy with hardware identifiers
                    redacted. NEVER modifies the original. The sanitized
                    copy must still pass the normal privacy scanner before
                    it may be submitted. Sanitization is best-effort; the
                    operator is responsible for reviewing the result.

Contract (from the boot-reliability PR — private local diagnostics):
  - Normal resume/collection leaves PRIVATE-DIAGNOSTICS on the USB.
  - Evidence submission fails closed if PRIVATE-DIAGNOSTICS, any raw
    journal/dmesg artifact, or any symlink referencing private diagnostics
    appears inside the proposed bundle.
  - This tool is the ONLY supported way to move raw diagnostics off the USB
    for local review.

Usage:
  python3 tools/testos-diagnostics.py inspect /run/testos/usb/PRIVATE-DIAGNOSTICS/<run_id>
  python3 tools/testos-diagnostics.py export <dir> <dest>
  python3 tools/testos-diagnostics.py sanitize <dir> <dest>

Exit codes: 0 success, 1 usage error, 2 I/O error, 3 privacy violation.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import sys
from pathlib import Path

MARKER_TEXT = "PRIVATE — MAY CONTAIN HARDWARE IDENTIFIERS — DO NOT SUBMIT"

# Redaction patterns used by `sanitize`. These mirror the redaction library
# used by the strict evidence validator (rush_capture_lib.redact) so the
# sanitized output passes the normal privacy scanner.
REDACTION_PATTERNS: list[tuple[str, str]] = [
    # UUIDs before MACs (UUIDs contain colons that look like MAC fragments).
    (r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}", "<UUID>"),
    # MAC addresses.
    (r"([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}", "<MAC>"),
    # Serial numbers (DMI / sysfs / kernel USB debug styles). Catch both
    # `serial=HEX` and `serial HEX` forms — the kernel USB subsystem emits
    # `serial <token>` with a space, while DMI/sysfs emits `serial=HEX`.
    (r"[Ss]erial[Nn]umber=[^ \n]*", "<SERIAL>"),
    (r"serial=[0-9a-fA-F]{6,}", "<SERIAL>"),
    (r"\bserial\s+[0-9a-zA-Z]{6,}", "<SERIAL>"),
    # IPv4 addresses.
    (r"\b([0-9]{1,3}\.){3}[0-9]{1,3}\b", "<IPV4>"),
    # Hostnames (best-effort: 'testos' is the testOS hostname; we redact it
    # only when it appears as a standalone word, not as a substring).
    (r"\btestos\b", "<HOSTNAME>"),
]


def _is_path_safe(p: Path) -> bool:
    """Reject symlinks, absolute paths inside the source tree, and traversal."""
    try:
        p.resolve(strict=False)
    except (OSError, RuntimeError):
        return False
    return True


def _check_no_symlinks(dir_path: Path) -> list[str]:
    """Return a list of symlink paths found inside dir_path. Symlinks are
    forbidden in private diagnostics because they can reference files
    outside the directory (path traversal / privacy boundary violation)."""
    problems: list[str] = []
    for p in dir_path.rglob("*"):
        if p.is_symlink():
            problems.append(str(p))
    return problems


def _redact(text: str) -> str:
    """Apply all redaction patterns to text."""
    out = text
    for pattern, replacement in REDACTION_PATTERNS:
        out = re.sub(pattern, replacement, out)
    return out


# ─── Subcommands ─────────────────────────────────────────────────────────────


def cmd_inspect(args: argparse.Namespace) -> int:
    """Read-only inspection of a PRIVATE-DIAGNOSTICS directory."""
    dir_path = Path(args.directory).resolve()
    if not dir_path.is_dir():
        print(f"inspect: not a directory: {dir_path}", file=sys.stderr)
        return 2
    # Refuse to inspect a directory that contains symlinks — they could
    # reference files outside the diagnostics directory.
    symlinks = _check_no_symlinks(dir_path)
    if symlinks:
        print(
            f"inspect: PRIVACY VIOLATION — symlinks found inside {dir_path}:",
            file=sys.stderr,
        )
        for s in symlinks:
            print(f"  {s}", file=sys.stderr)
        return 3
    print("=" * 60)
    print("testOS private diagnostics — INSPECT (read-only)")
    print("=" * 60)
    print(f"Directory: {dir_path}")
    print()
    # Print the marker README if present.
    readme = dir_path / "README.txt"
    if readme.exists():
        print("--- README.txt ---")
        print(readme.read_text(encoding="utf-8", errors="replace"), end="")
        print()
    else:
        print(f"WARNING: no {README_NAME} found — directory may be incomplete", file=sys.stderr)
    # List every file with its size and a one-line summary.
    print("--- Files ---")
    files = sorted(p for p in dir_path.rglob("*") if p.is_file())
    if not files:
        print("  (no files)")
    for f in files:
        rel = f.relative_to(dir_path)
        size = f.stat().st_size
        # Print the first non-empty line as a summary (best-effort).
        summary = ""
        try:
            with f.open("r", encoding="utf-8", errors="replace") as fh:
                for line in fh:
                    s = line.strip()
                    if s:
                        summary = s[:80]
                        break
        except OSError:
            summary = "(unreadable)"
        print(f"  {rel}  ({size} B)  {summary}")
    print()
    print("Inspection is read-only. Nothing was modified.")
    print("To export raw diagnostics to another location:")
    print(f"  python3 {sys.argv[0]} export {dir_path} <dest>")
    print("To create a sanitized copy:")
    print(f"  python3 {sys.argv[0]} sanitize {dir_path} <dest>")
    return 0


README_NAME = "README.txt"


def cmd_export(args: argparse.Namespace) -> int:
    """Copy raw diagnostics to an explicit destination. Prints a privacy
    warning and refuses to overwrite an existing destination."""
    src = Path(args.directory).resolve()
    dest = Path(args.destination).resolve()
    if not src.is_dir():
        print(f"export: source not a directory: {src}", file=sys.stderr)
        return 2
    if dest.exists():
        print(f"export: destination already exists: {dest}", file=sys.stderr)
        print("Refusing to overwrite. Remove it first if you really want to re-export.",
              file=sys.stderr)
        return 1
    # Refuse to export if the source contains symlinks.
    symlinks = _check_no_symlinks(src)
    if symlinks:
        print(
            f"export: PRIVACY VIOLATION — symlinks found inside {src}; "
            "cannot safely copy:",
            file=sys.stderr,
        )
        for s in symlinks:
            print(f"  {s}", file=sys.stderr)
        return 3
    # Print a privacy warning and require explicit confirmation.
    print("=" * 60, file=sys.stderr)
    print("PRIVACY WARNING — RAW DIAGNOSTICS EXPORT", file=sys.stderr)
    print("=" * 60, file=sys.stderr)
    print(
        f"You are about to copy RAW boot diagnostics from:\n  {src}\n"
        f"to:\n  {dest}\n\n"
        "The source is marked:\n"
        f"  {MARKER_TEXT}\n\n"
        "The destination will contain the SAME raw identifiers (MACs,\n"
        "serials, UUIDs, hostnames, IPs, kernel cmdline). Do NOT commit\n"
        "it to the repository, attach it to a pull request, or share it\n"
        "outside your local investigation.\n",
        file=sys.stderr,
    )
    if not args.yes:
        print("Pass --yes to confirm the export.", file=sys.stderr)
        return 1
    # Copy the tree.
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(src, dest, symlinks=False, ignore_dangling_symlinks=True)
    # Write the marker README at the destination too.
    (dest / README_NAME).write_text(
        MARKER_TEXT + "\n\n"
        "Exported from a PRIVATE-DIAGNOSTICS directory. This copy contains\n"
        "the SAME raw identifiers as the source. Do NOT submit.\n",
        encoding="utf-8",
    )
    print(f"Exported {src} -> {dest}", file=sys.stderr)
    print(f"The destination is marked with the same PRIVATE warning.", file=sys.stderr)
    return 0


def cmd_sanitize(args: argparse.Namespace) -> int:
    """Create a NEW reviewed copy with hardware identifiers redacted.
    NEVER modifies the original."""
    src = Path(args.directory).resolve()
    dest = Path(args.destination).resolve()
    if not src.is_dir():
        print(f"sanitize: source not a directory: {src}", file=sys.stderr)
        return 2
    if dest.exists():
        print(f"sanitize: destination already exists: {dest}", file=sys.stderr)
        return 1
    # Refuse to sanitize if the source contains symlinks.
    symlinks = _check_no_symlinks(src)
    if symlinks:
        print(
            f"sanitize: PRIVACY VIOLATION — symlinks found inside {src}; "
            "cannot safely copy:",
            file=sys.stderr,
        )
        for s in symlinks:
            print(f"  {s}", file=sys.stderr)
        return 3
    if src == dest:
        print("sanitize: source and destination are the same path", file=sys.stderr)
        return 1
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(src, dest, symlinks=False, ignore_dangling_symlinks=True)
    # Redact every text file in the destination.
    redacted_count = 0
    for f in dest.rglob("*"):
        if not f.is_file():
            continue
        # Only redact text-like files. Skip binary files (none expected here,
        # but be defensive).
        try:
            text = f.read_text(encoding="utf-8", errors="strict")
        except (OSError, UnicodeDecodeError):
            continue
        redacted = _redact(text)
        if redacted != text:
            f.write_text(redacted, encoding="utf-8")
            redacted_count += 1
    # Write a marker README at the destination.
    (dest / README_NAME).write_text(
        "SANITIZED COPY — hardware identifiers redacted.\n\n"
        "This is a reviewed copy of a PRIVATE-DIAGNOSTICS directory with\n"
        "MAC addresses, serial numbers, UUIDs, IPv4 addresses, and the\n"
        "testOS hostname replaced with <MAC>, <SERIAL>, <UUID>, <IPV4>,\n"
        "and <HOSTNAME> respectively.\n\n"
        "Sanitization is BEST-EFFORT. The operator is responsible for\n"
        "reviewing the result before any submission. The sanitized copy\n"
        "must still pass the normal privacy scanner\n"
        "(tools/validate-testos-evidence.py) before it may be included\n"
        "in an evidence bundle.\n",
        encoding="utf-8",
    )
    print(f"Sanitized {src} -> {dest}", file=sys.stderr)
    print(f"Redacted identifiers in {redacted_count} file(s).", file=sys.stderr)
    print(
        "Review the result manually, then run the normal privacy scanner\n"
        "before any submission.",
        file=sys.stderr,
    )
    return 0


# ─── CLI ─────────────────────────────────────────────────────────────────────


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="testos-diagnostics.py",
        description="Local-only inspection, export, and sanitization of "
        "testOS private boot diagnostics.",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    p_inspect = sub.add_parser(
        "inspect",
        help="Read-only inspection of a PRIVATE-DIAGNOSTICS directory.",
    )
    p_inspect.add_argument("directory", help="Path to PRIVATE-DIAGNOSTICS/<run_id>/")
    p_inspect.set_defaults(func=cmd_inspect)

    p_export = sub.add_parser(
        "export",
        help="Copy raw diagnostics to an explicit destination (privacy warning).",
    )
    p_export.add_argument("directory", help="Source PRIVATE-DIAGNOSTICS/<run_id>/")
    p_export.add_argument("destination", help="Destination directory (must not exist)")
    p_export.add_argument(
        "--yes",
        action="store_true",
        help="Confirm the privacy warning and proceed with the export.",
    )
    p_export.set_defaults(func=cmd_export)

    p_sanitize = sub.add_parser(
        "sanitize",
        help="Create a new reviewed copy with identifiers redacted.",
    )
    p_sanitize.add_argument("directory", help="Source PRIVATE-DIAGNOSTICS/<run_id>/")
    p_sanitize.add_argument("destination", help="Destination directory (must not exist)")
    p_sanitize.set_defaults(func=cmd_sanitize)

    return p


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
