#!/usr/bin/env python3
"""
Validate evidence integrity (Dragnet gate).

Enforces the project's Evidence Rule mechanically: a milestone exit criterion
may be marked `verified = true` in release/milestones.toml ONLY when it carries
a `transcript = "<path>"` that points to a committed, non-empty file under
release/evidence/.

Also scans release/milestones.toml and release/evidence/**/*.md for any
`release/evidence/...` path mention that does not resolve on disk (closes the
blind spot left by the markdown-link checker, which does not inspect paths
inside code blocks, tables, or TOML strings).

evidence-validator phase (Prompt 5): also calls tools/validate-hwtest-evidence.py
for every hardware evidence bundle (any directory under release/evidence/host-bench/
that contains a hwtest-manifest.json file). The hardware validator performs
content-aware validation (manifest parses, source version/commit exist, hardware
slot valid, laptop battery present, battery/AC runs match power source, baseline/
optid paired, sample count sufficient, results parse, privacy report exists,
secrets absent, AI not evidence, event chain intact).

Exit non-zero on any violation. This is a required CI status check; it runs on
every push and PR with no path filter and cannot be skipped by editing a doc.
"""

import re
import subprocess
import sys
from pathlib import Path
import tomllib

MILESTONES = Path("release/milestones.toml")
EVIDENCE_DIR = Path("release/evidence")
HOST_BENCH_DIR = EVIDENCE_DIR / "host-bench"
HWTEST_VALIDATOR = Path("tools/validate-hwtest-evidence.py")
ROOT = Path(__file__).resolve().parent.parent

# Matches a repo-relative evidence path mention in free text / TOML / markdown.
PATH_RE = re.compile(r"release/evidence/[A-Za-z0-9._/\-]+")


def load_milestones() -> dict:
    if not MILESTONES.exists():
        print(f"ERROR: {MILESTONES} not found")
        sys.exit(1)
    with MILESTONES.open("rb") as f:
        return tomllib.load(f)


def check_verified_have_transcripts(data: dict) -> list[str]:
    """Every `verified = true` criterion needs a resolving, non-empty transcript.
    Any `transcript` field present (even on unverified rows) must also resolve."""
    errors: list[str] = []
    for ms in data.get("milestone", []):
        ver = ms.get("version", "<unknown>")
        for c in ms.get("criteria_status", []):
            crit = c.get("criterion", "<unnamed>")
            verified = c.get("verified", False)
            transcript = c.get("transcript")

            if verified and not transcript:
                errors.append(
                    f"{ver}: criterion {crit!r} is verified = true but has no "
                    f"`transcript = \"<path>\"` field"
                )
            if transcript:
                p = Path(transcript)
                if not p.exists():
                    errors.append(
                        f"{ver}: criterion {crit!r} transcript {transcript!r} "
                        f"does not exist"
                    )
                elif p.is_file() and p.stat().st_size == 0:
                    errors.append(
                        f"{ver}: criterion {crit!r} transcript {transcript!r} "
                        f"is empty"
                    )
    return errors


def check_path_mentions_resolve() -> list[str]:
    """Scan milestones.toml + evidence markdown for evidence-path mentions that
    do not resolve. Catches dangling citations (e.g. a referenced transcript
    file that was never committed)."""
    errors: list[str] = []
    files: list[Path] = []
    if MILESTONES.exists():
        files.append(MILESTONES)
    if EVIDENCE_DIR.exists():
        # Skip auto-generated Dragnet run reports: they embed validator output
        # verbatim (which may quote transient/historical paths) and are
        # observational snapshots, not curated evidence citations.
        files.extend(
            p for p in sorted(EVIDENCE_DIR.rglob("*.md"))
            if not p.name.startswith("DRAGNET-")
        )

    for f in files:
        text = f.read_text(encoding="utf-8", errors="replace")
        for m in PATH_RE.findall(text):
            mention = m.rstrip(".,;:)]}>\"'`")
            # A bare directory prefix that is clearly a glob/example is skipped.
            if mention.endswith("/"):
                continue
            if not Path(mention).exists():
                errors.append(f"{f}: references {mention!r} which does not exist")
    return errors


def check_hwtest_evidence_bundles() -> list[str]:
    """evidence-validator phase: call tools/validate-hwtest-evidence.py for every
    hardware evidence bundle under release/evidence/host-bench/ that contains a
    hwtest-manifest.json file. Returns a list of error strings (empty if no
    bundles or all pass).

    Skips the _TEMPLATE/ directory (it's a shape template, not evidence).
    """
    errors: list[str] = []
    if not HOST_BENCH_DIR.exists():
        return errors

    # Find every directory containing hwtest-manifest.json, excluding _TEMPLATE.
    bundles = []
    for p in HOST_BENCH_DIR.rglob("hwtest-manifest.json"):
        if "_TEMPLATE" in p.parts:
            continue
        bundles.append(p.parent)

    if not bundles:
        return errors

    for bundle in sorted(bundles):
        rel = bundle.relative_to(ROOT)
        try:
            r = subprocess.run(
                ["python3", str(HWTEST_VALIDATOR), "--bundle", str(bundle)],
                capture_output=True,
                text=True,
                timeout=30,
                cwd=str(ROOT),
            )
            if r.returncode != 0:
                # Collect the validator's error lines.
                for line in r.stdout.splitlines() + r.stderr.splitlines():
                    if line.strip().startswith("error:"):
                        errors.append(f"{rel}: {line.strip()}")
                if not any("error:" in l for l in r.stdout.splitlines() + r.stderr.splitlines()):
                    errors.append(f"{rel}: hardware evidence validation failed (see validator output)")
        except (OSError, subprocess.TimeoutExpired) as e:
            errors.append(f"{rel}: could not run hardware validator: {e}")

    return errors


def main() -> None:
    print("=" * 60)
    print("Rush Linux — Evidence Integrity Check (Dragnet gate)")
    print("=" * 60)

    data = load_milestones()
    errors = check_verified_have_transcripts(data)
    errors += check_path_mentions_resolve()

    # evidence-validator phase: also validate hardware evidence bundles.
    hw_errors = check_hwtest_evidence_bundles()
    if hw_errors:
        print("\n── Check: Hardware evidence bundles (content-aware) ──")
        for e in hw_errors:
            print(f"  ✗ {e}")
    errors.extend(hw_errors)

    # De-duplicate while preserving order.
    seen = set()
    unique = [e for e in errors if not (e in seen or seen.add(e))]

    print("\n" + "-" * 60)
    if unique:
        print(f"FAILED: {len(unique)} evidence-integrity violation(s)")
        for e in unique:
            print(f"  ✗ {e}")
        print("-" * 60)
        print(
            "\nEvery `verified = true` criterion must cite a committed transcript "
            "via `transcript = \"<path>\"`. Produce the transcript, or set the "
            "criterion back to `verified = false` until it exists."
        )
        sys.exit(1)
    else:
        print("PASSED: all verified criteria carry resolving transcripts; "
              "no dangling evidence citations")
        print("-" * 60)
        sys.exit(0)


if __name__ == "__main__":
    main()
