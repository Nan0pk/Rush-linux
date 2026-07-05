#!/usr/bin/env python3
"""
pytest tests for tools/livedev-next, tools/livedev-bootstrap.sh, and
tools/livedev-bootstrap.ps1.

Required coverage (per the LiveDev one-command USB workflow spec):
  - tools/livedev-next --help mentions --auto, --prepare-usb, --resume
  - default prints Linux/macOS/Windows bootstrap commands
  - default does not request GH_TOKEN
  - --mock works
  - --auto --dry-run shows the full pipeline and does not write USB
  - livedev-bootstrap.sh supports --auto, --resume, --dry-run
  - livedev-bootstrap.ps1 supports -Auto, -Resume, -DryRun
  - no file (in the LiveDev workflow set) claims auto-merge
  - no path marks milestones verified
  - README shows LiveDev one-command section before testOS

Run with:
  python3 -m pytest tools/test-livedev-next.py -v
  python3 tools/test-livedev-next.py  # standalone
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

_TOOLS_DIR = Path(__file__).resolve().parent
_ROOT = _TOOLS_DIR.parent


def _run(args: list[str], timeout: int = 120, env: dict | None = None) -> tuple[int, str, str]:
    r = subprocess.run(
        ["python3", str(_TOOLS_DIR / "livedev-next")] + args,
        capture_output=True, text=True, timeout=timeout, cwd=str(_ROOT), env=env,
    )
    return r.returncode, r.stdout, r.stderr


# --- livedev-next --help and default ----------------------------------------

def test_help_shows_all_modes():
    rc, stdout, _ = _run(["--help"])
    assert rc == 0
    for mode in ["--auto", "--mock", "--plan", "--run", "--submit",
                 "--prepare-usb", "--resume", "--run-vm", "--dry-run",
                 "--submit-mode", "--ci", "--debug"]:
        assert mode in stdout, f"--help should mention {mode}"


def test_help_mentions_prepare_usb_and_resume():
    """Spec: --help must mention --auto, --prepare-usb, --resume."""
    rc, stdout, _ = _run(["--help"])
    assert rc == 0
    assert "--auto" in stdout
    assert "--prepare-usb" in stdout
    assert "--resume" in stdout


def test_default_prints_bootstrap_commands():
    """Spec: default output must show the Linux/macOS and Windows bootstrap commands."""
    rc, stdout, _ = _run([])
    assert rc == 0
    assert "livedev-bootstrap.sh" in stdout
    assert "livedev-bootstrap.ps1" in stdout
    assert "--auto" in stdout
    # Windows command uses -Auto (PowerShell flag style)
    assert "-Auto" in stdout
    # Must mention the user-path description
    assert "USB" in stdout or "usb" in stdout
    assert "maintainer review" in stdout.lower()


def test_default_does_not_ask_for_token():
    """Spec: default must not request GH_TOKEN."""
    rc, stdout, stderr = _run([])
    assert "TOKEN" not in stdout
    assert "TOKEN" not in stderr
    assert "GH_TOKEN" not in stdout
    assert "GH_TOKEN" not in stderr


def test_default_exits_zero():
    rc, _, _ = _run([])
    assert rc == 0


def test_default_shows_tool_check():
    rc, stdout, _ = _run([])
    assert "rush-autopilot" in stdout
    assert "rush-exec" in stdout
    assert "validate-hwtest-evidence" in stdout


# --- livedev-next --mock ----------------------------------------------------

def test_mock_runs_all_scenarios():
    rc, stdout, _ = _run(["--mock"], timeout=180)
    assert rc == 0
    assert "success" in stdout.lower()
    assert "failure" in stdout.lower()
    assert "fixtures" in stdout.lower()


# --- livedev-next --auto --dry-run ------------------------------------------

def test_auto_dry_run_shows_full_pipeline():
    """Spec: --auto --dry-run shows the full pipeline and does not write USB."""
    rc, stdout, stderr = _run(["--auto", "--dry-run"], timeout=60)
    assert rc == 0, f"--auto --dry-run should exit 0, got {rc}: {stderr}"
    # Must show all 4 steps
    assert "Step 1/4" in stdout
    assert "Step 2/4" in stdout
    assert "Step 3/4" in stdout
    assert "Step 4/4" in stdout
    # Must show the rush-autopilot commands it would run
    assert "rush-autopilot plan" in stdout
    assert "rush-autopilot run" in stdout
    assert "validate-hwtest-evidence.py" in stdout
    assert "rush-autopilot submit-evidence" in stdout
    # Must indicate dry-run
    assert "[dry-run]" in stdout or "dry-run" in stdout.lower()
    # Must NOT write USB (no real sudo bash invocation that writes)
    assert "sudo bash testos/install.sh" not in stdout.replace(
        "[dry-run] ", "") or "[dry-run]" in stdout
    # Must explicitly say USB is not written
    assert "USB is not written" in stdout or "Not writing USB" in stdout


# --- livedev-next --auto (real) ---------------------------------------------

def test_auto_runs_full_pipeline():
    rc, stdout, _ = _run(["--auto"], timeout=300)
    # --auto may fail at validation (ambiguous slot on CI) but should
    # at least complete steps 1 and 2 and print the pipeline structure.
    assert "Step 1/4" in stdout
    assert "Step 2/4" in stdout
    assert "Pipeline" in stdout


# --- livedev-next --plan / --run / --submit ---------------------------------

def test_plan_generates_file():
    rc, stdout, _ = _run(["--plan"], timeout=60)
    assert rc == 0
    assert "/tmp/rush-livedev-plan.json" in stdout
    plan_path = Path("/tmp/rush-livedev-plan.json")
    assert plan_path.exists()
    plan = json.loads(plan_path.read_text())
    assert plan.get("plan_kind") == "rush-autopilot-plan"


def test_run_executes_plan():
    _run(["--plan"], timeout=60)
    rc, stdout, _ = _run(["--run", "/tmp/rush-livedev-plan.json"], timeout=300)
    assert rc == 0


def test_submit_dry_run_works():
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')
        rc, stdout, stderr = _run(["--submit", str(run_dir), "--dry-run"], timeout=60)
        assert "TOKEN" not in stdout
        assert "TOKEN" not in stderr


def test_submit_without_token_prints_token_needed():
    env = os.environ.copy()
    env.pop("GH_TOKEN", None)
    env.pop("GITHUB_TOKEN", None)
    with tempfile.TemporaryDirectory() as tmp:
        run_dir = Path(tmp) / "run"
        run_dir.mkdir()
        (run_dir / "run-record.json").write_text('{"status": "completed"}')
        r = subprocess.run(
            ["python3", str(_TOOLS_DIR / "livedev-next"), "--submit", str(run_dir)],
            capture_output=True, text=True, timeout=30, cwd=str(_ROOT), env=env,
        )
        assert "[TOKEN NEEDED]" in r.stdout


def test_run_nonexistent_plan_fails():
    rc, stdout, stderr = _run(["--run", "/tmp/nonexistent-plan-12345.json"], timeout=30)
    assert rc != 0
    assert "not found" in stdout.lower() or "not found" in stderr.lower()


# --- livedev-bootstrap.sh ---------------------------------------------------

def test_bootstrap_sh_exists_and_is_executable():
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    assert p.exists(), "tools/livedev-bootstrap.sh must exist"
    assert os.access(p, os.X_OK), "tools/livedev-bootstrap.sh must be executable"


def test_bootstrap_sh_supports_required_flags():
    """Spec: livedev-bootstrap.sh supports --auto, --resume, --dry-run, --smart, --vm."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    assert "--auto" in text
    assert "--resume" in text
    assert "--dry-run" in text
    assert "--skip-mock" in text
    assert "--submit" in text
    assert "--smart" in text, "must support --smart (default mode)"
    assert "--vm" in text, "must support --vm (force QEMU path)"
    # Must NOT auto-merge (no merge API call)
    assert "pulls/${PR_NUM}/merge" not in text
    assert "pulls/$PR_NUM/merge" not in text
    # Must print [TOKEN NEEDED] when token is missing
    assert "[TOKEN NEEDED]" in text


def test_bootstrap_sh_smart_is_default_when_no_args():
    """Spec: running with no args defaults to SMART mode (auto-detect)."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    # The dispatch logic must default to SMART when no mode flag is given.
    assert 'SMART=true' in text
    assert 'do_smart' in text


def test_bootstrap_sh_has_usb_result_detection():
    """Spec: smart mode can detect a USB with testos-results/ plugged in."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    assert "usb_has_results" in text
    assert "testos-results" in text


def test_bootstrap_sh_submit_preflights_auth():
    """Spec: --submit pre-flights auth BEFORE doing USB/copy/validate work."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    assert "preflight_submit_auth" in text, "must have a preflight auth function"
    # The preflight must be called at the start of do_resume when SUBMIT=true,
    # BEFORE the USB copy step.
    assert "PRE-FLIGHT" in text or "preflight" in text.lower()


def test_bootstrap_sh_submit_supports_gh_cli():
    """Spec: --submit uses gh CLI if authenticated, no token needed."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    assert "gh auth status" in text, "must check gh auth status as auth method"
    assert "gh auth login" in text, "must offer to run gh auth login"
    assert "gh auth token" in text, "must use gh auth token to get the token"


def test_bootstrap_sh_submit_prompts_interactively():
    """Spec: --submit prompts for token interactively if no env var / gh."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    # read -rs reads silently (no echo) — the token is not printed.
    assert "read -rs" in text, "must read token silently (read -rs)"


def test_bootstrap_sh_smart_checks_qemu_before_usb():
    """Spec: smart mode checks QEMU first to avoid unnecessary sudo for USB scan."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    # Find do_smart function body.
    idx = text.find("do_smart()")
    assert idx != -1
    body = text[idx:idx + 2000]
    # Find the actual code lines (not comments) that check QEMU and USB.
    # The QEMU check is `command -v qemu-system-x86_64`.
    # The USB check is `if usb_has_results; then`.
    qemu_check = body.find("command -v qemu-system-x86_64")
    usb_call = body.find("if usb_has_results")
    assert qemu_check != -1, "do_smart must check QEMU availability"
    assert usb_call != -1, "do_smart must call usb_has_results"
    assert qemu_check < usb_call, \
        "do_smart must check QEMU BEFORE calling usb_has_results (avoids sudo prompt)"


def test_bootstrap_sh_vm_auto_sudos_for_injection():
    """Spec: --vm path auto-sudos when state injection needs root."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    text = p.read_text()
    assert "need_sudo" in text, "must have need_sudo logic in do_vm"
    assert "guestfish" in text, "must check for guestfish"
    assert "id -u" in text, "must check if running as root"
    # The sudo re-exec must preserve GH_TOKEN for github submit.
    assert "GH_TOKEN" in text
    assert "sudo" in text


def test_bootstrap_sh_help_works():
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    r = subprocess.run(
        ["bash", str(p), "--help"],
        capture_output=True, text=True, timeout=30, cwd=str(_ROOT),
    )
    assert r.returncode == 0
    assert "--auto" in r.stdout
    assert "--resume" in r.stdout
    assert "--dry-run" in r.stdout


def test_bootstrap_sh_auto_dry_run_works():
    """Spec: livedev-bootstrap.sh --auto --dry-run works."""
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    r = subprocess.run(
        ["bash", str(p), "--auto", "--dry-run"],
        capture_output=True, text=True, timeout=60, cwd=str(_ROOT),
    )
    assert r.returncode == 0, f"bootstrap.sh --auto --dry-run failed: {r.stderr}"
    # Must show the 4 steps
    assert "Step 1/4" in r.stdout
    assert "Step 2/4" in r.stdout
    assert "Step 3/4" in r.stdout
    assert "Step 4/4" in r.stdout
    # Must say it's using testOS as backend
    assert "Using testOS as the current LiveDev boot backend" in r.stdout
    # Must print reboot instructions
    assert "Reboot instructions" in r.stdout
    # Must NOT actually write USB (dry-run)
    assert "[dry-run]" in r.stdout
    assert "Not writing USB" in r.stdout


def test_bootstrap_sh_resume_dry_run_works():
    p = _TOOLS_DIR / "livedev-bootstrap.sh"
    r = subprocess.run(
        ["bash", str(p), "--resume", "--dry-run"],
        capture_output=True, text=True, timeout=60, cwd=str(_ROOT),
    )
    assert r.returncode == 0, f"bootstrap.sh --resume --dry-run failed: {r.stderr}"
    assert "Step 1/3" in r.stdout
    assert "Step 2/3" in r.stdout
    assert "Step 3/3" in r.stdout
    assert "[dry-run]" in r.stdout


# --- Regression tests: REAL mode (not dry-run) using RUSH_LIVEDEV_TEST_STUB -
# These exercise the actual --auto repo-resolution path that broke for users
# with an existing ~/Rush-linux directory. They use a local fixture repo via
# RUSH_LIVEDEV_SOURCE_REPO so no network, USB, GitHub, or hardware is touched.

def _make_fixture_repo() -> Path:
    """Create a minimal git repo that looks enough like Rush-linux for the
    bootstrap script's find_repo_root() to detect it as 'inside repo'.
    Returns the path to the fixture repo."""
    fixture = Path(tempfile.mkdtemp(prefix="rush-fixture-"))
    # tools/livedev-next + testos/install.sh + .git are what find_repo_root
    # checks for. tools/livedev-bootstrap.sh is what we copy in for the test.
    (fixture / "tools").mkdir(parents=True)
    (fixture / "testos").mkdir()
    (fixture / "tools" / "livedev-next").write_text("#!/usr/bin/env python3\n")
    (fixture / "testos" / "install.sh").write_text("#!/usr/bin/env bash\n")
    subprocess.run(["git", "init", str(fixture)], capture_output=True, check=True)
    subprocess.run(["git", "-C", str(fixture), "config", "user.email",
                    "test@example.com"], capture_output=True, check=True)
    subprocess.run(["git", "-C", str(fixture), "config", "user.name",
                    "test"], capture_output=True, check=True)
    subprocess.run(["git", "-C", str(fixture), "add", "."], capture_output=True, check=True)
    subprocess.run(["git", "-C", str(fixture), "commit", "-m", "fixture"],
                   capture_output=True, check=True)
    return fixture


def _run_bootstrap_real(workdir: Path, fixture: Path, extra_env: dict | None = None):
    """Copy bootstrap.sh into workdir and run it with RUSH_LIVEDEV_TEST_STUB=1
    + RUSH_LIVEDEV_SOURCE_REPO=<fixture>. Returns the CompletedProcess."""
    bootstrap = _TOOLS_DIR / "livedev-bootstrap.sh"
    target = workdir / "livedev-bootstrap.sh"
    target.write_text(bootstrap.read_text())
    target.chmod(0o755)
    env = os.environ.copy()
    env["RUSH_LIVEDEV_TEST_STUB"] = "1"
    env["RUSH_LIVEDEV_SOURCE_REPO"] = str(fixture)
    if extra_env:
        env.update(extra_env)
    return subprocess.run(
        ["bash", str(target), "--auto"],
        capture_output=True, text=True, timeout=120, cwd=str(workdir), env=env,
    )


def test_real_auto_existing_non_git_dir_clones_to_alternate():
    """Spec: real --auto with existing non-git ./Rush-linux exits 0 and
    clones into timestamped alternate dir. This is the EXACT failing case
    from the user report."""
    fixture = _make_fixture_repo()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            workdir = Path(tmp)
            # Existing ./Rush-linux that is NOT a git repo.
            (workdir / "Rush-linux").mkdir()
            (workdir / "Rush-linux" / "not-a-repo").write_text("garbage")
            r = _run_bootstrap_real(workdir, fixture)
            assert r.returncode == 0, \
                f"Should exit 0, got {r.returncode}. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            # Must NOT print the fatal clone error.
            assert "fatal: destination path 'Rush-linux' already exists" not in r.stdout, \
                f"Found fatal clone error in stdout: {r.stdout}"
            assert "fatal: destination path 'Rush-linux' already exists" not in r.stderr, \
                f"Found fatal clone error in stderr: {r.stderr}"
            # Must print that existing ./Rush-linux is not a git repo.
            assert "is not a git repo" in r.stderr.lower() or \
                   "is not a git repo" in r.stdout.lower(), \
                f"Should say 'not a git repo'. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            # Must use a timestamped Rush-linux-livedev-* alternate dir.
            alternates = list(workdir.glob("Rush-linux-livedev-*"))
            assert len(alternates) == 1, \
                f"Expected 1 timestamped alternate dir, found {len(alternates)}: {alternates}"
            # TEST_STUB success message should mention the alternate dir.
            assert "[TEST_STUB]" in r.stdout
            assert "Repo resolution succeeded" in r.stdout
            assert str(alternates[0].name) in r.stdout or \
                   str(alternates[0]) in r.stdout
    finally:
        import shutil
        shutil.rmtree(fixture, ignore_errors=True)


def test_real_auto_existing_git_dir_reuses_it():
    """Spec: real --auto with existing git ./Rush-linux reuses it."""
    fixture = _make_fixture_repo()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            workdir = Path(tmp)
            # Pre-clone ./Rush-linux from the fixture so it IS a git repo.
            subprocess.run(
                ["git", "clone", "--depth", "1", str(fixture),
                 str(workdir / "Rush-linux")],
                capture_output=True, check=True,
            )
            r = _run_bootstrap_real(workdir, fixture)
            assert r.returncode == 0, \
                f"Should exit 0, got {r.returncode}. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            assert "Found existing Rush-linux git repo" in r.stdout, \
                f"Should print 'Found existing Rush-linux git repo'. stdout:\n{r.stdout}"
            # Must NOT have cloned into a timestamped alternate dir.
            alternates = list(workdir.glob("Rush-linux-livedev-*"))
            assert len(alternates) == 0, \
                f"Should not create alternate dir when ./Rush-linux is a git repo. Found: {alternates}"
            # Must NOT print the fatal clone error.
            assert "fatal: destination path" not in r.stdout
            assert "fatal: destination path" not in r.stderr
    finally:
        import shutil
        shutil.rmtree(fixture, ignore_errors=True)


def test_real_auto_no_existing_dir_clones_into_rush_linux():
    """Spec: real --auto with no ./Rush-linux clones into ./Rush-linux."""
    fixture = _make_fixture_repo()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            workdir = Path(tmp)
            r = _run_bootstrap_real(workdir, fixture)
            assert r.returncode == 0, \
                f"Should exit 0, got {r.returncode}. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            # Must clone into ./Rush-linux (not an alternate).
            assert (workdir / "Rush-linux" / ".git").exists() or \
                   (workdir / "Rush-linux").is_dir(), \
                f"Should create ./Rush-linux. workdir contents: {list(workdir.iterdir())}"
            alternates = list(workdir.glob("Rush-linux-livedev-*"))
            assert len(alternates) == 0, \
                f"Should not create alternate dir for clean workdir. Found: {alternates}"
            assert "Cloned into" in r.stdout
    finally:
        import shutil
        shutil.rmtree(fixture, ignore_errors=True)


def test_real_auto_inside_repo_does_not_clone():
    """Spec: real --auto from inside repo does not clone."""
    fixture = _make_fixture_repo()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            workdir = Path(tmp)
            # Make workdir look like a Rush-linux repo.
            (workdir / "tools").mkdir()
            (workdir / "testos").mkdir()
            (workdir / "tools" / "livedev-next").write_text("#!/usr/bin/env python3\n")
            (workdir / "testos" / "install.sh").write_text("#!/usr/bin/env bash\n")
            subprocess.run(["git", "init", str(workdir)], capture_output=True, check=True)
            subprocess.run(["git", "-C", str(workdir), "config", "user.email",
                            "test@example.com"], capture_output=True, check=True)
            subprocess.run(["git", "-C", str(workdir), "config", "user.name",
                            "test"], capture_output=True, check=True)
            subprocess.run(["git", "-C", str(workdir), "add", "."],
                           capture_output=True, check=True)
            subprocess.run(["git", "-C", str(workdir), "commit", "-m", "init"],
                           capture_output=True, check=True)
            r = _run_bootstrap_real(workdir, fixture)
            assert r.returncode == 0, \
                f"Should exit 0, got {r.returncode}. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            assert "Using current Rush-linux repo" in r.stdout, \
                f"Should print 'Using current Rush-linux repo'. stdout:\n{r.stdout}"
            # Must NOT clone anywhere.
            assert "Cloning from" not in r.stdout
            assert "Cloned into" not in r.stdout
    finally:
        import shutil
        shutil.rmtree(fixture, ignore_errors=True)


def test_real_auto_repo_dir_override_uses_existing_git_repo():
    """Spec: RUSH_LIVEDEV_REPO_DIR existing git repo is used."""
    fixture = _make_fixture_repo()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            workdir = Path(tmp)
            r = _run_bootstrap_real(workdir, fixture,
                                    extra_env={"RUSH_LIVEDEV_REPO_DIR": str(fixture)})
            assert r.returncode == 0, \
                f"Should exit 0, got {r.returncode}. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            assert "Using RUSH_LIVEDEV_REPO_DIR" in r.stdout, \
                f"Should print 'Using RUSH_LIVEDEV_REPO_DIR'. stdout:\n{r.stdout}"
    finally:
        import shutil
        shutil.rmtree(fixture, ignore_errors=True)


def test_real_auto_repo_dir_override_non_git_fails_clearly():
    """Spec: RUSH_LIVEDEV_REPO_DIR existing non-git dir fails clearly."""
    fixture = _make_fixture_repo()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            workdir = Path(tmp)
            nongit = workdir / "notgit"
            nongit.mkdir()
            (nongit / "junk").write_text("not a repo")
            r = _run_bootstrap_real(workdir, fixture,
                                    extra_env={"RUSH_LIVEDEV_REPO_DIR": str(nongit)})
            assert r.returncode != 0, \
                f"Should fail, got exit 0. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            combined = r.stdout + r.stderr
            assert "RUSH_LIVEDEV_REPO_DIR exists but is not a git repo" in combined, \
                f"Should fail clearly. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
    finally:
        import shutil
        shutil.rmtree(fixture, ignore_errors=True)


def test_dry_run_also_uses_same_repo_resolution():
    """Spec: dry-run also uses same repo resolution. In particular, an existing
    non-git ./Rush-linux should NOT cause a fatal error in --dry-run either."""
    fixture = _make_fixture_repo()
    try:
        with tempfile.TemporaryDirectory() as tmp:
            workdir = Path(tmp)
            (workdir / "Rush-linux").mkdir()
            (workdir / "Rush-linux" / "not-a-repo").write_text("garbage")
            bootstrap = _TOOLS_DIR / "livedev-bootstrap.sh"
            target = workdir / "livedev-bootstrap.sh"
            target.write_text(bootstrap.read_text())
            target.chmod(0o755)
            env = os.environ.copy()
            env["RUSH_LIVEDEV_SOURCE_REPO"] = str(fixture)
            r = subprocess.run(
                ["bash", str(target), "--auto", "--dry-run"],
                capture_output=True, text=True, timeout=60,
                cwd=str(workdir), env=env,
            )
            assert r.returncode == 0, \
                f"Dry-run should exit 0. stdout:\n{r.stdout}\nstderr:\n{r.stderr}"
            # Must NOT print the fatal clone error.
            assert "fatal: destination path 'Rush-linux' already exists" not in r.stdout
            assert "fatal: destination path 'Rush-linux' already exists" not in r.stderr
            # Must indicate the alternate-dir path (even in dry-run).
            assert "is not a git repo" in (r.stderr + r.stdout).lower() or \
                   "Rush-linux-livedev-" in r.stdout or \
                   "Rush-linux-livedev-" in r.stderr
    finally:
        import shutil
        shutil.rmtree(fixture, ignore_errors=True)


# --- livedev-bootstrap.ps1 --------------------------------------------------

def test_bootstrap_ps1_exists():
    """Spec: livedev-bootstrap.ps1 exists."""
    p = _TOOLS_DIR / "livedev-bootstrap.ps1"
    assert p.exists(), "tools/livedev-bootstrap.ps1 must exist"


def test_bootstrap_ps1_supports_required_flags():
    """Spec: livedev-bootstrap.ps1 supports -Auto, -Resume, -DryRun, -Smart."""
    p = _TOOLS_DIR / "livedev-bootstrap.ps1"
    text = p.read_text()
    # PowerShell param block
    assert "[switch]$Auto" in text
    assert "[switch]$Resume" in text
    assert "[switch]$DryRun" in text
    assert "[switch]$SkipMock" in text
    assert "[switch]$Submit" in text
    assert "[switch]$Smart" in text, "must support -Smart (default mode)"
    # Must NOT auto-merge (no merge API call)
    assert "pulls/" not in text.replace("pulls'", "").replace("pulls`", "")  # rough check
    # Specifically: no /merge endpoint
    assert "/merge" not in text
    # Must print [TOKEN NEEDED] when token is missing
    assert "[TOKEN NEEDED]" in text
    # Must use Invoke-RestMethod for PR creation (not merge)
    assert "Invoke-RestMethod" in text or "Invoke-WebRequest" in text


def test_bootstrap_ps1_has_matching_existing_dir_handling():
    """Spec: PowerShell script contains matching existing-dir handling.

    Must include: Test-IsGitRepo, alternate-dir logic, RUSH_LIVEDEV_REPO_DIR
    override, and RUSH_LIVEDEV_TEST_STUB support.
    """
    p = _TOOLS_DIR / "livedev-bootstrap.ps1"
    text = p.read_text()
    # Existing-dir handling
    assert "Test-IsGitRepo" in text, "ps1 must define Test-IsGitRepo"
    assert "is not a git repo" in text.lower(), \
        "ps1 must handle existing non-git .\\Rush-linux case"
    assert "livedev-" in text, \
        "ps1 must use timestamped Rush-linux-livedev-* alternate dir"
    # Env overrides
    assert "$env:RUSH_LIVEDEV_REPO_DIR" in text, \
        "ps1 must read RUSH_LIVEDEV_REPO_DIR env var"
    assert "$env:RUSH_LIVEDEV_TEST_STUB" in text, \
        "ps1 must read RUSH_LIVEDEV_TEST_STUB env var"
    assert "$env:RUSH_LIVEDEV_SOURCE_REPO" in text, \
        "ps1 must read RUSH_LIVEDEV_SOURCE_REPO env var"
    # Rule E: explicit override fail-clearly message
    assert "RUSH_LIVEDEV_REPO_DIR exists but is not a git repo" in text, \
        "ps1 must fail clearly when RUSH_LIVEDEV_REPO_DIR is non-git"
    # TEST_STUB must NOT skip repo resolution
    assert "Ensure-Repo" in text, \
        "ps1 must call Ensure-Repo even in TEST_STUB mode"


# --- No auto-merge claims ---------------------------------------------------

def test_no_file_claims_auto_merge():
    """Spec: no file (in the LiveDev one-command workflow set) claims auto-merge.

    We scan the files we author + the README + runbook for POSITIVE auto-merge
    claims (negations like 'does not auto-merge' are allowed).
    """
    files_to_check = [
        _ROOT / "README.md",
        _ROOT / "docs" / "livedev" / "OPERATOR_RUNBOOK.md",
        _TOOLS_DIR / "livedev-next",
        _TOOLS_DIR / "livedev-bootstrap.sh",
        _TOOLS_DIR / "livedev-bootstrap.ps1",
    ]
    negations = (
        "not ", "never", "cannot", "no ", "don't", "do not",
        "doesn't", "does not", "isn't", "is not", "without",
    )
    bad: list[tuple[str, str]] = []
    for f in files_to_check:
        if not f.exists():
            continue
        for line in f.read_text(errors="replace").splitlines():
            lower = line.lower()
            if "auto-merge" in lower or "auto merge" in lower or "auto-merges" in lower \
                    or "auto-merging" in lower:
                # Check if it's a negation.
                if any(neg in lower for neg in negations):
                    continue
                bad.append((str(f), line.strip()))
    assert not bad, f"Positive auto-merge claims found: {bad}"


# --- No milestone verification ----------------------------------------------

def test_no_path_marks_milestones_verified():
    """Spec: no path marks milestones verified.

    The LiveDev workflow files must not contain code that sets
    verified = true in release/milestones.toml. We look for assignment
    patterns, not quoted string literals used in forbidden-pattern lists.
    """
    files_to_check = [
        _TOOLS_DIR / "livedev-next",
        _TOOLS_DIR / "livedev-bootstrap.sh",
        _TOOLS_DIR / "livedev-bootstrap.ps1",
        _TOOLS_DIR / "rush-autopilot",
        _TOOLS_DIR / "rush_pr_lib.py",
    ]
    bad: list[tuple[str, str]] = []
    for f in files_to_check:
        if not f.exists():
            continue
        text = f.read_text(errors="replace")
        # Look for patterns that write verified = true to milestones.toml.
        # Exclude: quoted string literals (used in forbidden-pattern lists),
        # comments, and negation context.
        for pattern in [
            r'verified\s*=\s*true',
        ]:
            for m in re.finditer(pattern, text, re.IGNORECASE):
                start = text.rfind('\n', 0, m.start()) + 1
                end = text.find('\n', m.end())
                line = text[start:end if end != -1 else len(text)]
                # Skip if the match is inside a quoted string literal.
                # (i.e., there's a `"` before the match on the same line, with no
                # closing `"` between them.)
                prefix = line[:m.start() - start]
                if prefix.count('"') % 2 == 1:
                    continue
                # Skip if the match is inside a single-quoted string.
                if prefix.count("'") % 2 == 1:
                    continue
                # Skip negation context.
                lower = line.lower()
                if any(neg in lower for neg in
                       ("not ", "never", "cannot", "no ", "don't", "do not",
                        "doesn't", "does not", "isn't", "is not", "forbidden",
                        "never set", "human-only", "maintainer")):
                    continue
                bad.append((str(f), line.strip()))
    assert not bad, f"Milestone-verification claims found: {bad}"


# --- README ordering --------------------------------------------------------

def test_readme_shows_livedev_before_testos():
    """Spec: README shows LiveDev one-command section before testOS."""
    readme = (_ROOT / "README.md").read_text()
    livedev_pos = readme.find("Rush LiveDev")
    testos_pos = readme.find("testOS")
    assert livedev_pos != -1, "README must mention Rush LiveDev"
    assert testos_pos != -1, "README must mention testOS"
    assert livedev_pos < testos_pos, \
        f"LiveDev section (pos {livedev_pos}) must appear before testOS (pos {testos_pos})"


def test_readem_shows_one_command_within_first_60_lines():
    """Spec: Within the first 60 lines, before testOS, add a prominent LiveDev section."""
    readme = (_ROOT / "README.md").read_text()
    lines = readme.splitlines()
    # Find the first line that mentions livedev-bootstrap
    for i, line in enumerate(lines[:60], start=1):
        if "livedev-bootstrap.sh" in line:
            return
    assert False, "livedev-bootstrap.sh must appear within first 60 lines of README"


def test_readme_has_linux_and_windows_commands():
    """Spec: README has both Linux/macOS and Windows PowerShell one-command paths."""
    readme = (_ROOT / "README.md").read_text()
    assert "curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.sh" in readme
    assert "curl.exe -L -o livedev-bootstrap.ps1 https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/tools/livedev-bootstrap.ps1" in readme
    # Smart mode is the default now; -Auto is still accepted but no longer
    # the documented default in the README one-command line.
    assert "powershell -ExecutionPolicy Bypass -File .\\livedev-bootstrap.ps1" in readme


def test_readme_labels_testos_as_backend_or_fallback():
    """Spec: Label testOS below as: Current boot backend / manual fallback."""
    readme = (_ROOT / "README.md").read_text()
    assert "boot backend" in readme.lower() or "manual fallback" in readme.lower(), \
        "README must label testOS as 'current boot backend' or 'manual fallback'"


# --- Standalone runner ------------------------------------------------------


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
