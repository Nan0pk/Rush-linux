from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def _run(*args: str, cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )


def test_failed_checks_are_indexed_for_agents(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    tools = repo / "tools"
    tools.mkdir(parents=True)
    shutil.copy2(ROOT / "tools" / "checks.sh", tools / "checks.sh")

    passing_checks = (
        "check-workflow-safety.py",
        "check-repo-hygiene.py",
        "validate-versions.py",
        "validate-doc-sync.py",
        "check-docs-impact.py",
        "render-frontpage.py",
        "validate-evidence.py",
    )
    for name in passing_checks:
        (tools / name).write_text("raise SystemExit(0)\n", encoding="utf-8")

    (tools / "validate-optid-packages.py").write_text(
        'print("FAILED: injected package failure")\nraise SystemExit(3)\n',
        encoding="utf-8",
    )

    assert _run("git", "init", "-q", cwd=repo).returncode == 0
    assert _run("git", "config", "user.email", "ci@example.invalid", cwd=repo).returncode == 0
    assert _run("git", "config", "user.name", "Rush CI", cwd=repo).returncode == 0
    assert _run("git", "add", ".", cwd=repo).returncode == 0
    assert _run("git", "commit", "-qm", "fixture", cwd=repo).returncode == 0

    step_summary = repo / "step-summary.md"
    env = os.environ.copy()
    env["GITHUB_ACTIONS"] = "true"
    env["GITHUB_STEP_SUMMARY"] = str(step_summary)
    result = _run(
        "bash",
        "tools/checks.sh",
        "--changed-base",
        "HEAD",
        cwd=repo,
        env=env,
    )

    output = result.stdout + result.stderr
    risk = "R1/R5 — optid package claims outrun integrated, verified behavior"
    command = "python3 tools/validate-optid-packages.py --base HEAD"

    assert result.returncode == 1
    assert "::error title=Rush CI check failed::" in output
    assert "RUSH CI FAILURE SUMMARY" in output
    assert f"1. {risk}" in output
    assert "exit: 3" in output
    assert f"reproduce: {command}" in output

    summary = step_summary.read_text(encoding="utf-8")
    assert "## Rush CI failure summary" in summary
    assert risk in summary
    assert command in summary
