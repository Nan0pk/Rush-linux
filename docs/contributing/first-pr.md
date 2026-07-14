# Your First Contribution to Rush Linux

This is a step-by-step guide to making your first contribution. It assumes
you're comfortable with Git and the command line but new to this project.

---

## 1. Set Up Your Environment

### Prerequisites

- **Linux** (native or container) — this is the canonical build environment
- **Rust toolchain** — install via [rustup](https://rustup.rs/)
- **PowerShell Core** (`pwsh`) — for the repository policy check (optional but recommended)

### Clone and Build

```sh
git clone https://github.com/Nan0pk/Rush-linux.git
cd Rush-linux
cargo build --workspace
```

If this fails, your Rust toolchain may be too old. The project requires
Rust 1.78+ (see `Cargo.toml`). Update with:

```sh
rustup update stable
```

### Run Tests

```sh
cargo test --workspace
```

You should see all tests pass. If they don't, something is wrong with your
environment — ask for help in [Discussions](https://github.com/Nan0pk/Rush-linux/discussions).

---

## 2. Find Something to Work On

### Good First Issues

Browse issues labeled
[`good first issue`](https://github.com/Nan0pk/Rush-linux/labels/good%20first%20issue).
These are specifically chosen for newcomers.

### Common Starter Tasks

| Task Type | Difficulty | Where to Look |
|-----------|-----------|---------------|
| Fix a typo in docs | Easy | Any `.md` file in `docs/` or root |
| Add a unit test | Easy | `crates/optid/src/main.rs` or `crates/optctl/src/main.rs` (find the `#[cfg(test)]` blocks) |
| Improve error messages | Easy | Search for `eprintln!` and `format!` in `crates/` |
| Add a missing doc cross-reference | Easy | `docs/` files that don't link to related docs |
| Implement a `good first issue` | Medium | Check the GitHub issue list |

### Not Sure Where to Start?

Open a [Discussion](https://github.com/Nan0pk/Rush-linux/discussions) with the
category "Q&A" and tell us:
- What languages/tools you're comfortable with
- How much time you have
- What interests you (systems programming, docs, testing, kernel, etc.)

We'll help you find something.

---

## 3. Understand the Codebase

The project has two main Rust programs:

### `optid` — The Adaptive Optimizer Daemon
**Location:** `crates/optid/src/main.rs`

```
main() → run()
  ├── Snapshot::collect()      ← reads PSI, battery, thermal, loadavg
  ├── Policy::load()           ← reads policy.toml
  ├── Policy::decide()         ← picks mode + generates action plan
  ├── Decision::render()       ← formats status text
  ├── Actuator::apply()        ← applies actions (only with --apply)
  └── OptidServer (D-Bus)      ← serves status/explain/set_mode over D-Bus
```

Key concepts:
- **Snapshot**: all sensor readings at one point in time
- **Policy**: thresholds and per-mode configurations
- **Decision**: a mode, a list of reasons, and a list of actions
- **Action**: one guarded write to sysfs or systemd

### `optctl` — The CLI Tool
**Location:** `crates/optctl/src/main.rs`

Talks to `optid` via D-Bus first, falls back to reading `/run/optid/` files.

### Build System
**Location:** `tools/rush-builder.py`

Python script with subcommands: `build`, `repo-init`, `rootfs-create`,
`build-uki`, `vm-image`.

---

## 4. Make Your Change

### Create a Branch

```sh
git checkout -b my-fix
```

### Make the Change

Follow the code style you see in the existing files:
- Rust: idiomatic, no `unsafe`, use `Result`/`Option` properly
- Tests: add them alongside the code in `#[cfg(test)] mod tests`
- Docs: update in the same commit

### Test Locally

```sh
cargo fmt --all -- --check      # formatting
cargo test --workspace           # tests
cargo clippy --workspace --all-targets -- -D warnings   # lint
```

### Commit

Use clear, descriptive commit messages. Examples:

```
fix(optid): add hysteresis to mode transitions
docs(adaptive-engine): clarify thermal override behavior
test(optctl): add test for JSON output with null values
```

---

## 5. Open a Pull Request

1. **Push your branch** to your fork.
2. **Open a PR** against `main` on the Rush Linux repo.
3. **Fill out the PR template** — describe what changed, why, and how you tested.
4. **CI runs automatically** — if it fails, fix the issues and push again.
5. **Wait for review** — we aim for initial feedback within 7 days.

### What Happens in Review

A reviewer will check:
- Does the change work and have tests?
- Is documentation updated?
- Does it align with the project's design rules?
- Are no guardrails weakened?

You may be asked to make changes. That's normal — it's how we maintain quality
together, not a judgment on your skills.

---

## 6. Celebrate!

Once your PR is merged:

- You're officially a Rush Linux contributor! 🎉
- Add yourself to the `AUTHORS` file (or ask the reviewer to do it).
- Tell people! Your contribution matters.
- Look for the next issue to tackle.

---

## Getting Unstuck

| Problem | Solution |
|---------|----------|
| Tests fail locally | Check your Rust version (`rustc --version` needs ≥1.78). Ask in Discussions. |
| Not sure what to work on | Browse `good first issue` labels, or ask in Discussions. |
| CI fails on your PR | Read the error log. Common fixes: `cargo fmt`, missing docs, clippy warnings. |
| No review after 7 days | Gently ping the PR by commenting. The maintainer may be busy. |
| Don't understand the code | Read `docs/architecture.md` and `docs/adaptive-engine.md`, then ask questions. |

**Remember:** Everyone was new to this project once. There are no stupid questions.
