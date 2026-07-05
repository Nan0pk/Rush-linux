#!/usr/bin/env python3
"""
pytest tests for tools/render-frontpage.py and tools/check-docs-impact.py.

Covers:
  - README generator deterministic output (same repo state → same output)
  - --check fails when generated section is stale
  - --write updates README
  - docs-impact detects user-facing change without docs
  - docs-impact passes when docs/frontpage updated
  - docs-impact bypass works with --allow-docs-not-needed
  - PR #254-like change is caught if front page/docs missing

Run with:
  python3 -m pytest tools/test-frontpage-sync.py -v
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _load_module(name: str, path: Path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    loader.exec_module(mod)
    return mod


rfp = _load_module("render_frontpage", _TOOLS_DIR / "render-frontpage.py")
cdi = _load_module("check_docs_impact", _TOOLS_DIR / "check-docs-impact.py")


# ─── Generator determinism ──────────────────────────────────────────────────


def test_generator_deterministic():
    """render_section() returns the same output on repeated calls."""
    a = rfp.render_section()
    b = rfp.render_section()
    assert a == b, "render_section() is not deterministic"


def test_generator_has_expected_sections():
    """The generated section includes the expected subsections."""
    out = rfp.render_section()
    assert "### Editions" in out
    assert "### CI workflows" in out
    assert "### Systemd services" in out
    assert "### Documentation" in out
    assert "### Tests & validation" in out


def test_generator_lists_all_editions():
    """All mkosi profiles appear in the generated editions table."""
    out = rfp.render_section()
    for edition in ("desktop", "livedev", "server", "testos"):
        assert edition in out, f"edition {edition!r} missing from generated output"


# ─── --check and --write ────────────────────────────────────────────────────


def test_check_passes_when_in_sync():
    """--check exits 0 when README is in sync with the generator."""
    # Sync first.
    subprocess.run(
        ["python3", str(_TOOLS_DIR / "render-frontpage.py"), "--write"],
        check=True, capture_output=True, cwd=str(_ROOT),
    )
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "render-frontpage.py"), "--check"],
        capture_output=True, text=True, cwd=str(_ROOT),
    )
    assert r.returncode == 0, f"--check failed:\n{r.stdout}\n{r.stderr}"


def test_check_fails_when_stale():
    """--check exits 1 when the generated section is stale."""
    # Read the current README, corrupt the generated section, write back.
    readme_path = _ROOT / "README.md"
    original = readme_path.read_text(encoding="utf-8")
    corrupted = original.replace(
        rfp.START_MARKER,
        rfp.START_MARKER + "\nSTALE CONTENT THAT IS NOT GENERATED\n",
    )
    try:
        readme_path.write_text(corrupted, encoding="utf-8")
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "render-frontpage.py"), "--check"],
            capture_output=True, text=True, cwd=str(_ROOT),
        )
        assert r.returncode == 1, "--check should fail when stale"
        assert "stale" in r.stderr.lower() or "stale" in r.stdout.lower()
    finally:
        # Restore.
        readme_path.write_text(original, encoding="utf-8")


def test_write_updates_readme():
    """--write updates README.md so that --check passes."""
    # Make it stale, then --write, then --check.
    readme_path = _ROOT / "README.md"
    original = readme_path.read_text(encoding="utf-8")
    corrupted = original.replace(
        rfp.START_MARKER,
        rfp.START_MARKER + "\nSTALE\n",
    )
    try:
        readme_path.write_text(corrupted, encoding="utf-8")
        subprocess.run(
            ["python3", str(_TOOLS_DIR / "render-frontpage.py"), "--write"],
            check=True, capture_output=True, cwd=str(_ROOT),
        )
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "render-frontpage.py"), "--check"],
            capture_output=True, text=True, cwd=str(_ROOT),
        )
        assert r.returncode == 0
    finally:
        readme_path.write_text(original, encoding="utf-8")


# ─── Docs impact: path matching ─────────────────────────────────────────────


def test_user_facing_pattern_matches_tools():
    """tools/foo and tools/sub/bar match the user-facing patterns."""
    assert cdi._match_any("tools/livedev-next", cdi.USER_FACING_PATTERNS)
    assert cdi._match_any("tools/rush-livedev-runner", cdi.USER_FACING_PATTERNS)
    assert cdi._match_any("tools/sub/dir/file.py", cdi.USER_FACING_PATTERNS)


def test_user_facing_pattern_matches_systemd():
    """packaging/systemd/*.service matches."""
    assert cdi._match_any("packaging/systemd/rush-livedev-test.service",
                          cdi.USER_FACING_PATTERNS)


def test_user_facing_pattern_matches_mkosi():
    """mkosi/** matches."""
    assert cdi._match_any("mkosi/mkosi.profiles/livedev/mkosi.conf",
                          cdi.USER_FACING_PATTERNS)


def test_user_facing_pattern_matches_workflows():
    """.github/workflows/** matches."""
    assert cdi._match_any(".github/workflows/frontpage-sync.yml",
                          cdi.USER_FACING_PATTERNS)


def test_user_facing_pattern_does_not_match_internal():
    """Internal-only paths (e.g. .gitignore, LICENSE) are not user-facing."""
    assert not cdi._match_any(".gitignore", cdi.USER_FACING_PATTERNS)
    assert not cdi._match_any("LICENSE", cdi.USER_FACING_PATTERNS)
    assert not cdi._match_any("Cargo.lock", cdi.USER_FACING_PATTERNS)


def test_docs_satisfying_pattern_matches_readme():
    """README.md satisfies the docs requirement."""
    assert cdi._match_any("README.md", cdi.DOCS_SATISFYING_PATTERNS)


def test_docs_satisfying_pattern_matches_docs():
    """docs/** satisfies the docs requirement."""
    assert cdi._match_any("docs/editions/livedev.md", cdi.DOCS_SATISFYING_PATTERNS)
    assert cdi._match_any("docs/frontpage/project.yml", cdi.DOCS_SATISFYING_PATTERNS)


# ─── Docs impact: check() logic ─────────────────────────────────────────────


def test_check_no_changes_passes():
    """No file changes → pass."""
    code, msg = cdi.check(base="HEAD", head="HEAD")
    assert code == 0


def test_check_user_facing_without_docs_fails():
    """User-facing change with no docs → fail (exit 1)."""
    # Simulate: patch _changed_files to return a tools/ file only.
    orig = cdi._changed_files
    cdi._changed_files = lambda base, head: ["tools/some-tool"]
    try:
        code, msg = cdi.check(base="HEAD", head="HEAD")
        assert code == 1
        assert "user-facing" in msg.lower()
    finally:
        cdi._changed_files = orig


def test_check_user_facing_with_docs_passes():
    """User-facing change + docs change → pass."""
    orig = cdi._changed_files
    cdi._changed_files = lambda base, head: [
        "tools/some-tool", "docs/editions/livedev.md",
    ]
    try:
        code, msg = cdi.check(base="HEAD", head="HEAD")
        assert code == 0
    finally:
        cdi._changed_files = orig


def test_check_user_facing_with_readme_passes():
    """User-facing change + README change → pass."""
    orig = cdi._changed_files
    cdi._changed_files = lambda base, head: [
        "tools/some-tool", "README.md",
    ]
    try:
        code, msg = cdi.check(base="HEAD", head="HEAD")
        assert code == 0
    finally:
        cdi._changed_files = orig


def test_check_bypass_passes():
    """--allow-docs-not-needed bypasses the check."""
    orig = cdi._changed_files
    cdi._changed_files = lambda base, head: ["tools/some-tool"]
    try:
        code, msg = cdi.check(base="HEAD", head="HEAD", allow_bypass=True)
        assert code == 0
        assert "bypass" in msg.lower()
    finally:
        cdi._changed_files = orig


def test_check_non_user_facing_passes():
    """Non-user-facing change (e.g. LICENSE) → pass."""
    orig = cdi._changed_files
    cdi._changed_files = lambda base, head: ["LICENSE"]
    try:
        code, msg = cdi.check(base="HEAD", head="HEAD")
        assert code == 0
    finally:
        cdi._changed_files = orig


# ─── PR #254-like scenario ──────────────────────────────────────────────────


def test_pr254_like_change_caught_without_docs():
    """A PR #254-like change (tools/ + packaging/systemd/ + mkosi/ but no docs)
    is caught as a failure."""
    orig = cdi._changed_files
    cdi._changed_files = lambda base, head: [
        "tools/rush-livedev-runner",
        "tools/rush_livedev_state.py",
        "packaging/systemd/rush-livedev-test.service",
        "mkosi/mkosi.profiles/livedev/mkosi.conf",
        ".github/workflows/livedev-validate.yml",
    ]
    try:
        code, msg = cdi.check(base="HEAD", head="HEAD")
        assert code == 1
        assert "user-facing" in msg.lower()
    finally:
        cdi._changed_files = orig


def test_pr254_like_change_passes_with_docs():
    """Same change + README/docs update → pass."""
    orig = cdi._changed_files
    cdi._changed_files = lambda base, head: [
        "tools/rush-livedev-runner",
        "packaging/systemd/rush-livedev-test.service",
        "README.md",
        "docs/editions/livedev.md",
    ]
    try:
        code, msg = cdi.check(base="HEAD", head="HEAD")
        assert code == 0
    finally:
        cdi._changed_files = orig


# ─── Standalone runner ──────────────────────────────────────────────────────


def _run_all_tests() -> int:
    test_funcs = [
        (name, obj)
        for name, obj in sorted(globals().items())
        if name.startswith("test_") and callable(obj)
    ]
    passed = 0
    failed = 0
    for name, func in test_funcs:
        try:
            func()
            print(f"  PASS {name}")
            passed += 1
        except Exception as e:
            print(f"  FAIL {name}: {e}")
            import traceback
            traceback.print_exc()
            failed += 1
    print(f"\n{passed} passed, {failed} failed, {passed + failed} total")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(_run_all_tests())
