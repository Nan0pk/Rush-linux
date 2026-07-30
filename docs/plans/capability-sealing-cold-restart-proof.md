# Capability Sealing and Supervisor-Managed Cold Restart Proof

This document is the verifier-facing procedure for the experimental safety package **Prototype capability sealing and supervisor-managed cold restart**. Its builder implementation is draft pull request **#366**. The prototype is feature-gated, is not installed in the shipped image, and never writes real hardware attributes.

## What the proof must establish

A passing result must demonstrate all of the following on the exact commit being verified:

1. The detected Landlock ABI selects only rights supported by that ABI.
2. `no_new_privs` is set and remains set after `execve`.
3. Descriptors opened before sealing remain usable.
4. New write opens fail after sealing while read opens remain available.
5. A child thread and a fork/exec child inherit the restriction.
6. A genuinely unlinked object is handled through its pre-opened descriptor without reopening the removed path or panicking.
7. `--topology-rebuild` exits with status 75.
8. The test-only systemd unit runs its recovery step before each fresh capability-table construction and forces a restart only for status 75.
9. The first supervised cycle requests a cold restart and the second cycle succeeds only after recovery runs again.

## Automated checks

```sh
cargo test -p optid --features experimental-capability-sealing \
  --bin optid-capability-seal-test
cargo test -p optid --features experimental-capability-sealing \
  --test capability_sealing_cli
cargo build -p optid --features experimental-capability-sealing \
  --bin optid-capability-seal-test
systemd-analyze verify packaging/systemd/optid-capability-seal-test.service
./target/debug/optid-capability-seal-test --probe
```

The pull-request workflow `Capability sealing kernel proof` runs these checks on an Ubuntu 24.04 hosted kernel and uploads `capability-sealing-kernel-proof.log` containing the kernel release, active Linux security modules when visible, detected Landlock ABI, rights mask, and every runtime result.

## Supervised restart proof on a systemd host

```sh
sudo install -D -m 0755 target/debug/optid-capability-seal-test \
  /usr/local/libexec/optid-capability-seal-test
sudo install -D -m 0644 packaging/systemd/optid-capability-seal-test.service \
  /etc/systemd/system/optid-capability-seal-test.service
sudo touch /run/optid-capability-seal-test-enabled
sudo systemctl daemon-reload
sudo systemctl reset-failed optid-capability-seal-test.service
sudo systemctl start optid-capability-seal-test.service
systemctl show optid-capability-seal-test.service \
  -p Result -p ExecMainCode -p ExecMainStatus -p NRestarts
journalctl -u optid-capability-seal-test.service --no-pager
```

The journal must show this order twice:

1. `recovery step completed`;
2. `recovery marker verified before capability discovery`;
3. a complete passing capability-sealing probe.

The first probe must then request status 75. systemd must start a new process, rerun recovery, rebuild and seal a fresh capability table, and finish successfully. A failure other than status 75 must not be converted into a restart loop.

## Cold-verification boundary

The builder may record this package only as `candidate`. A different worker must check the exact implementation commit without repairing it, inspect the workflow artifact and supervised-host output, and commit `docs/plans/optid-verification/d0.toml` before proposing `completed`.

The receipt must name the exact 40-character commit, implementation pull request, commands above, observed kernel release and Landlock ABI, supervisor restart count, and an empty unresolved list. Synthetic and hosted-kernel evidence proves the mechanism; it does not authorize automatic actuation on any physical hardware identity.
