#!/usr/bin/env python3
"""
flip.py — Atomic BATON.md + STATUS.md updater for the agent-bus protocol.

Usage:
    flip.py --owner BUILDER \\
             --task "trim the bloat" \\
             --state "verifier accept; awaiting trim PR" \\
             --verdict docs/agent-bus/WP-X.verdict.md \\
             --by "Claude (verifier, session-XYZ)" \\
             --action-bullets "Open PR trim/benchmark-results-bloat\\n- 311 MB → 12 KB" \\
             --next-track "Triage research PRs #50/#51/#52"

    flip.py --print   # print current BATON state without changing anything

The script rewrites BATON.md and STATUS.md on the agent-bus branch via the
GitHub Contents API. It reads existing file SHAs, then PUTs new content with
those SHAs for in-place updates. No force-push, no merge commits — atomic
single-commit updates.
"""

from __future__ import annotations
import argparse
import base64
import json
import os
import sys
import urllib.request
from pathlib import Path

REPO        = os.environ.get("AGENT_BUS_REPO", "Nan0pk/Rush-linux")
BRANCH      = os.environ.get("AGENT_BUS_BRANCH", "agent-bus")
API_BASE    = "https://api.github.com"
TOKEN_ENV   = "GITHUB_TOKEN"


def gh_headers() -> dict:
    tok = os.environ.get(TOKEN_ENV)
    if not tok:
        sys.exit(f"ERROR: ${TOKEN_ENV} not set. export $GITHUB_TOKEN before running.")
    return {
        "Authorization":        f"Bearer {tok}",
        "Accept":               "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent":           "agent-bus-flip.py/1.0",
    }


def gh_get(path: str) -> dict:
    req = urllib.request.Request(f"{API_BASE}{path}", headers=gh_headers())
    with urllib.request.urlopen(req) as r:
        return json.loads(r.read())


def gh_put_file(path: str, message: str, content: str, sha: str | None) -> dict:
    body = {
        "message": message,
        "content": base64.b64encode(content.encode()).decode(),
        "branch":  BRANCH,
    }
    if sha:
        body["sha"] = sha
    req = urllib.request.Request(
        f"{API_BASE}/repos/{REPO}/contents/{path}",
        data=json.dumps(body).encode(),
        headers={**gh_headers(), "Content-Type": "application/json"},
        method="PUT",
    )
    with urllib.request.urlopen(req) as r:
        return json.loads(r.read())


def fetch_existing(path: str) -> tuple[str | None, str]:
    """Return (sha, decoded_content) for a path on the ledger branch, or (None, '')."""
    try:
        meta = gh_get(f"/repos/{REPO}/contents/{path}?ref={BRANCH}")
        sha = meta["sha"]
        content_b64 = meta.get("content", "")
        if content_b64:
            # GitHub returns base64 with embedded newlines; strip them.
            content = base64.b64decode(content_b64.replace("\n", "")).decode()
            return sha, content
        return sha, ""
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None, ""
        raise


def render_baton(args) -> str:
    bullets = "\n".join(f"- {b}" for b in (args.action_bullets or []))
    bullets = bullets or "- (none)"
    sections = [
        f"# Agent Bus — Baton",
        "",
        f"OWNER: {args.owner}",
        f"TASK: {args.task}",
        f"STATE: {args.state}",
        f"VERDICT: {args.verdict or '—'}",
        f"UPDATED: {args.updated} by {args.by}",
        "",
        f"## Action for {args.next_owner or 'next-role'}",
        bullets,
        "",
        f"## Next track",
        args.next_track or "(none)",
        "",
        "## Protocol: VERIFIER commits only this ledger + docs/strategy/. BUILDER owns code/data. HUMAN = merge authority. COMPULSORY 5-line update per flip.",
        "",
    ]
    return "\n".join(sections)


def render_status(args) -> str:
    return (
        "# Agent Bus — Human Status\n\n"
        f"**NOW:** {args.now}\n"
        f"**YOUR MOVE:** {args.your_move}\n"
        f"**LAST:** {args.last}\n"
        f"**NEXT:** {args.next_track or '(none)'}\n\n"
        f"Updated: {args.updated} by {args.by}.\n"
    )


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--owner",       choices=["BUILDER", "VERIFIER", "HUMAN"])
    p.add_argument("--next-owner",  help="Who the next flip is targeted at (e.g. BUILDER).")
    p.add_argument("--task",        help="One-line task for the next agent.")
    p.add_argument("--state",       help="One-line evidence-backed state.")
    p.add_argument("--verdict",     help="Path to WP-*.verdict.md or '—'.")
    p.add_argument("--updated",     help="ISO timestamp.")
    p.add_argument("--by",          help="<role> (<session>)")
    p.add_argument("--action-bullets", nargs="*", help="Bullets under 'Action for ...'")
    p.add_argument("--next-track",  help="Upcoming track text.")
    p.add_argument("--now",         help="STATUS: one-line NOW.")
    p.add_argument("--your-move",   help="STATUS: one-line YOUR MOVE.")
    p.add_argument("--last",        help="STATUS: one-line LAST.")
    p.add_argument("--message",     default="ledger: agent-bus flip", help="Commit message.")
    p.add_argument("--dry-run",     action="store_true", help="Print rendered files, do not push.")
    p.add_argument("--print",       action="store_true", help="Just print current BATON.md from ledger.")
    args = p.parse_args()

    if args.print:
        sha, content = fetch_existing("docs/agent-bus/BATON.md")
        print(f"# current BATON.md (sha {sha[:12] if sha else '—'}):\n")
        print(content)
        return 0

    required = ["owner", "task", "state", "updated", "by", "now", "your_move", "last"]
    missing  = [r for r in required if getattr(args, r) is None]
    if missing and not args.dry_run:
        sys.exit(f"ERROR: missing required flags: {', '.join('--' + m.replace('_','-') for m in missing)}")

    baton  = render_baton(args)
    status = render_status(args)
    if args.dry_run:
        print("=== BATON.md (rendered) ===\n" + baton)
        print("\n=== STATUS.md (rendered) ===\n" + status)
        return 0

    sha_b, _   = fetch_existing("docs/agent-bus/BATON.md")
    sha_s, _   = fetch_existing("docs/agent-bus/STATUS.md")
    r1 = gh_put_file("docs/agent-bus/BATON.md",  args.message, baton,  sha_b)
    r2 = gh_put_file("docs/agent-bus/STATUS.md", args.message, status, sha_s)
    print(f"✓ BATON.md  committed: {r1['commit']['sha'][:12]}")
    print(f"✓ STATUS.md committed: {r2['commit']['sha'][:12]}")
    return 0


if __name__ == "__main__":
    main()
