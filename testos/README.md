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

- **Host disk never touched.** testOS boots from USB and runs from RAM. The host's partitions and filesystem are untouched. The only writes are to the USB stick itself (results) and to the test machine's RAM (which clears on reboot).
- **Crash recovery is automatic.** If testOS hangs or panics, a hard reset reboots the machine. Since the host's bootloader was never modified, the machine boots back into the host OS. No bricked machines.
- **One-shot boot.** testOS doesn't install a bootloader. You pick the USB from the boot menu manually. Pull the USB, reboot, and you're back in the host OS.
- **`testos-launcher write` is paranoid.** It refuses to write to a mounted device, refuses to write to anything that looks like the host's root disk, and requires you to type the device name twice to confirm.
- **Esc-to-abort saves partial results.** If you press Esc mid-run, the runner writes a marker, skips remaining tests, writes what it has, and reboots. No lost work.

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

## Quick start

```sh
# 1. Build the testOS image (one-time, ~10 minutes)
cargo build --workspace --release
sudo bash testos/build-testos.sh

# 2. Find your USB stick
lsblk

# 3. Write the image to the USB (will prompt for confirmation)
sudo ./target/release/testos-launcher write /dev/sdX

# 4. Plug the USB into the test machine, reboot, pick USB from the boot menu.
#    testOS will boot, show a menu, run benchmarks, write results, and reboot back.

# 5. After testOS reboots the test machine back to its host OS,
#    unplug the USB and plug it back into your workstation.

# 6. Pull the results into the repo
sudo ./target/release/testos-ingest pull /dev/sdX

# 7. Generate the Markdown summary
./target/release/testos-ingest format

# 8. Commit to the repo
./target/release/testos-ingest commit

# 9. Push
git push
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
