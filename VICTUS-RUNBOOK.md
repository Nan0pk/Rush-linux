# VICTUS — QUICK START (what to do *right now* from your existing Fedora)

No USB. No QEMU. No reboots. Just this.

## 1. Prep

```bash
# From the repo root on your Victus (in Fedora, your normal user shell):
sudo dnf install -y realtime-tests bc   # cyclictest + bc
# Make sure you have rust via rustup (you do if cargo --version works):
cargo --version || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## 2. Switch to a TTY (get off the GNOME desktop)

Press **Ctrl+Alt+F3**. Log in as your normal user. cd to the repo.

```bash
cd ~/src/Rush-linux   # wherever your clone is
```

## 3. Stop heavy apps from another VT (or from here)

```bash
# Quit chrome, discord, steam, code, etc. Be aggressive — we want quiet.
pkill -f chrome || true
pkill -f discord || true
pkill -f code || true
# Wait 10s for things to settle
sleep 10
```

## 4. Run on AC (plugged in)

```bash
sudo bash tools/host-bench.sh --tag "victus-ac-$(date +%Y-%m-%d)"
```

- It will build optid/optctl/rushbench in release mode (~1–2 min).
- It asks "Ready? (type 'yes' to continue)" — type `yes`.
- It runs baseline (distro defaults, ~6 min), then optid --apply (~6 min).
- When done, it prints a quick comparison table of cyclictest / PSI / watts.
- It restores your system state (stops optid, restarts tuned/PPD if they were running).

**Do not touch the machine** while it's running — especially during the idle energy windows.

## 5. Run on BATTERY (unplug)

Wait for the AC run to finish. Unplug the charger. Wait 10s for the power source to settle. Then:

```bash
sudo bash tools/host-bench.sh --tag "victus-bat-$(date +%Y-%m-%d)"
```

## 6. Look at results

```bash
ls benchmarks/host-runs/
# You'll see two dirs: victus-ac-<date>/ and victus-bat-<date>/

# Quick eyeball (each directory has baseline/ and optid/ subdirs):
cat benchmarks/host-runs/victus-ac-*/baseline/results.csv
cat benchmarks/host-runs/victus-ac-*/optid/results.csv
cat benchmarks/host-runs/victus-bat-*/baseline/results.csv
cat benchmarks/host-runs/victus-bat-*/optid/results.csv
```

What "good" looks like (honest first version):
- **cyclictest max us**: optid should be **<=** baseline (it's enforcing PM QoS floors, not doing magic). Large *increase* under optid = the latency floor is set wrong (likely the 1 ms vs 10 µs correction we already made).
- **psi-cpu / psi-io avg10**: optid should be **<=** baseline (no extra pressure from the daemon itself).
- **avg_watts idle**: on battery, optid idle should be **<=** baseline.

If optid loses on all three, that's still useful — it means the current contracts/actuators aren't earning their keep and you debug from there. **FAIL is data.**

## 7. Switch back to GUI

**Ctrl+Alt+F1** (or F2) gets you back to GDM/GNOME.

## 8. Copy into evidence when you're happy

Pick the strongest AC + battery run and copy into the evidence tree:

```bash
HB=release/evidence/host-bench
DIR="$HB/$(date +%Y-%m-%d)-victus"
mkdir -p "$DIR"
cp benchmarks/host-runs/victus-ac-YYYY-MM-DD/baseline/* "$DIR/"  # combine later
# (Or keep ac and bat as separate subdirs — we'll write VERDICT.md together.)
```

Then come back and paste the four results.csv files to me (or just tell me the numbers) and I'll help you write the VERDICT.md and flip the milestone flags.

**That is it. That is the end-to-end path. One laptop slot of Phase D, ~30 minutes total.**
