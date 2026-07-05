#!/usr/bin/env python3
"""
rush-livedev-submit — automated result submission for Rush LiveDev.

Submission modes (selected via --submit or RUSH_LIVEDEV_SUBMIT env var):

  none    — no submission at all. Local artifacts + summary still produced.
  local   — produce bundle, print exact artifact path + pass/fail summary.
  github  — write Markdown summary to $GITHUB_STEP_SUMMARY (if running in
            GH Actions). Optionally post one bot comment on a PR (no spam).
  http    — POST summary.json + bundle to $RUSH_RESULTS_ENDPOINT.
  auto    — pick the best available: GH Actions if detected, HTTP if
            endpoint configured, otherwise local.

Normal local livedev testing does NOT require network credentials.
GitHub and HTTP modes fail gracefully (record the error in summary.json,
do not crash the orchestrator).
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))


def submit(mode: str, artifacts_dir: Path, *, run_id: str,
           summary: dict[str, Any], console_log: Path,
           repo_root: Path) -> tuple[str, str]:
    """Dispatch submission to the right backend.

    Returns (status, error_message). status is one of:
        none, local, github, http, auto, skipped, error
    error_message is "" on success.
    """
    # Resolve env override if mode is "auto" or empty.
    if not mode:
        mode = os.environ.get("RUSH_LIVEDEV_SUBMIT", "local")

    if mode == "none":
        return ("none", "")
    if mode == "local":
        return _submit_local(artifacts_dir, run_id, summary)
    if mode == "github":
        return _submit_github(artifacts_dir, run_id, summary, repo_root)
    if mode == "http":
        return _submit_http(artifacts_dir, run_id, summary, console_log)
    if mode == "auto":
        # Pick best available.
        if _in_github_actions():
            return _submit_github(artifacts_dir, run_id, summary, repo_root)
        if os.environ.get("RUSH_RESULTS_ENDPOINT"):
            return _submit_http(artifacts_dir, run_id, summary, console_log)
        return _submit_local(artifacts_dir, run_id, summary)
    return ("error", f"unknown submit mode: {mode!r}")


# ─── Local ───────────────────────────────────────────────────────────────────


def _submit_local(artifacts_dir: Path, run_id: str,
                  summary: dict[str, Any]) -> tuple[str, str]:
    """Print the artifact path and a pass/fail summary."""
    bundle = _find_bundle(artifacts_dir, run_id)
    status = summary.get("status", "unknown")
    exit_code = summary.get("exit_code", "?")
    print(flush=True)
    print("[submit-local] Rush LiveDev run complete.", flush=True)
    print(f"  run_id:        {run_id}", flush=True)
    print(f"  status:        {status}", flush=True)
    print(f"  exit_code:     {exit_code}", flush=True)
    print(f"  artifacts_dir: {artifacts_dir}", flush=True)
    if bundle:
        print(f"  bundle:        {bundle}", flush=True)
        print(f"  bundle size:   {_human_size(Path(bundle).stat().st_size)}",
              flush=True)
    else:
        print("  bundle:        (none — zstd and gzip both unavailable)",
              flush=True)
    print(f"  summary.json:  {artifacts_dir / 'summary.json'}", flush=True)
    return ("local", "")


# ─── GitHub ──────────────────────────────────────────────────────────────────


def _in_github_actions() -> bool:
    return os.environ.get("GITHUB_ACTIONS") == "true"


def _submit_github(artifacts_dir: Path, run_id: str,
                   summary: dict[str, Any],
                   repo_root: Path) -> tuple[str, str]:
    """Write Markdown summary to $GITHUB_STEP_SUMMARY (if set).

    Optionally post one bot comment on a PR (no spam). The PR comment uses
    the GitHub API via `gh` CLI; if `gh` is unavailable or no PR context
    exists, the step summary alone is sufficient.
    """
    if not _in_github_actions():
        # Not in GH Actions — fall back to local.
        return _submit_local(artifacts_dir, run_id, summary)

    step_summary_path = os.environ.get("GITHUB_STEP_SUMMARY", "")
    md = _render_github_markdown(summary, artifacts_dir, run_id)

    if step_summary_path:
        try:
            # Append (not overwrite) so multiple jobs can contribute.
            with open(step_summary_path, "a", encoding="utf-8") as f:
                f.write(md + "\n")
        except OSError as e:
            return ("error", f"cannot write GITHUB_STEP_SUMMARY: {e}")

    # Expose JUnit XML if the test framework produced one.
    junit = _find_junit(artifacts_dir)
    if junit:
        print(f"::set-output name=junit-path::{junit}", flush=True)

    # Optionally post a PR comment (only if there is a PR context).
    pr_url = _post_pr_comment(md, repo_root)
    if pr_url:
        print(f"[submit-github] PR comment: {pr_url}", flush=True)

    return ("github", "")


def _render_github_markdown(summary: dict[str, Any],
                            artifacts_dir: Path, run_id: str) -> str:
    status = summary.get("status", "unknown")
    exit_code = summary.get("exit_code", "?")
    duration = summary.get("duration_sec", 0)
    markers = summary.get("markers_seen", [])
    failure = summary.get("failure_reason", "")
    suite = summary.get("suite", "")
    test_cmd = summary.get("test_command", "")
    image = summary.get("image", "")
    host = summary.get("host", {}) or {}
    git_info = summary.get("config", {}).get("git", {}) or {}
    if not git_info and "git" in summary:
        git_info = summary.get("git", {}) or {}

    emoji = {
        "passed": "✅",
        "failed": "❌",
        "timeout": "⏱️",
        "infra_error": "🔧",
        "guest_failure": "⚠️",
        "unknown": "❓",
    }.get(status, "❓")

    lines = [
        f"## {emoji} Rush LiveDev — `{run_id}`",
        "",
        f"| field | value |",
        f"|---|---|",
        f"| status | **{status}** |",
        f"| exit code | `{exit_code}` |",
        f"| suite | `{suite}` |",
        f"| duration | `{duration:.1f}s` |",
        f"| image | `{image}` |",
        f"| host kernel | `{host.get('kernel', '?')}` |",
        f"| qemu version | `{host.get('qemu_version', '?')}` |",
        f"| git commit | `{git_info.get('commit', '?')[:12]}` |",
        f"| git branch | `{git_info.get('branch', '?')}` |",
        f"| git dirty | `{git_info.get('dirty', '?')}` |",
        "",
    ]
    if test_cmd:
        lines += [
            "### Test command",
            "",
            "```sh",
            test_cmd,
            "```",
            "",
        ]
    if markers:
        lines += [
            "### Markers seen",
            "",
            "```",
            *markers,
            "```",
            "",
        ]
    if failure:
        lines += [
            "### Failure reason",
            "",
            "```",
            failure,
            "```",
            "",
        ]
    lines += [
        "### Artifacts",
        "",
        f"- summary: `{artifacts_dir / 'summary.json'}`",
        f"- console: `{artifacts_dir / 'console.log'}`",
        f"- metadata: `{artifacts_dir / 'metadata.json'}`",
    ]
    bundle = _find_bundle(artifacts_dir, run_id)
    if bundle:
        lines.append(f"- bundle: `{bundle}`")
    return "\n".join(lines)


def _post_pr_comment(md: str, repo_root: Path) -> str | None:
    """Post a single bot comment on the current PR (no spam).

    Strategy: search for existing comments by the bot user; if one exists,
    update it (PATCH). Otherwise create a new one (POST). If `gh` is not
    available or there is no PR context, return None.
    """
    gh = shutil.which("gh")
    if not gh:
        return None
    if not os.environ.get("GITHUB_TOKEN") and not os.environ.get("GH_TOKEN"):
        return None
    # Find the current PR.
    try:
        r = subprocess.run(
            [gh, "pr", "view", "--json", "number,url", "--jq", ".number,.url"],
            capture_output=True, text=True, timeout=15, cwd=str(repo_root),
        )
    except (subprocess.SubprocessError, FileNotFoundError):
        return None
    if r.returncode != 0 or not r.stdout.strip():
        return None
    parts = r.stdout.strip().split(",")
    if len(parts) < 2:
        return None
    try:
        pr_number = int(parts[0])
    except ValueError:
        return None
    pr_url = parts[1].strip().strip('"')

    # Look for an existing comment with our signature.
    signature = "<!-- rush-livedev-bot -->"
    body = signature + "\n\n" + md
    try:
        list_r = subprocess.run(
            [gh, "api", f"repos/:owner/:repo/issues/{pr_number}/comments",
             "--jq", ".[] | select(.body | contains(\"" + signature + "\")) | .id"],
            capture_output=True, text=True, timeout=15, cwd=str(repo_root),
        )
    except subprocess.SubprocessError:
        return None
    existing_id = ""
    if list_r.returncode == 0:
        ids = [l.strip() for l in list_r.stdout.splitlines() if l.strip()]
        if ids:
            existing_id = ids[0]

    try:
        if existing_id:
            subprocess.run(
                [gh, "api", "-X", "PATCH",
                 f"repos/:owner/:repo/issues/comments/{existing_id}",
                 "-f", f"body={body}"],
                capture_output=True, text=True, timeout=15, cwd=str(repo_root),
                check=False,
            )
        else:
            subprocess.run(
                [gh, "api", "-X", "POST",
                 f"repos/:owner/:repo/issues/{pr_number}/comments",
                 "-f", f"body={body}"],
                capture_output=True, text=True, timeout=15, cwd=str(repo_root),
                check=False,
            )
    except subprocess.SubprocessError:
        return None
    return pr_url


# ─── HTTP ────────────────────────────────────────────────────────────────────


def _submit_http(artifacts_dir: Path, run_id: str,
                 summary: dict[str, Any],
                 console_log: Path) -> tuple[str, str]:
    """POST summary.json + bundle to $RUSH_RESULTS_ENDPOINT."""
    endpoint = os.environ.get("RUSH_RESULTS_ENDPOINT")
    if not endpoint:
        return ("error", "RUSH_RESULTS_ENDPOINT not set; cannot submit via HTTP")
    if not endpoint.startswith(("http://", "https://")):
        return ("error", f"RUSH_RESULTS_ENDPOINT must be http(s)://, got {endpoint!r}")

    # Post summary.json.
    summary_path = artifacts_dir / "summary.json"
    if not summary_path.exists():
        return ("error", f"summary.json not found at {summary_path}")
    try:
        summary_bytes = summary_path.read_bytes()
    except OSError as e:
        return ("error", f"cannot read summary.json: {e}")

    headers = {"Content-Type": "application/json"}
    auth = os.environ.get("RUSH_RESULTS_AUTH", "")
    if auth:
        headers["Authorization"] = auth

    try:
        req = urllib.request.Request(
            endpoint, data=summary_bytes, headers=headers, method="POST"
        )
        with urllib.request.urlopen(req, timeout=30) as resp:
            if resp.status >= 400:
                return ("error", f"HTTP {resp.status} from {endpoint}")
    except urllib.error.HTTPError as e:
        return ("error", f"HTTP {e.code} from {endpoint}: {e.reason}")
    except (urllib.error.URLError, OSError) as e:
        return ("error", f"HTTP submission to {endpoint} failed: {e}")

    # Optionally post the bundle if the endpoint accepts it.
    bundle = _find_bundle(artifacts_dir, run_id)
    if bundle:
        bundle_endpoint = endpoint.rstrip("/") + "/bundle"
        try:
            with open(bundle, "rb") as f:
                bundle_bytes = f.read()
            bundle_headers = {
                "Content-Type": "application/octet-stream",
                "X-Rush-LiveDev-Run-Id": run_id,
            }
            if auth:
                bundle_headers["Authorization"] = auth
            req = urllib.request.Request(
                bundle_endpoint, data=bundle_bytes,
                headers=bundle_headers, method="POST",
            )
            with urllib.request.urlopen(req, timeout=120) as resp:
                if resp.status >= 400:
                    return ("error",
                            f"bundle HTTP {resp.status} from {bundle_endpoint}")
        except (urllib.error.HTTPError, urllib.error.URLError, OSError) as e:
            # Bundle upload failure is non-fatal — summary already posted.
            return ("http", f"summary posted; bundle upload failed: {e}")

    return ("http", "")


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _find_bundle(artifacts_dir: Path, run_id: str) -> str | None:
    for name in (
        f"rush-livedev-results-{run_id}.tar.zst",
        f"rush-livedev-results-{run_id}.tar.gz",
    ):
        p = artifacts_dir.parent / name
        if p.exists():
            return str(p)
    # Also check inside artifacts_dir.
    for name in (
        f"rush-livedev-results-{run_id}.tar.zst",
        f"rush-livedev-results-{run_id}.tar.gz",
    ):
        p = artifacts_dir / name
        if p.exists():
            return str(p)
    return None


def _find_junit(artifacts_dir: Path) -> str | None:
    for name in ("junit.xml", "test-output.xml", "results.xml"):
        p = artifacts_dir / name
        if p.exists():
            return str(p)
        p = artifacts_dir / "guest-diagnostics" / name
        if p.exists():
            return str(p)
    return None


def _human_size(n: int) -> str:
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < 1024:
            return f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} TiB"


# ─── CLI ─────────────────────────────────────────────────────────────────────


def _main() -> int:
    import argparse
    parser = argparse.ArgumentParser(
        prog="rush-livedev-submit",
        description="Submit Rush LiveDev results.",
    )
    parser.add_argument("artifacts_dir",
                        help="path to the artifacts directory")
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--submit", default="local",
                        choices=["none", "local", "github", "http", "auto"])
    parser.add_argument("--summary", default="",
                        help="path to summary.json (default: <artifacts>/summary.json)")
    parser.add_argument("--console-log", default="",
                        help="path to console.log (default: <artifacts>/console.log)")
    ns = parser.parse_args()

    artifacts = Path(ns.artifacts_dir)
    summary_path = Path(ns.summary) if ns.summary else artifacts / "summary.json"
    console_log = Path(ns.console_log) if ns.console_log else artifacts / "console.log"
    if not summary_path.exists():
        print(f"summary.json not found: {summary_path}", file=sys.stderr)
        return 2
    summary = json.loads(summary_path.read_text())
    repo_root = Path(__file__).resolve().parent.parent
    status, err = submit(
        ns.submit, artifacts,
        run_id=ns.run_id, summary=summary,
        console_log=console_log, repo_root=repo_root,
    )
    print(f"submit status: {status}")
    if err:
        print(f"submit error:  {err}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(_main())
