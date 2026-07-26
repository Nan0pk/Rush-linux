# Rush-linux Workspace Enablement & Execution Plan (Agent Mode)

**Date**: 2026-07-27 (local Asia/Karachi)
**Workspace root**: /home/user/Rush-linux
**Current branch**: work/20260726-workspace-enablement-and-ci-audit
**Repo state**: main at 104061f (post #357)

## 1. Environment Audit (COMPLETED)
- OS: Debian GNU/Linux 13 (trixie)
- Git: 2.47.3
- Python: 3.13.14 (with pytest/ruff available via pip)
- Rust: Installed via rustup (stable 1.97.1) - cargo check --workspace **PASSED**
- No native cargo initially, now functional
- No pwsh (Windows lane skipped locally; CI handles)
- No sudo apt (system pkgs limited, but Rust + Python checks work)
- Tools: All `tools/*.sh`, `tools/*.py` present and executable
- Devcontainer: rust:1 image + postCreate for dbus/pkg-config + cargo build

## 2. CI/CD Workflow Understanding (COMPLETED)
**Primary workflow**: `.github/workflows/ci.yml` — "PR Gate"

**Structure**:
- **classify** job (ubuntu-latest):
  - Detects changed paths using git diff (PR or push base)
  - Sets outputs: rust, python, shell, powershell, windows, workflow, image, dependencies
  - Special rule: change to ci.yml triggers *all* lanes

- **linux** job (always runs core):
  - Python 3.12, shellcheck, actionlint (Go), Rust stable + clippy/rustfmt + dbus/pkg-config
  - Runs `bash tools/checks.sh --ci --section <X> --changed-base <BASE>` for:
    - integrity
    - docs
    - optid (validate-optid-packages.py)
    - evidence (validate-evidence.py)
    - policy (pwsh validate-repo.ps1)
    - workflow (actionlint)
    - shell (bash -n + shellcheck)
    - powershell
    - python (py_compile + ruff + pytest)
    - rust (cargo fmt --check, test, clippy -D warnings, check --all-features)
  - Detailed step summary for root-cause diagnosis (named steps = root causes)
  - Gate reports failures explicitly

- Optional lanes (only if classify says true):
  - dependencies: cargo-deny
  - windows: native pwsh parse + pytest windows-livedev-parity
  - image: archlinux container, mkosi build for livedev/testos + pytest image tests
  - gate: aggregator requiring classify + linux; optional for others

**Legacy aliases**: rust, docs, policy, evidence (for status protection)

**checks.sh**:
- Change-aware (or --all/--quick)
- Sections mirror CI lanes
- Uses validate-* tools + direct cargo/shell/python
- Strict in CI (--ci)
- Failure summary with exact reproduce commands

**Root causes of recent CI issues** (from git history + recent PRs #356/#357):
- F1/F2/D0 status drift: "merged_incomplete" because:
  - Stale verification receipts (PRs modified runtime_entrypoints, deleted test files)
  - validate-optid-packages.py + ledger rules: "A merged PR is not package completion"
  - Missing *production surface* integration tests (must enter via daemon/CLI/optctl, not just in-crate tests)
  - F2: kernel_io seams exist (MemoryKernel/FaultKernel), but "no production-surface integration test"
  - F3 envelope dormant
  - D0 missing: removed-object, child/exec, exit-75, recovery-order, ABI, no-new-privs, feature CI, cold-kernel proof
- Recent fixes: #357 improved CI failure visibility (named steps + summary)
- Image lane, shell, workflow changes are high-risk (trigger full classification)
- No bypasses allowed: must satisfy ledger acceptance_tests, runtime_entrypoints, evidence paths
- Validators enforce honesty (e.g. validate-receipt-freshness, transcript reuse advisory)

**No shortcuts policy**: Always run full `bash tools/checks.sh`, update ledger only with cold-verifiable evidence, use start/finish-work.sh.

## 3. Step-wise Execution Plan (Loop until "stop" or all F2/F1/D0 blockers cleared)

**Principles** (per AGENTS.md, CONTRIBUTING, OPTID-COMPLETION-PLAN):
- One logical work size per PR (small coherent change)
- Use `bash tools/start-work.sh "description"`
- Always `bash tools/finish-work.sh --dry-run` before commit
- Update only affected docs + ledger when package state changes
- Evidence = committed path + transcript (not comments)
- Draft PRs via finish-work.sh (or prepare branch + body)
- Re-check CI: simulate by running checks.sh sections; in real would use `gh run list`
- Fix root causes, not symptoms

### Phase 0: Enablement (DONE)
- [x] Clone + workspace setup
- [x] Rust toolchain + cargo check --workspace
- [x] start-work branch + initial checks pass
- [x] CI workflow + checks.sh + ledger analysis
- [x] Create this WORKSPACE_PLAN.md

### Phase 1: Local CI Parity & Diagnostics (IN PROGRESS)
1.1 Run full local checks (all sections)
1.2 Run full `cargo test --workspace` + clippy
1.3 Run validate-optid-packages.py + full checks.sh
1.4 Document current F2/F1/D0 blockers precisely (from ledger + code)

### Phase 2: F2 Completion (active_general)
**Current ledger** (F2):
- status: merged_incomplete
- PR: 356
- Blocker: "production-surface integration test that enters through the daemon or CLI rather than only in-crate functions"
- F2 seam: kernel_io.rs (MemoryKernel, FaultKernel), io_util, actuator routes through injected KernelIo
- Recent: Fault injection for rename/remove/dir (PR #356)

**Logical steps (small PRs)**:
- Step 2.1: Add a minimal production integration test that exercises optid binary with --dry-run + injected kernel (via test harness or env)
- Step 2.2: Ensure F2 acceptance coverage in ledger (update acceptance_tests if new tests)
- Step 2.3: Run full checks + validate-optid-packages
- Step 2.4: finish-work.sh to commit + prepare PR

**Target for F2 completion**:
- Ledger update to `candidate` only after:
  - Production test passes through `optid` binary path
  - All checks pass
  - Cold verification receipt (simulated here)

### Phase 3: F1 Fresh Verification (merged_incomplete)
- Stale receipt from PR #332
- Add/update behavioral tests in crates/optid/src/policy.rs
- Produce fresh cold-verification receipt file
- Update ledger + docs/plans/optid-verification/

### Phase 4: D0 Safety Prototype
- Prototype extensions per ledger: removed-object, exec, recovery-order etc.
- Use capability_seal_test dir

### Phase 5: Parallel Ready Work
- T1 (merged_incomplete) fix if F2/F3 progress
- R1/R2/R3 research
- Documentation sync if drift

### Phase 6: PR Submission & Re-check Loop
For each logical unit:
1. start-work.sh
2. Implement + tests
3. finish-work.sh --dry-run (must PASS)
4. finish-work.sh "msg"
5. (Simulate) Create branch commit, prepare PR body referencing plan
6. Re-run full checks.sh --ci sections
7. If CI would fail: diagnose root (named step), fix honestly
8. Update WORKSPACE_PLAN.md + ledger if state changes
9. Repeat

**PR size rule**: One package blocker or one validator fix per PR. Logical work size = 1 acceptance test addition or 1 seam extension.

## 4. Current Blockers to Address (from ledger + code inspection)
- F2: Need daemon/CLI entry test using KernelIo injection
- F1: Fresh verification receipt + update stale references
- D0: Missing kernel proofs in prototype
- General: Ensure every merged_incomplete has clear remaining items + evidence paths

## 5. Tools & Commands (Canonical)
- Start: `bash tools/start-work.sh "short desc"`
- Validate: `bash tools/finish-work.sh --dry-run`
- Full: `bash tools/checks.sh --ci --section all --changed-base origin/main`
- Optid: `python3 tools/validate-optid-packages.py --base origin/main`
- Rust: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
- Evidence: `python3 tools/validate-evidence.py`
- Git flow: branch per logical unit; draft PR

## 6. Status Tracking
- Last enablement: 2026-07-27
- Next: Run full tests + F2 test addition
- Loop condition: Continue while active_general=F2 or active_safety=D0 or open merged_incomplete in ledger

**Repo instructions followed**: start/finish-work, checks.sh, no direct main push, draft PRs, honest status in ledger, AGENTS.md.

Ready to execute Phase 1.2+ in next loop iteration.
