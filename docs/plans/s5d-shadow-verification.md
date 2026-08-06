# S5D shadow verification outside GitHub Actions

This procedure reproduces the read-only proof portion of the S5D post-merge
verifier when GitHub Actions is unavailable. It is intentionally separate from
PR #393 and does not alter that verifier's source boundary.

## Authority boundary

The shadow verifier is diagnostic evidence only.

It must not:

- create `docs/plans/optid-verification/s5d.toml`;
- change S5D from `candidate` to `completed`;
- advance C1;
- invent GitHub workflow, job, or artifact identifiers;
- merge or replace PR #393; or
- claim that a local run is the repository's official cold-verification receipt.

The script binds itself to:

- integrated `main`: `f3d785df064c9b2734509307bd1b33cf409ea9fb`;
- PR #392 implementation head: `f1b38e3e4b1b1b8f2e48a65eeb84a31b600654c6`;
- the current S5D `candidate` ledger state; and
- every S5D runtime, test, and completion-evidence path declared by the ledger.

It fails if `origin/main` moves, a declared S5D source path changes, the checkout
is dirty, the package pre-state changes, or a verification receipt already
exists.

## Safe test-only run

From a clean checkout of branch `work/20260807-s5d-shadow-verifier`:

```sh
bash tools/verify-s5d-shadow.sh \
  --expected-head "$(git rev-parse HEAD)"
```

This runs:

- source and ledger provenance checks;
- current-work, package-ledger, README, and whitespace validation;
- Rust formatting, all-target/all-feature checking, and strict Clippy;
- all thirteen mapped S5D acceptance tests individually with `--exact`;
- the complete optid test suite; and
- the complete workspace test suite.

The default evidence directory is under `/tmp` and contains the proof log,
resolved acceptance-test list, environment metadata, and a SHA-256 manifest.
Use `--output-dir PATH` to choose another destination.

## Full system proof

The root/systemd/Landlock part must run only in a disposable Ubuntu 24.04 VM.
It refuses to overwrite an existing optid installation and cleans up its
installed binaries, units, runtime override, and temporary proof state.

```sh
bash tools/verify-s5d-shadow.sh \
  --full-system-proof \
  --disposable-vm \
  --install-deps \
  --expected-head "$(git rev-parse HEAD)"
```

In addition to the test-only gates, this verifies:

- packaged and mkosi apply/recovery unit parity;
- the root-only one-shot circuit-clear production path;
- denial of the same clear operation to an unprivileged user;
- absence of daemon-loop state after the one-shot command;
- the live capability-sealing and Landlock probe;
- exact status-75 supervised cold rebuild behavior; and
- suppression of restart for a non-75 failure.

The evidence bundle records the kernel release, Landlock ABI when the full proof
runs, OS metadata, exact commits, timestamps, logs, and file digests.

## After GitHub Actions recovers

PR #393 remains the official verifier. Before allowing it to run, recheck that
`main` and the verifier head still match their declared SHAs. A successful
shadow run can identify failures early, but it cannot substitute for the
repository's GitHub-native immutable artifact and receipt transition under the
current policy.
