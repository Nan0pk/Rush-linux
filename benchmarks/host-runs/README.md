# Host Benchmarks — Direct-On-Host Path

## TL;DR — forget the USB for your own hardware.

```
sudo bash tools/host-bench.sh --tag victus-$(date +%Y-%m-%d)
```

Run it from a TTY (Ctrl+Alt+F3) on your existing Linux install. Quit browsers and heavy apps first. ~15 minutes. It captures cyclictest latency, PSI, and idle power under **baseline** (distro default governor, no optid) vs **optid --apply**, writes everything to `benchmarks/host-runs/<tag>/`, and prints a quick comparison at the end. optid's revert journal restores sysctl/PM QoS/EPP to pre-run state when it exits.

No USB. No QEMU. No mkosi image. No reboot. No booting into a 500MB environment without your editor or AI tooling.

## Why this path exists (and why you should use it instead of testOS/LiveDev for now)

testOS and the LiveDev USB path are **contributor onramps** — designed for strangers on random Windows laptops to mail you evidence without risking their host disk. They are not the fastest way for *you*, the project owner sitting at a machine with cargo, shell, editor, and network access, to produce the first v0.6 Phase D numbers.

Building a reproducible, air-gapped, signed-boot USB benchmark rig before you have even one number was over-engineering. The rig started eating the project. `host-bench.sh` is the shortcut out of that loop.

You can always go back and clean-room re-verify under testOS later. Right now the blocker is a number, not a perfect dyno.

## Prerequisites

- Rust toolchain (`cargo` in PATH, installed via rustup).
- `cyclictest` from `rt-tests` (Fedora: `sudo dnf install realtime-tests`; Debian/Ubuntu: `sudo apt install rt-tests`).
- `bc` for arithmetic.
- Root (for `/dev/cpu_dma_latency`, EPP writes, `/sys/class/powercap/*`).
- Run from a **TTY, not a desktop terminal** — your X11/Wayland session's compositor and Chrome will skew the numbers if you're logged into GNOME/KDE on the same VT. Switch with Ctrl+Alt+F3, log in as yourself, then sudo.

## What it captures per leg (baseline and optid)

| Metric | What it means | v0.6 criterion it serves |
|---|---|---|
| `cyclictest-max-us` (×5, 30s each) | Worst-case wakeup latency under CPU load | Responsiveness floor |
| `psi-cpu-avg10` (×5 samples) | Kernel CPU pressure stall average | Mixed-load responsiveness |
| `psi-io-avg10` (×5 samples) | Kernel IO pressure stall average | Throughput class |
| `avg_watts` (30s idle window) | Package idle power (RAPL on Intel/AMD, else battery drain) | Battery / energy criterion |

The script also writes `meta.txt` with kernel, CPU, DMI board, battery design capacity, git SHA, and optid version (fixing the two Dragnet-001 defects from the 2026-06-10 Victus sample — the `--version` flag now exists, and tee/ANSI artifacts are avoided by using a subshell `2>&1 | tee` rather than ad-hoc pipes).

## Workflow for closing the v0.6 laptop slot

1. Run `sudo bash tools/host-bench.sh --tag victus-ac-YYYY-MM-DD` **on AC power**.
2. Unplug the charger, wait 10s, run `sudo bash tools/host-bench.sh --tag victus-bat-YYYY-MM-DD --skip-baseline` (baseline under AC already captured; but you want baseline-on-battery too, so just omit `--skip-baseline` and re-run both legs on battery).
3. Inspect `benchmarks/host-runs/<tag>/{baseline,optid}/results.csv`.
4. Copy the strongest run into `release/evidence/host-bench/<date>-victus/{baseline,optid}/` (rename to match the `_TEMPLATE/` layout).
5. Write a `VERDICT.md`: does optid match or beat baseline on cyclictest max, PSI, and watts? Be honest — FAIL is useful too, it tells you which levers aren't earning their keep.
6. Nominate a second machine (desktop, or a friend's laptop) and repeat.
7. Once both slots have VERDICT.md, flip the two v0.6 criteria to `verified = true` in `release/milestones.toml` and commit.

## What this script does NOT do

- It does not drive a synthetic workload like a browser Speedometer or `ninja` build yet. Phase D's formal `mixed-load-001` preset (idle → interactive → throughput → latency-critical → idle, ×5) needs to be wired into `rushbench` as a named preset. The current script uses **synthetic microprobes** (cyclictest + PSI + idle power), which are enough to get a first signal and unblock the rest. The full preset can be layered in later.
- It does not sign results, upload them, or open a PR. That's the LiveDev path's job later.
- It does not test suspend/resume, runtime PM, NVMe APST, PCIe ASPM, display/VRR, or sched_ext. Those come in v0.7 laptop/desktop editions.

## Caveats / honesty

- **This is not a clean-room measurement.** Your host system has whatever daemons, cron jobs, and flatpak apps you run day-to-day. The script records ambient loadavg in `meta.txt`; quit obvious CPU hogs (Chrome, Discord, Steam, IDEs, Docker) before running.
- The baseline leg sets the governor to `balance_performance`/`powersave` on Intel P-State (or `schedutil` on other platforms) — this is the closest you can get to "mainstream default" without rebooting into Ubuntu. Note this in your `VERDICT.md`.
- If `optid --apply` refuses to start because a conflicting daemon (tuned, PPD) is active, the script stops those for the duration and restarts them on exit. If you have other power-management tools (gamemode, etc.), stop them manually first.
- Numbers from this path are honest first-signal evidence for v0.6. When v0.8 Benchmark Lab ships, the public comparable numbers should be re-run under the reproducible testOS image. That's the right time for that rigor — not before you know if optid even moves the needle.
