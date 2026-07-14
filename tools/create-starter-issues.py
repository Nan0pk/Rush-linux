#!/usr/bin/env python3
"""
Create GitHub issues with labels for Rush Linux contributor onboarding.
Run this after pushing the onboarding commit to main.

Usage:
  export GH_TOKEN=<your-write-capable-token>
  python3 tools/create-starter-issues.py
"""

import os
import json
import requests
import sys

TOKEN = os.environ.get("GH_TOKEN", "")
if not TOKEN:
    print("Error: set GH_TOKEN environment variable with a write-capable token")
    sys.exit(1)

REPO = "Nan0pk/Rush-linux"
API = f"https://api.github.com/repos/{REPO}"
HEADERS = {
    "Authorization": f"token {TOKEN}",
    "Accept": "application/vnd.github.v3+json",
    "Content-Type": "application/json",
}

# First, create the label set
LABELS = [
    {"name": "good first issue",   "color": "7057ff", "description": "Approachable for new contributors"},
    {"name": "help wanted",        "color": "008672", "description": "Extra attention needed"},
    {"name": "type:bug",           "color": "d73a4a", "description": "Something isn't working"},
    {"name": "type:enhancement",   "color": "a2eeef", "description": "New feature or request"},
    {"name": "type:design",        "color": "bfd4f2", "description": "Architecture or design discussion"},
    {"name": "type:question",      "color": "d876e3", "description": "Further information requested"},
    {"name": "type:documentation", "color": "0075ca", "description": "Improvements or additions to documentation"},
    {"name": "area:optid",         "color": "fbca04", "description": "Adaptive optimizer daemon"},
    {"name": "area:optctl",        "color": "fbca04", "description": "CLI tool"},
    {"name": "area:packaging",     "color": "c5def5", "description": "Build system, recipes, rootfs"},
    {"name": "area:boot",          "color": "c5def5", "description": "UKI, boot, update, rollback"},
    {"name": "area:docs",          "color": "c5def5", "description": "Documentation"},
    {"name": "area:kernel",        "color": "c5def5", "description": "Kernel config fragments"},
    {"name": "area:security",      "color": "b60205", "description": "Security-sensitive changes"},
    {"name": "needs-triage",       "color": "ededed", "description": "Awaiting initial review"},
]

GOOD_FIRST_ISSUES = [
    {
        "title": "Replace hand-rolled JSON builder in optctl with serde_json",
        "body": """## What

`optctl`'s `format_status_as_json()` function (in `crates/optctl/src/main.rs`) manually constructs JSON strings by concatenating string fragments. This is fragile for edge cases (unicode, special characters, deeply nested structures).

## Why

Using `serde_json` is the idiomatic Rust approach and eliminates an entire class of formatting bugs. It also makes it easy to add new fields without manually updating string construction.

## How

1. Add `serde_json` to `crates/optctl/Cargo.toml` dependencies.
2. Define a `StatusJson` struct with `#[derive(Serialize)]` that mirrors the current output shape.
3. Replace `format_status_as_json()` with a function that deserializes the key-value status text into the struct, then serializes to JSON.
4. Update the existing tests to still pass.

## Files to look at

- `crates/optctl/src/main.rs` — `format_status_as_json()` function and its tests
- `crates/optctl/Cargo.toml` — add serde_json dependency

## Acceptance criteria

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] JSON output format is unchanged (existing tests pass)
- [ ] No manual JSON string construction remains

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:optctl", "type:enhancement"],
    },
    {
        "title": "Split optid/src/main.rs into modules",
        "body": """## What

`crates/optid/src/main.rs` is currently a single 1,100-line file containing CLI parsing, sensor reading, policy engine, decision rendering, actuation, D-Bus server, and tests. This should be split into a module structure.

## Suggested structure

```
crates/optid/src/
  main.rs          — CLI entry point and run loop
  args.rs          — Args struct and CLI parsing
  sensors.rs       — Snapshot, Pressure, read_* functions
  policy.rs        — Policy, Thresholds, ModeConfig, decide()
  decision.rs      — Decision struct, render()
  action.rs        — Action enum, Actuator, guarded_write
  dbus.rs          — OptidServer, D-Bus interface
  lib.rs           — (optional) for integration tests
```

## Why

- Makes the codebase navigable for new contributors
- Each module has a clear single responsibility
- Enables targeted testing and review

## How

1. Create the module files
2. Move related types and functions into each module
3. Add `mod` declarations in `main.rs`
4. Ensure `cargo test`, `cargo clippy`, and `cargo fmt` all pass
5. Keep tests in the same module as the code they test (or move to a `tests/` integration directory)

## Acceptance criteria

- [ ] `cargo test --workspace` passes (all existing tests still pass)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] No behavioral change — this is a pure refactor
- [ ] Each module is under ~250 lines

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:optid", "type:enhancement"],
    },
    {
        "title": "Replace hand-rolled TOML parser with toml crate",
        "body": """## What

`Policy::load()` in `crates/optid/src/main.rs` manually parses TOML line-by-line instead of using the `toml` crate. The struct already has `#[derive(serde::Deserialize)]` but the derive is unused — the manual parser bypasses serde entirely.

## Why

The hand-rolled parser:
- Cannot handle nested tables correctly (e.g., `[modes.battery]`)
- Cannot handle inline tables, multiline strings, or arrays
- Must be updated manually for every new config key
- Is ~100 lines of repetitive match arms

Using the `toml` crate replaces all of this with a single `toml::from_str()` call.

## How

1. Add `toml = "0.8"` to `crates/optid/Cargo.toml`
2. Simplify `Policy::load()` to call `toml::from_str()` with error handling
3. Keep the `Default` impl as fallback when the file is missing or invalid
4. Ensure existing tests pass
5. Add a test with a valid TOML file to confirm proper deserialization

## Files to look at

- `crates/optid/src/main.rs` — `Policy::load()` function
- `crates/optid/Cargo.toml` — dependencies
- `config/optid/policy.toml` — the real policy file (should parse correctly)

## Acceptance criteria

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] The `toml` crate is used instead of manual parsing
- [ ] Invalid TOML files fail gracefully (error message + fallback to defaults)

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:optid", "type:enhancement"],
    },
    {
        "title": "Add hysteresis to optid mode transitions",
        "body": """## What

`optid` makes mode decisions every 2 seconds with no dampening. If sensor values hover near a threshold (e.g., `cpu_pressure_avg10` near 12.0), the mode will rapidly oscillate between modes. Add hysteresis so the mode doesn't change unless the new mode has been consistently indicated for several consecutive cycles.

## Why

Oscillating between modes causes:
- Rapid sysfs writes (CPU EPP, platform profile)
- Confusing `optctl explain` output
- Potential performance degradation from constant transitions
- Poor user experience

## Suggested approach

1. Add a `mode_history` or `pending_mode` field that tracks the last N mode decisions
2. Only transition to a new mode if it has been the winning decision for at least 3 consecutive cycles (6 seconds at the default interval)
3. Always allow immediate transitions for critical thermal override (no hysteresis for safety)
4. Log the hysteresis state in the decision report

## Files to look at

- `crates/optid/src/main.rs` — `Policy::auto_mode()` and the main loop
- `docs/adaptive-engine.md` — guardrails section mentions hysteresis as required
- `config/optid/policy.toml` — might add a `hysteresis_cycles` setting

## Acceptance criteria

- [ ] Mode doesn't change more than once per 6 seconds (3 cycles) unless thermal is critical
- [ ] Critical thermal override bypasses hysteresis immediately
- [ ] `optctl explain` shows when hysteresis is delaying a mode change
- [ ] Tests cover the hysteresis behavior
- [ ] `docs/adaptive-engine.md` updated to document the behavior

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:optid", "type:enhancement"],
    },
    {
        "title": "Add CI job to check for broken links in documentation",
        "body": """## What

The project has 45+ Markdown files with many cross-references and links. Add a CI job that checks for broken internal and external links.

## How

1. Add a step to `.github/workflows/ci.yml` (or a new workflow) that runs a link checker.
2. Consider using [lychee](https://github.com/lycheeverse/lychee) — it's fast, written in Rust, and handles Markdown files well.
3. Check:
   - Internal links (relative paths between `.md` files)
   - External URLs
   - Skip links to localhost or private resources
4. Run on push to `main` and on pull requests.

## Example workflow snippet

```yaml
- name: Check links
  uses: lycheeverse/lychee-action@v2
  with:
    args: --base . --no-progress './**/*.md'
```

## Acceptance criteria

- [ ] CI workflow includes a link-check step
- [ ] Broken links cause CI to fail
- [ ] Known-good links in the repo pass the check
- [ ] External link checking respects rate limits

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:docs", "type:enhancement"],
    },
    {
        "title": "Add a dev container for one-click development setup",
        "body": """## What

Create a `.devcontainer/devcontainer.json` so contributors can open the repo in VS Code Dev Containers or GitHub Codespaces and have a working build environment immediately.

## What the container should include

- Rust toolchain (stable, 1.78+)
- PowerShell Core (`pwsh`) for the policy validation script
- Python 3 with `tomllib` support (3.11+) for build tools
- `git`, `curl`, basic build tools
- VS Code extensions: `rust-analyzer`, `vadimcn.vscode-lldb`

## How

1. Create `.devcontainer/devcontainer.json`
2. Use a base image like `mcr.microsoft.com/devcontainers/rust:1` (includes Rust)
3. Add a `postCreateCommand` that runs `cargo build --workspace`
4. Test by opening the repo in Codespaces

## Acceptance criteria

- [ ] `.devcontainer/devcontainer.json` exists
- [ ] `cargo build --workspace` succeeds inside the container
- [ ] `cargo test --workspace` passes
- [ ] `pwsh ./tools/validate-repo.ps1` passes
- [ ] README links to the dev container option

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:packaging", "type:enhancement"],
    },
    {
        "title": "Add more unit tests for edge cases in policy decisions",
        "body": """## What

The `optid` test suite currently has 3 tests. We need more coverage for edge cases.

## Suggested test cases

1. **Low battery on AC power** — battery_pct=15% but on_ac=true → should stay balanced, not battery mode
2. **High CPU pressure + critical thermal** — should prefer thermal protection over performance
3. **IO pressure threshold** — verify background.slice throttling is added when IO pressure exceeds threshold
4. **Memory pressure threshold** — verify MemoryLow and background throttling are added
5. **Manual mode override** — explicitly setting "performance" should override auto-detection
6. **Missing sensors** — when `/proc/pressure/cpu` doesn't exist (Snapshot has None fields), policy should still decide something reasonable
7. **All pressures zero** — no load at all, on AC → should be balanced

## How

Add tests to the `#[cfg(test)] mod tests` block in `crates/optid/src/main.rs`. Follow the pattern of existing tests.

## Acceptance criteria

- [ ] At least 5 new test cases added
- [ ] All tests pass with `cargo test --workspace`
- [ ] Each test has a clear name describing the scenario

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:optid", "type:enhancement"],
    },
    {
        "title": "Document the optid policy engine decision tree as a flowchart",
        "body": """## What

The `Policy::auto_mode()` and `Policy::decide()` methods in `optid` implement a decision tree. This should be documented as a visual flowchart in `docs/adaptive-engine.md`.

## Why

- New contributors can understand the policy logic without reading Rust code
- Reviewers can verify the decision tree matches the implementation
- The ADRs reference "explainable behavior" — a flowchart makes it literal

## How

1. Read `Policy::auto_mode()` and `Policy::decide()` in `crates/optid/src/main.rs`
2. Create an SVG or Mermaid flowchart showing the decision path
3. Embed it in `docs/adaptive-engine.md` using a Mermaid code block (GitHub renders these natively)

## Mermaid example format

```mermaid
flowchart TD
    A[Auto mode] --> B{Critical thermal?}
    B -->|Yes| C[Balanced]
    B -->|No| D{On battery?}
    ...
```

## Acceptance criteria

- [ ] `docs/adaptive-engine.md` contains a Mermaid flowchart
- [ ] The flowchart accurately reflects `auto_mode()` and `decide()` logic
- [ ] All 5 modes (auto, battery, balanced, performance, realtime) are shown
- [ ] All threshold conditions are labeled

Feel free to ask questions in this issue or in Discussions!
""",
        "labels": ["good first issue", "area:docs", "area:optid", "type:documentation"],
    },
]


def main():
    print("Creating labels...")

    # Get existing labels
    r = requests.get(f"{API}/labels", headers=HEADERS)
    existing_labels = set()
    if r.status_code == 200:
        existing_labels = {l["name"] for l in r.json()}

    for label in LABELS:
        if label["name"] in existing_labels:
            print(f"  Label exists: {label['name']}")
            continue
        r = requests.post(f"{API}/labels", headers=HEADERS, json=label)
        if r.status_code in (200, 201):
            print(f"  Created label: {label['name']}")
        else:
            print(f"  Failed to create label {label['name']}: {r.status_code} {r.text[:200]}")

    print(f"\nCreating {len(GOOD_FIRST_ISSUES)} good-first-issues...")

    for issue in GOOD_FIRST_ISSUES:
        r = requests.post(f"{API}/issues", headers=HEADERS, json=issue)
        if r.status_code in (200, 201):
            data = r.json()
            print(f"  Created: #{data['number']} — {issue['title']}")
            print(f"    URL: {data['html_url']}")
        else:
            print(f"  Failed: {issue['title']}")
            print(f"    Error: {r.status_code} {r.text[:200]}")

    print("\nDone!")


if __name__ == "__main__":
    main()
