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


def test_failed_section_is_self_identifying(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    tools = repo / "tools"
    tools.mkdir(parents=True)
    shutil.copy2(ROOT / "tools" / "checks.sh", tools / "checks.sh")
    (tools / "validate-optid-packages.py").write_text(
        'print("FAILED: injected package failure")\nraise SystemExit(3)\n',
        encoding="utf-8",
    )

    assert _run("git", "init", "-q", cwd=repo).returncode == 0
    assert _run("git", "config", "user.email", "ci@example.invalid", cwd=repo).returncode == 0
    assert _run("git", "config", "user.name", "Rush CI", cwd=repo).returncode == 0
    assert _run("git", "add", ".", cwd=repo).returncode == 0
    assert _run("git", "commit", "-qm", "fixture", cwd=repo).returncode == 0

    env = os.environ.copy()
    env["GITHUB_ACTIONS"] = "true"
    result = _run(
        "bash",
        "tools/checks.sh",
        "--section",
        "optid",
        "--changed-base",
        "HEAD",
        cwd=repo,
        env=env,
    )

    output = result.stdout + result.stderr
    risk = "R1/R5 — optid package claims outrun integrated, verified behavior"
    command = "python3 tools/validate-optid-packages.py --base HEAD"

    assert result.returncode == 1
    assert "Rush checks: section=optid" in output
    assert "::error title=Rush CI check failed::" in output
    assert "RUSH CI FAILURE SUMMARY" in output
    assert f"1. {risk}" in output
    assert "exit: 3" in output
    assert f"reproduce: {command}" in output


def test_workflow_exposes_logical_checks_as_named_steps() -> None:
    workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
    expected = {
        "Repository integrity": "integrity",
        "Documentation truth": "docs",
        "optid package contract": "optid",
        "Evidence integrity": "evidence",
        "Repository policy": "policy",
        "Workflow syntax": "workflow",
        "Shell entry points": "shell",
        "PowerShell parsing": "powershell",
        "Python and tooling": "python",
        "Rust workspace": "rust",
    }
    for label, section in expected.items():
        assert f'name: "{label} — checks.sh --section {section}"' in workflow
        assert f"--section {section} --changed-base" in workflow

    linux = workflow.split("  linux:\n", 1)[1].split("  dependencies:\n", 1)[0]
    assert "continue-on-error" not in linux
    assert "EmbarkStudios/cargo-deny-action" not in linux
    assert "CI outcome index — failed named steps above are root causes" in linux

    dependencies = workflow.split("  dependencies:\n", 1)[1].split("  windows:\n", 1)[0]
    assert "EmbarkStudios/cargo-deny-action@v2" in dependencies
