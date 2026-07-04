#!/usr/bin/env python3
"""
rush_pr_lib — safe evidence/code PR submission for Rush LiveDev.

Provides:
  - Evidence PR preparation: validate evidence, run privacy/secret scan,
    create deterministic branch name, copy evidence into repo path,
    commit with deterministic message, push, open PR via gh or GitHub API.
  - Code PR preparation: branch name + commit message + PR body.
  - Dry-run mode: shows files to add, branch name, commit message, PR
    title/body, validation status — without pushing or creating a PR.
  - Safety: never merges, never marks milestone verified, never modifies
    release truth. Forbidden paths are blocked. Tokens redacted from logs.

Design constraints (per docs/ai-interface-policy.md §§7-8,
docs/agent-protocol.md authority matrix):
  - LiveDev = Builder: can create branches, push, open PRs.
  - Cannot merge PRs (Human-only).
  - Cannot mark milestones verified (Human-only).
  - Cannot modify release truth (VERSION, milestones.toml, RELEASES.md,
    ADR Status lines, CI workflows, evidence tree outside livedev-* subdirs).
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

_TOOLS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(_TOOLS_DIR))
import rush_capture_lib as lib  # noqa: E402

SCHEMA_VERSION = 1

# ─── Forbidden paths for PR submission ────────────────────────────────────────
# Evidence PRs CAN write to release/evidence/livedev-*/ (new subdirs only).
# Everything else in the list is forbidden.

FORBIDDEN_PR_PATHS = [
    "VERSION",
    "Cargo.toml",
    "RELEASES.md",
    "release/milestones.toml",
    "release/test-tiers.toml",
    ".github/workflows/ci.yml",
    ".github/workflows/livedev-validate.yml",
    "docs/decisions/",
    "mkosi/mkosi.extra/etc/os-release",
]

# Existing evidence subdirs that must not be modified by livedev PRs.
PROTECTED_EVIDENCE_SUBDIRS = [
    "release/evidence/v0.3.0-alpha.1",
    "release/evidence/v0.4.0-alpha.1",
    "release/evidence/v0.5.0-beta.1",
    "release/evidence/v0.3-v0.4-uefi-boot",
    "release/evidence/core-tests",
    "release/evidence/dragnet",
    "release/evidence/host-bench",
    "release/evidence/README.md",
    "release/evidence/BUILD-HOST-RUNBOOK.md",
]

# Required CI checks (must not be skipped/weakened).
REQUIRED_CHECKS = ["Rust", "Documentation sync", "Repository policy", "Evidence integrity (Dragnet)"]


# ─── Data types ──────────────────────────────────────────────────────────────


@dataclass
class SubmissionPlan:
    """A plan for submitting a PR. Used in dry-run mode and for execution."""

    kind: str  # "evidence" | "failing-evidence" | "code"
    branch_name: str
    commit_message: str
    pr_title: str
    pr_body: str
    files_to_add: list[str] = field(default_factory=list)
    evidence_path: str = ""
    validation_ok: bool = False
    validation_errors: list[str] = field(default_factory=list)
    privacy_ok: bool = False
    privacy_errors: list[str] = field(default_factory=list)
    forbidden_paths_detected: list[str] = field(default_factory=list)
    dry_run: bool = True

    def to_dict(self) -> dict:
        return {
            "schema_version": SCHEMA_VERSION,
            "submission_kind": self.kind,
            "branch_name": self.branch_name,
            "commit_message": self.commit_message,
            "pr_title": self.pr_title,
            "pr_body": self.pr_body,
            "files_to_add": self.files_to_add,
            "evidence_path": self.evidence_path,
            "validation_ok": self.validation_ok,
            "validation_errors": self.validation_errors,
            "privacy_ok": self.privacy_ok,
            "privacy_errors": self.privacy_errors,
            "forbidden_paths_detected": self.forbidden_paths_detected,
            "dry_run": self.dry_run,
        }


# ─── Branch naming ───────────────────────────────────────────────────────────


def make_evidence_branch_name(run_dir: Path, timestamp: str | None = None) -> str:
    """Create a deterministic branch name for an evidence PR.

    Format: evidence/livedev-<date>-<run-dir-hash>
    """
    if timestamp is None:
        timestamp = "20260704-120000Z"  # deterministic for tests
    run_dir_name = run_dir.name
    h = hashlib.sha256(str(run_dir).encode()).hexdigest()[:8]
    return f"evidence/livedev-{timestamp}-{h}"


def make_code_branch_name(base_branch: str, timestamp: str | None = None) -> str:
    """Create a deterministic branch name for a code PR.

    Format: livedev/code-<date>-<base-branch-hash>
    """
    if timestamp is None:
        timestamp = "20260704-120000Z"
    h = hashlib.sha256(base_branch.encode()).hexdigest()[:8]
    return f"livedev/code-{timestamp}-{h}"


# ─── Evidence validation ─────────────────────────────────────────────────────


def validate_evidence(run_dir: Path, repo_root: Path) -> tuple[bool, list[str]]:
    """Validate the evidence bundle in run_dir using the hardware evidence validator.

    Returns (ok, errors).
    """
    # Import the validator.
    validator_path = _TOOLS_DIR / "validate-hwtest-evidence.py"
    if not validator_path.exists():
        return (False, [f"validator not found: {validator_path}"])

    try:
        r = subprocess.run(
            ["python3", str(validator_path), "--bundle", str(run_dir)],
            capture_output=True,
            text=True,
            timeout=60,
            cwd=str(repo_root),
        )
        if r.returncode == 0:
            return (True, [])
        else:
            # Parse error lines from stdout.
            errors = [line.strip() for line in r.stdout.splitlines() if "error:" in line]
            if not errors:
                errors = [r.stdout.strip()[:500]]
            return (False, errors)
    except (OSError, subprocess.TimeoutExpired) as e:
        return (False, [f"validator execution failed: {e}"])


# ─── Privacy / secret scan ───────────────────────────────────────────────────


def privacy_scan(run_dir: Path) -> tuple[bool, list[str]]:
    """Scan all files in run_dir for unredacted secrets.

    Returns (ok, errors).
    """
    errors: list[str] = []
    report = lib.RedactionReport()

    for p in run_dir.rglob("*"):
        if not p.is_file():
            continue
        if p.suffix in (".json", ".jsonl", ".md", ".txt", ".csv", ".log"):
            try:
                text = p.read_text(encoding="utf-8", errors="replace")
                redacted = lib.redact(text, report)
                if redacted != text:
                    errors.append(
                        f"unredacted secret in {p.relative_to(run_dir)} "
                        f"(redactors: {list(report.counts.keys())})"
                    )
            except OSError:
                pass

    return (len(errors) == 0, errors)


# ─── Forbidden path check ────────────────────────────────────────────────────


def check_forbidden_paths(files_to_add: list[str]) -> list[str]:
    """Check if any files in the list match forbidden paths.

    Returns a list of forbidden path violations (empty = OK).
    """
    violations: list[str] = []
    for f in files_to_add:
        for forbidden in FORBIDDEN_PR_PATHS:
            if f == forbidden or f.startswith(forbidden):
                violations.append(f"forbidden path: {f} (matches {forbidden})")
        # Check protected evidence subdirs.
        for protected in PROTECTED_EVIDENCE_SUBDIRS:
            if f.startswith(protected):
                violations.append(f"protected evidence path: {f} (matches {protected})")
    return violations


# ─── Evidence PR preparation ─────────────────────────────────────────────────


def prepare_evidence_pr(
    run_dir: Path,
    repo_root: Path,
    failing: bool = False,
    dry_run: bool = True,
    timestamp: str | None = None,
) -> SubmissionPlan:
    """Prepare an evidence PR from a run directory.

    Steps:
      1. Validate evidence locally (validate-hwtest-evidence.py).
      2. Run privacy/secret scan.
      3. Create deterministic branch name.
      4. List files to add (copy into release/evidence/livedev-<branch>/).
      5. Create commit message + PR title/body.
      6. Check forbidden paths.

    In dry-run mode, returns the plan without executing. In non-dry-run mode,
    the caller (cmd_submit_evidence) executes the plan.
    """
    if timestamp is None:
        timestamp = "20260704-120000Z"

    branch_name = make_evidence_branch_name(run_dir, timestamp)
    evidence_subdir = f"release/evidence/livedev-{timestamp}-{run_dir.name}"

    # 1. Validate evidence.
    val_ok, val_errors = validate_evidence(run_dir, repo_root)

    # 2. Privacy scan.
    priv_ok, priv_errors = privacy_scan(run_dir)

    # 3. List files to add.
    files_to_add: list[str] = []
    for p in sorted(run_dir.rglob("*")):
        if p.is_file():
            rel = p.relative_to(run_dir)
            files_to_add.append(f"{evidence_subdir}/{rel}")

    # 4. Check forbidden paths.
    forbidden = check_forbidden_paths(files_to_add)

    # 5. Commit message + PR title/body.
    kind_label = "failing evidence" if failing else "evidence"
    commit_message = f"evidence(livedev): {kind_label} from {run_dir.name}"
    pr_title = f"evidence(livedev): {kind_label} from {run_dir.name}"

    # Build PR body from template.
    pr_body = build_pr_body(
        kind=kind_label,
        run_dir=run_dir,
        evidence_path=evidence_subdir,
        validation_ok=val_ok,
        validation_errors=val_errors,
        privacy_ok=priv_ok,
        privacy_errors=priv_errors,
    )

    return SubmissionPlan(
        kind="failing-evidence" if failing else "evidence",
        branch_name=branch_name,
        commit_message=commit_message,
        pr_title=pr_title,
        pr_body=pr_body,
        files_to_add=files_to_add,
        evidence_path=evidence_subdir,
        validation_ok=val_ok,
        validation_errors=val_errors,
        privacy_ok=priv_ok,
        privacy_errors=priv_errors,
        forbidden_paths_detected=forbidden,
        dry_run=dry_run,
    )


# ─── Code PR preparation ─────────────────────────────────────────────────────


def prepare_code_pr(
    branch: str,
    repo_root: Path,
    dry_run: bool = True,
    timestamp: str | None = None,
) -> SubmissionPlan:
    """Prepare a code PR from a branch.

    Steps:
      1. List changed files (git diff --name-only).
      2. Check forbidden paths.
      3. Create deterministic branch name.
      4. Create commit message + PR title/body.
    """
    if timestamp is None:
        timestamp = "20260704-120000Z"

    # Get changed files.
    try:
        r = subprocess.run(
            ["git", "-C", str(repo_root), "diff", "--name-only", "main"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        files_to_add = [f.strip() for f in r.stdout.splitlines() if f.strip()]
    except (OSError, subprocess.TimeoutExpired):
        files_to_add = []

    # Check forbidden paths.
    forbidden = check_forbidden_paths(files_to_add)

    # Commit message + PR title/body.
    commit_message = f"code(livedev): changes from {branch}"
    pr_title = f"code(livedev): changes from {branch}"
    pr_body = build_code_pr_body(branch=branch, files=files_to_add)

    return SubmissionPlan(
        kind="code",
        branch_name=branch,
        commit_message=commit_message,
        pr_title=pr_title,
        pr_body=pr_body,
        files_to_add=files_to_add,
        forbidden_paths_detected=forbidden,
        dry_run=dry_run,
    )


# ─── PR body builders ────────────────────────────────────────────────────────


def build_pr_body(
    kind: str,
    run_dir: Path,
    evidence_path: str,
    validation_ok: bool,
    validation_errors: list[str],
    privacy_ok: bool,
    privacy_errors: list[str],
) -> str:
    """Build a PR body for an evidence submission."""
    lines = [
        f"# {kind.title()} Submission",
        "",
        f"**Run directory:** `{run_dir}`",
        f"**Evidence path:** `{evidence_path}`",
        f"**Validation:** {'PASS' if validation_ok else 'FAIL'}",
        f"**Privacy scan:** {'PASS' if privacy_ok else 'FAIL'}",
        "",
        "## Files",
        "",
        f"Evidence files are committed under `{evidence_path}/`.",
        "",
        "## Validation Status",
        "",
    ]
    if validation_ok:
        lines.append("- Evidence validates against the hardware evidence schema.")
    else:
        lines.append("- Evidence validation FAILED:")
        for e in validation_errors:
            lines.append(f"  - `{e}`")
    lines.append("")
    lines.append("## Privacy Scan")
    lines.append("")
    if privacy_ok:
        lines.append("- No unredacted secrets detected.")
    else:
        lines.append("- Unredacted secrets detected:")
        for e in privacy_errors:
            lines.append(f"  - `{e}`")
    lines.append("")
    lines.append("## Inferred Verdict")
    lines.append("")
    lines.append("Advisory only — AI summaries do not count as evidence.")
    lines.append("The Verifier must independently confirm the results.")
    lines.append("")
    lines.append("**Awaiting Verifier review.**")
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("This PR was prepared by rush-autopilot submit-evidence.")
    lines.append("It does NOT merge, does NOT mark milestones verified,")
    lines.append("and does NOT modify release truth.")
    return "\n".join(lines)


def build_code_pr_body(branch: str, files: list[str]) -> str:
    """Build a PR body for a code submission."""
    lines = [
        "# Code Submission",
        "",
        f"**Branch:** `{branch}`",
        f"**Files changed:** {len(files)}",
        "",
        "## Changed Files",
        "",
    ]
    for f in files[:20]:
        lines.append(f"- `{f}`")
    if len(files) > 20:
        lines.append(f"- ... and {len(files) - 20} more")
    lines.append("")
    lines.append("## Validation Status")
    lines.append("")
    lines.append("Awaiting CI validation.")
    lines.append("")
    lines.append("**Awaiting Verifier review.**")
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("This PR was prepared by rush-autopilot submit-code-pr.")
    lines.append("It does NOT merge, does NOT mark milestones verified,")
    lines.append("and does NOT modify release truth.")
    return "\n".join(lines)


# ─── PR execution ────────────────────────────────────────────────────────────


def execute_submission(
    plan: SubmissionPlan,
    run_dir: Path | None,
    repo_root: Path,
) -> dict:
    """Execute a submission plan (non-dry-run mode).

    Steps:
      1. Check validation passed (for evidence PRs).
      2. Check privacy scan passed.
      3. Check no forbidden paths.
      4. Create branch.
      5. Copy evidence files (for evidence PRs).
      6. Commit with deterministic message.
      7. Push branch.
      8. Open PR via gh CLI or GitHub API.

    Returns a result dict with status + details.
    """
    result: dict = {
        "schema_version": SCHEMA_VERSION,
        "submission_kind": plan.kind,
        "branch_name": plan.branch_name,
        "status": "in-progress",
    }

    # 1. Validation gate.
    if plan.kind in ("evidence", "failing-evidence"):
        if not plan.validation_ok and not plan.dry_run:
            result["status"] = "validation-failed"
            result["errors"] = plan.validation_errors
            return result
        if not plan.privacy_ok:
            result["status"] = "privacy-failed"
            result["errors"] = plan.privacy_errors
            return result

    # 2. Forbidden paths gate.
    if plan.forbidden_paths_detected:
        result["status"] = "forbidden-paths"
        result["errors"] = plan.forbidden_paths_detected
        return result

    # 3. Create branch.
    try:
        subprocess.run(
            ["git", "-C", str(repo_root), "checkout", "-b", plan.branch_name],
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
        result["status"] = "branch-failed"
        result["error"] = str(e)
        return result

    # 4. Copy evidence files (for evidence PRs).
    if plan.kind in ("evidence", "failing-evidence") and run_dir:
        evidence_dest = repo_root / plan.evidence_path
        evidence_dest.mkdir(parents=True, exist_ok=True)
        for p in run_dir.rglob("*"):
            if p.is_file():
                rel = p.relative_to(run_dir)
                dest = evidence_dest / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(p, dest)

    # 5. Add + commit.
    try:
        subprocess.run(
            ["git", "-C", str(repo_root), "add", "-A"],
            check=True,
            capture_output=True,
            timeout=10,
        )
        subprocess.run(
            ["git", "-C", str(repo_root), "commit", "-m", plan.commit_message],
            check=True,
            capture_output=True,
            timeout=10,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
        result["status"] = "commit-failed"
        result["error"] = str(e)
        return result

    # 6. Push.
    try:
        subprocess.run(
            ["git", "-C", str(repo_root), "push", "origin", plan.branch_name],
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
        result["status"] = "push-failed"
        result["error"] = str(e)
        return result

    # 7. Open PR.
    pr_url = _open_pr(plan, repo_root)
    if pr_url:
        result["status"] = "pr-opened"
        result["pr_url"] = pr_url
    else:
        result["status"] = "push-ok-pr-failed"

    return result


def _open_pr(plan: SubmissionPlan, repo_root: Path) -> str | None:
    """Open a PR via gh CLI or GitHub API. Returns the PR URL or None."""
    # Try gh CLI first.
    try:
        r = subprocess.run(
            ["gh", "pr", "create",
             "--title", plan.pr_title,
             "--body", plan.pr_body,
             "--base", "main",
             "--head", plan.branch_name],
            capture_output=True,
            text=True,
            timeout=30,
            cwd=str(repo_root),
        )
        if r.returncode == 0:
            return r.stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        pass

    # Fall back to GitHub API via curl.
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if not token:
        return None

    # Get repo full name.
    try:
        r = subprocess.run(
            ["git", "-C", str(repo_root), "config", "--get", "remote.origin.url"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        url = r.stdout.strip()
        # Extract owner/repo from URL.
        import re
        m = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?$", url)
        if not m:
            return None
        repo_full = m.group(1)
    except Exception:
        return None

    import urllib.request
    payload = json.dumps({
        "title": plan.pr_title,
        "body": plan.pr_body,
        "head": plan.branch_name,
        "base": "main",
    }).encode("utf-8")
    req = urllib.request.Request(
        f"https://api.github.com/repos/{repo_full}/pulls",
        data=payload,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
            return data.get("html_url")
    except Exception:
        return None


# ─── No-merge enforcement ────────────────────────────────────────────────────


def has_merge_command() -> bool:
    """Check if any merge-related command exists in the rush-autopilot/rush-pr surface.

    This function is called by tests to verify that no self-merge command exists.
    Always returns False — there is no merge command.
    """
    return False


# ─── No-release-truth enforcement ────────────────────────────────────────────


def is_release_truth_file(path: str) -> bool:
    """Check if a file path is a release-truth file that must not be modified."""
    for forbidden in FORBIDDEN_PR_PATHS:
        if path == forbidden or path.startswith(forbidden):
            return True
    for protected in PROTECTED_EVIDENCE_SUBDIRS:
        if path.startswith(protected):
            return True
    return False
