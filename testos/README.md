# testOS — Real-Hardware Benchmark Environment for Rush Linux

testOS is a temporary, self-contained Linux environment that boots from a USB stick, runs the Rush Linux benchmark suite on real hardware, and writes the results back to the USB. After it finishes, you reboot back into the host OS and pull the results into the repo.

It exists because Rush Linux's benchmark manifest declares 5 scenarios (mixed-load, server throughput, laptop battery, gaming, realtime audio) but the project is currently blocked on **Phase D** — no real-hardware benchmark workflow exists. testOS is that workflow.

## What testOS is

- A single bootable USB image (`.raw` file, ~500MB) built from the same mkosi config that builds Rush Linux itself.
- The image contains: the Rush Linux v0.5 server skeleton, the `optid` daemon, the `rushbench` measurement rig, plus the benchmark tools (fio, iperf3, postgres, nginx, ab, cyclictest, jq).
- A Rust binary called `testos-runner` that boots automatically on tty1, shows a menu, runs the selected benchmarks, writes results to the USB, and reboots.

## What testOS is NOT

- Not a new operating system. It's a thin wrapper around the existing Rush Linux image.
- Not a permanent install. Nothing is written to the host machine's disk.
- Not a substitute for the `rushbench` crate. testOS calls `rushbench` for the measurements it already knows how to make; testOS handles the boot/menu/results/ingest workflow that `rushbench` doesn't.

## The end-to-end flow

This is the exact workflow you asked for: a few commands, reboot, benchmark, reboot back, retrieve, format, commit.

```
    Your workstation                Test machine (any x86_64 box)
    ────────────────                ──────────────────────────────
    testos-launcher build
            │
            ▼
    testos-launcher write /dev/sdX
            │
            └────[ plug USB into test machine ]───┐
                                                  ▼
                                          Reboot, pick USB from boot menu
                                                  │
                                                  ▼
                                          testOS boots, shows menu
                                                  │
                                                  ▼
                                          Pick: "Run all" or individual tests
                                                  │
                                                  ▼
                                          Benchmarks run with progress display
                                          (Esc at any time to abort early)
                                                  │
                                                  ▼
                                          Results written to USB
                                                  │
                                                  ▼
                                          Auto-reboot back to host OS
                                                  │
    ┌─────────────────────────────────────────────┘
    ▼
    testos-ingest pull /dev/sdX
            │
            ▼
    testos-ingest format
            │
            ▼
    testos-ingest commit
            │
            ▼
    git push
```

## Why this design (the trade-offs you asked about)

**USB boot (not disk-file boot):**
- Zero risk to the host disk — never touches the host's partitions or filesystem.
- Works on any machine that can boot from USB (including Windows-only machines, since staging happens entirely on your workstation).
- Slightly slower iteration (you re-flash the USB each time) but rock-solid reliability.
- The "no USB" path (boot from a file on the existing disk) was considered and rejected because it writes to the host's EFI partition, violating the "minimal intrusion" rule. It can be added later as an opt-in `--no-usb` flag if iteration speed becomes painful.

**RAM runtime (not USB runtime, not disk runtime):**
- testOS boots from USB, but the rootfs lives in the UKI's initramfs (already in RAM once the kernel loads). The USB is only used for: reading the bench catalog (small TOML file), and writing results.
- This means disk benchmarks (fio, Postgres) hit the test machine's actual disk — exactly what you want to measure.
- The USB can be unplugged after boot if you want. testOS will still run; it just won't be able to save results. (Don't do this unless you have a good reason.)

**Full reboot (not kexec):**
- Every reboot goes through the BIOS. Hardware starts cold. Numbers are fair.
- A `--fast` kexec path is planned but not yet built. It would tag results as "warm-boot" so they don't get mixed with cold-boot numbers.

**New Rust crate `crates/testos/`:**
- Joins the existing `optid/optctl/rushbench/rush_collect` workspace.
- Shares types with `rushbench` (host fingerprint fields match) so results from both rigs can be joined later.
- Consistent with the repo's Rust-first style. Heavier initial lift than a shell-script folder, but pays off the first time you want to refactor the result schema.

## Files added

```
crates/testos/                      New Rust crate (workspace member)
├── Cargo.toml
├── src/
│   ├── lib.rs                      Shared library code
│   ├── catalog.rs                  Editable bench-list types
│   ├── results.rs                  Result schema (frozen at v1)
│   └── host.rs                     Host fingerprinting
├── bin/
│   ├── testos-launcher.rs          Host-side: build + write to USB
│   ├── testos-runner.rs            In-testOS: menu + run + reboot
│   └── testos-ingest.rs            Host-side: pull + format + commit
└── examples/
    └── show_menu.rs                Smoke test — prints the menu

testos/                             Top-level non-code assets
├── bench-list.toml                 The editable catalog of benchmarks
├── build-testos.sh                 Wraps tools/build-mkosi-image.sh for testOS
└── README.md                       This file

mkosi/mkosi.profiles/testos/
└── mkosi.conf                      Profile that adds benchmark tools + testOS units
```

The root `Cargo.toml` workspace `members` list now includes `crates/testos`.

## How to add a new benchmark

Open `testos/bench-list.toml` and add a new `[[benches]]` entry:

```toml
[[benches]]
id = "my-new-test"
name = "My New Test — description"
scenario = "server-throughput"      # or one of the other 4 scenarios
kind = "shell-numeric"              # or shell-json, shell-pass-fail, rushbench
command = "some-shell-command-that-prints-a-number"
estimated_seconds = 30
notes = "Optional notes shown in the menu."
```

Then `testos-launcher build` to bake it into the next USB image. No code changes required.

### Benchmark `kind` reference

| kind | What the runner expects |
|---|---|
| `shell-numeric` | The command's last non-empty line of stdout is the numeric result. Unit defaults to "numeric". |
| `shell-json` | The command writes `{"value": <number>, "unit": "<string>"}` to the file whose path is in `$TESTOS_RESULT_FILE`. |
| `shell-pass-fail` | Exit code is the only signal. 0 = pass, anything else = fail. No value captured. |
| `rushbench` | Calls `rushbench run ...` and parses the median from stdout. Full integration with rushbench's JSON output is a future enhancement. |

## The current benchmark catalog

9 benchmarks, total estimated runtime **3 minutes 40 seconds** on a typical machine. Covers two of the five manifest scenarios:

| Scenario | Benchmarks |
|---|---|
| `server-throughput` | fio seq read IOPS, fio seq write IOPS, iperf3 TCP throughput, PostgreSQL pgbench TPS, nginx RPS |
| `mixed-load-responsiveness` | PSI CPU avg10, PSI IO avg10, cyclictest max latency, foreground launch latency |

The remaining three scenarios (`laptop-battery`, `gaming-frame-time`, `realtime-audio`) are declared in the manifest but not yet wired into testOS — they need special hardware per machine. Add them as `[[benches]]` entries when hardware becomes available.

## Safety guarantees

- **Host disk never touched.** testOS boots from USB and runs from RAM. The host's partitions and filesystem are untouched. The only writes are to the USB stick itself (results + private diagnostics) and to the test machine's RAM (which clears on reboot).
- **Crash recovery is automatic.** If testOS hangs or panics, a hard reset reboots the machine. Since the host's bootloader was never modified, the machine boots back into the host OS. No bricked machines.
- **One-shot boot.** testOS doesn't install a bootloader. You pick the USB from the boot menu manually. Pull the USB, reboot, and you're back in the host OS.
- **`testos-launcher write` is paranoid.** It refuses to write to a mounted device, refuses to write to anything that looks like the host's root disk, and requires you to type the device name twice to confirm.
- **Esc-to-abort saves partial results.** If you press Esc mid-run, the runner writes a marker, skips remaining tests, writes what it has, and reboots. No lost work.
- **No root shell on failure (boot-reliability PR).** The runner used to drop to an interactive root shell on failure. It no longer does. On any uncorrectable failure it shows a privacy-safe recovery screen with a short failure code (E001–E101), a one-sentence description, a safe next action, and a 10-second reboot countdown. Raw diagnostics are written to `PRIVATE-DIAGNOSTICS/<run_id>/` on the USB for local review (see "Private local diagnostics" below).
- **Baseline purity.** The testOS baseline image does NOT enable `optid`, `optid-apply`, or `optid-boot-assess` as persistent services, and never runs `optid --apply`. The optid binaries and unit files are installed on disk for ad-hoc operator use, but they are not started automatically. A testOS boot measures the hardware as-is, without optid actuation or boot-assessment side effects.
- **Cloud-safe run-intent contract.** A physical testOS run is cryptographically associated with the host planner that launched it via a `run-intent.json` file (`schemas/testos-run-intent.schema.json`). The runner refuses to run if the intent is missing/malformed/stale/dry-run/inconsistent, and copies every field into `manifest.json` under a `provenance` block. The strict evidence validator (`tools/validate-testos-evidence.py`) re-checks every provenance field before an evidence PR may be opened. See `docs/livedev/OPERATOR_RUNBOOK.md` for the full contract and the remaining Windows-only work.

## Boot reliability (boot-reliability PR)

Previous real-hardware runs on the HP Victus exhibited two symptoms:

1. Early boot displayed ACPI/ACPI-table warnings.
2. The first boot stopped at something resembling a root shell; only after
   typing `reboot` did the next boot reach the testOS benchmark screen.

The boot-reliability PR addresses the root causes without hiding the
symptoms:

- **Bounded USB-discovery retry.** The `testos-usb-mount` helper now retries
  for a bounded window (default 30 s, clamped to [5, 300] s) using
  `udevadm settle` between attempts. There is no unbounded sleep. If the
  USB does not appear within the window, the helper exits non-zero and
  writes a discovery timeline to `/run/testos/usb-discovery-timeline.txt`
  (copied into `PRIVATE-DIAGNOSTICS/` for local post-mortem).
- **Runner Requires= the mount.** `testos-runner.service` now has
  `Requires=testos-usb-mount.service` (not just `Wants=`). A mount failure
  prevents the runner from starting, so it cannot race onto tty1 with an
  unmounted USB and fall through to a login prompt.
- **Getty race eliminated.** `getty@tty1.service` is masked in the image,
  and `testos-runner.service` declares `Conflicts=getty@tty1.service` as
  defense-in-depth. The runner owns tty1 exclusively.
- **Bounded startup + restart policy.** Both units have bounded
  `TimeoutStartSec=`. The runner restarts at most once on failure.
- **ACPI honesty.** testOS does NOT suppress ACPI output or add kernel
  suppression flags for aesthetics. The runner prints an operator-facing
  note explaining that firmware ACPI warnings are usually benign if boot
  continues. A boot-blocking ACPI failure is reported via the recovery
  screen with failure code `E101` (distinct from benign warnings).

### Recovery screen failure codes

| Code | Category | Meaning |
|------|----------|---------|
| E001 | USB not found | The USB partition (label RUSHESP) was not found within the retry window. |
| E002 | USB mount failed | The USB partition was found but could not be mounted. |
| E003 | intent unavailable | `run-intent.json` is missing, malformed, stale, or inconsistent. |
| E004 | plan unavailable | `plan.json` is missing or its hash does not match the intent. |
| E005 | catalog unavailable | The bench-list catalog is missing or its hash does not match the intent. |
| E006 | image version mismatch | The running testOS image version does not match the intent's `testos_version`. |
| E099 | runner internal error | The runner hit an internal error while executing the benchmark plan. |
| E101 | ACPI blocking | ACPI reported a boot-blocking failure. (Benign ACPI warnings do not trigger this.) |

## Private local diagnostics (boot-reliability PR)

Raw boot diagnostics are written ONLY to:

```
/run/testos/usb/PRIVATE-DIAGNOSTICS/<run_id>/
```

They are NEVER placed under `testos-results/`, the persistent evidence run
directory, or any submission bundle. Every `PRIVATE-DIAGNOSTICS/` directory
is marked with a `README.txt` containing:

```
PRIVATE — MAY CONTAIN HARDWARE IDENTIFIERS — DO NOT SUBMIT
```

What we capture (when available):

- `journalctl.txt` — `journalctl -b` with monotonic timestamps
- `dmesg.txt` — `dmesg` with monotonic timestamps
- `systemctl-failed.txt` — `systemctl --failed`
- `status-usb-mount.txt` / `status-runner.txt` — `systemctl status` for the two testOS units
- `critical-chain.txt` / `blame.txt` — `systemd-analyze` output
- `usb-discovery-timeline.txt` — the mount helper's retry timeline
- `runner-exit.txt` — runner exit status, failure category, boot attempt number
- `kernel-version.txt` / `image-version.txt` — testOS version, full image commit, kernel version

What we DO NOT capture: firmware tables, disk contents, user data,
authentication material, network credentials, or file contents unrelated
to boot diagnosis.

### Reviewing private diagnostics locally

```sh
# Read-only inspection (default safe action):
python3 tools/testos-diagnostics.py inspect /run/testos/usb/PRIVATE-DIAGNOSTICS/<run_id>

# Copy raw diagnostics to another location (prints a PRIVACY WARNING, requires --yes):
python3 tools/testos-diagnostics.py export /run/testos/usb/PRIVATE-DIAGNOSTICS/<run_id> /tmp/diag-copy --yes

# Create a sanitized copy with hardware identifiers redacted (never modifies the original):
python3 tools/testos-diagnostics.py sanitize /run/testos/usb/PRIVATE-DIAGNOSTICS/<run_id> /tmp/diag-sanitized
```

`inspect` is read-only by default. `export` requires an explicit destination
and `--yes`. `sanitize` creates a NEW reviewed copy; the sanitized output
must still pass the normal privacy scanner
(`tools/validate-testos-evidence.py`) before it may be included in any
evidence bundle.

### Privacy boundary enforcement

The strict evidence validator (`tools/validate-testos-evidence.py`) fails
closed if:

- `PRIVATE-DIAGNOSTICS` appears inside the proposed bundle (at any depth)
- any raw `dmesg.txt` / `journalctl.txt` / etc. artifact appears inside
  publishable evidence
- a symlink inside the bundle tries to reference private diagnostics (or
  anything outside the bundle — symlinks are forbidden in publishable
  evidence entirely)

Normal resume/collection leaves `PRIVATE-DIAGNOSTICS/` on the USB.

## What to photograph or transcribe if a boot failure recurs

If the HP Victus (or any other test machine) again stalls at a root prompt
or shows an ACPI error, capture:

1. **The recovery screen** — photograph the whole screen, especially the
   `Failure code:` line (E001–E101) and the `Category:` line.
2. **The boot-attempt number** — printed in the testOS banner as
   `attempt: N`.
3. **The source SHA** — printed in the banner as `source: <sha>`. Verify
   it matches `git rev-parse --short HEAD` in your repo.
4. **Any ACPI messages visible before the recovery screen** — transcribe
   the first few lines verbatim (they are not captured automatically
   unless boot reaches the runner).
5. **The USB discovery timeline** — after rebooting back to the host OS,
   run `python3 tools/testos-diagnostics.py inspect <USB>/PRIVATE-DIAGNOSTICS/<run_id>`
   and transcribe the `usb-discovery-timeline.txt` contents.

Do NOT photograph or transcribe: full dmesg dumps, MAC addresses, serial
numbers, UUIDs, hostnames, IP addresses, or the kernel command line.
Those stay in `PRIVATE-DIAGNOSTICS/` for local review only.

## Prerequisites

On your workstation (where you build and write the USB):

- Arch Linux (or a distro with `mkosi` available — `pacman -S mkosi` on Arch, or see `tools/env-setup.sh` for other distros)
- Rust toolchain (`pacman -S rust` or `rustup`)
- `archlinux-keyring` (for mkosi to verify packages)
- Root/sudo for `dd` and `mount`

On the test machine:

- Any x86_64 machine that can boot from USB
- No OS prerequisites — testOS is self-contained
- At least 1GB RAM (testOS runs from RAM)
- A USB port

## Quick start — Linux one-command flow

```sh
# Set your token (needs repo scope):
export GITHUB_TOKEN="github_pat_xxx..."

# Write the USB (auto-detects the USB stick; prompts before destructive write):
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/install.sh | sudo bash

# Boot testOS on the test machine, run benchmarks, let it reboot, then plug
# the USB back into this Linux workstation.

# Collect results, push a branch, and open a PR for maintainer review:
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/collect-results.sh | sudo bash
```

If your sudo configuration strips `GITHUB_TOKEN`, preserve the named variable:

```sh
curl -fsSL https://raw.githubusercontent.com/Nan0pk/Rush-linux/main/testos/collect-results.sh | sudo --preserve-env=GITHUB_TOKEN bash
```

## Manual developer flow

```sh
# 1. Build the testOS image (one-time, ~10 minutes)
cargo build --workspace --release
sudo bash testos/build-testos.sh

# 2. Write the image to the USB (will auto-detect if /dev/sdX is omitted)
sudo bash testos/install.sh

# 3. Plug the USB into the test machine, reboot, pick USB from the boot menu.
#    testOS will boot, show a menu, run benchmarks, write results, and reboot back.

# 4. After testOS reboots the test machine back to its host OS,
#    unplug the USB and plug it back into your workstation.

# 5. Collect, push, and open a PR. The script never merges it.
sudo --preserve-env=GITHUB_TOKEN bash testos/collect-results.sh
```

## What's NOT yet built (known limitations)

- **The `--fast` kexec flag** is declared in the design but not implemented. All reboots are full cold reboots today.
- **Battery, gaming, and realtime-audio scenarios** are declared in the manifest but have no `[[benches]]` entries yet. They need hardware-specific tools (Vulkan, PipeWire, battery sysfs) and may need additional packages in `mkosi/mkosi.profiles/testos/mkosi.conf`.
- **Phoronix Test Suite integration** is not built. It would be added as a new `kind = "phoronix"` in the catalog, plus the `phoronix-test-suite` package in the mkosi profile.
- **The `testos-launcher preview` command** is implemented but only reads the bench list from the USB. A fuller version would also show free space, the testOS version, and the last run's summary.
- **Secure Boot** is not handled. The testOS UKI is unsigned. Disable Secure Boot on the test machine, or sign the UKI as part of the build (future work).
- **The Esc watcher** reads `/dev/console` directly. This works on real hardware but may not work in some VM environments. Ctrl-C is always available as a fallback.

## Design references in the repo

- `benchmarks/manifest.toml` — the 5 declared scenarios testOS aims to cover.
- `docs/testing-and-benchmarks.md` — the testing strategy doc.
- `docs/decisions/0011-benchmark-methodology.md` — the methodology ADR (status: proposed).
- `crates/rushbench/` — the existing measurement rig. testOS calls into it for `kind = "rushbench"` entries.
- `release/test-tiers.toml` — T4 (comparative benchmarks) is the release gate testOS helps unblock.
- `ROADMAP.md` — Phase D (hardware-aware optid) is the milestone this work advances.
