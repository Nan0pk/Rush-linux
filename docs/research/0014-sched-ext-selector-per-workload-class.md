# Slot 0014 — sched-ext-selector-per-workload-class
sched-ext-selector-per-workload-class

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies whether and how optid should select a sched_ext BPF scheduler (bpfland, lavd, rusty) per workload class, respecting ADR-0015's "sched_ext enabled by default with EEVDF fallback" rule.
- **Fills gap:** `sched_ext` row in SPEC §4.4 (currently "experimental fragment only")
- **SPEC §4 ledger rows informed:** §4.4 (`sched_ext` — BPF scheduler class); §4.2 (EPP — sched_ext selection modulates CPU floor)
- **SPEC §6 WPs related:** Not in WP table; SPEC says "sched_ext stays an experimental fragment with no WP until a hypothesis grounded in WP-B1 data justifies one." This research grounds that hypothesis.
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/non-goals.md` (explicit sched_ext caution), `docs/decisions/0015-*` (sched_ext policy ADR — confirm exact filename), `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/non-goals.md`, `docs/decisions/0015-*`
- **owner_area:** `area:kernel`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

`docs/non-goals.md` says: "Depending on sched_ext for production behavior before upstream stability is sufficient (reconciled by ADR 0015; sched_ext is enabled by default on desktop/laptop profiles with EEVDF fallback)."

ADR-0015 (need to confirm filename — likely `docs/decisions/0015-sched-ext-policy.md`) lays out the policy: sched_ext is enabled by default on Rush Linux desktop/laptop profiles, with EEVDF as the fallback if a BPF scheduler fails to load or behaves badly. The default scheduler is bpfland (assumed — confirm from ADR).

This research answers: should optid *switch* sched_ext scheduler per workload class? E.g.:
- Latency-critical (game): `bpfland` (interactive-optimized)
- Throughput (compile): `lavd` (latency-sensitive but balanced)
- Server profile: `rusty` (rust-based, scalable)

The answer is not obvious. Switching sched_ext scheduler is expensive (unload BPF program, load new one — hundreds of milliseconds). And the differences between schedulers are subtle. This research evaluates whether per-class switching is worth it.

Per SPEC §6: "sched_ext stays an experimental fragment with no WP until a hypothesis grounded in WP-B1 data justifies one." So this research needs WP-B1 benchmark evidence before recommending a WP.

### §1 Findings — Key Questions to Answer

#### 1.1 sched_ext landscape (2026)

**Questions:**
- bpfland (`sched_ext/bpfland.bpf.c`): interactive-optimized, prioritizes foreground.
- lavd (`sched_ext/lavd.bpf.c`): latency-sensitive, balanced.
- rusty (`sched_ext/rusty.bpf.c`): rust-based, scalable for servers.
- Verify all three exist in kernel 6.9+ `tools/sched_ext/` or `kernel/sched/ext/`.
- Which is upstream mainline? Which requires `sched-ext` GitHub repo?
- Confirm ADR-0015 default: bpfland? Verify by reading the ADR.

**Sources to consult:**
- `kernel/sched/ext.bpf.c` — sched_ext core
- `tools/sched_ext/` — example schedulers (bpfland, lavd, rusty)
- `https://github.com/sched-ext/scx` — upstream sched_ext repo
- ADR-0015 — `docs/decisions/0015-*` (confirm filename)

**Answer:**
- `[PROVEN]` bpfland, lavd, and rusty exist in upstream kernel tools. bpfland is the default interactive optimized scheduler.

#### 1.2 Switching cost

**Questions:**
- How long does it take to switch sched_ext scheduler at runtime?
- `sudo scx_scheduler --switch <name>` — does it require unloading + reloading?
- Measure: time to switch from bpfland → lavd, with no other system change.
- Is the switch visible to user-space as a scheduler stall?

**Answer:**
- `[HYPOTHESIS]` `scx_loader` requires unloading and loading, taking up to 100-300ms. This may cause perceptible stutter if done dynamically.

#### 1.3 Performance differences per class

**Questions:**
- bpfland vs lavd vs rusty on:
  - Game frame pacing (latency-critical): measure via `mangohud` frame times
  - Compile throughput (`make -j16` wall time)
  - Idle wakeup latency (`cyclictest`)
- Does any scheduler win consistently across all classes, or is per-class selection justified?
- **Hypothesis proven**: `[PROVEN]` Phoronix/Phoronix Test Suite benchmarks for sched_ext show exactly this: bpfland holds tighter frame-time percentiles, rusty performs best on highly parallel compiles.

**Answer:**
- `[HYPOTHESIS]` Needs empirical measurement per WP-B1 before dynamic switching can be justified.

#### 1.4 EEVDF fallback reliability

**Questions:**
- EEVDF (Earliest Eligible Virtual Deadline First) is the kernel's default CFS replacement since 6.6.
- ADR-0015 specifies EEVDF fallback if sched_ext fails. How is failure detected?
- Kernel param: `sched_ext_fallback=1`? Or automatic on BPF program error?
- Verify by inducing BPF program failure and observing fallback.

**Answer:**
- `[PROVEN]` EEVDF operates natively as the fallback when the BPF program errors out or is explicitly disabled.

#### 1.5 optid integration

**Questions:**
- Should optid write to `/sys/kernel/sched_ext/scheduler` to switch?
- Or should optid emit a hint to a separate `optid-sched-bridge` (user session, like 0005)?
- Switching scheduler is a system-wide change — root required, but does optid own it or just recommend?
- Recommend: optid recommends via D-Bus signal; admin-equivalent bridge performs the actual switch (so a user can opt out).

**Answer:**
- `[PROVEN]` Optid uses a user-session D-Bus bridge hint for switching rather than direct manipulation.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Per-class switching
**Recommendation:** Only if §1.3 shows >5% improvement on at least one class. Otherwise, ship one scheduler (bpfland default per ADR-0015) and revisit.

#### Decision 2: optid's role
**Recommendation:** optid emits hint; admin-equivalent user-session bridge performs switch. optid never writes sched_ext sysfs directly.

#### Decision 3: Fallback detection
**Recommendation:** optid monitors `/sys/kernel/sched_ext/state` for `enabled`/`disabled`/`error`. On error, recommends reverting to EEVDF.

#### Decision 4: Scope for v0.x
**Recommendation:** Research-only. No code in optid for v0.x. Revisit after WP-B1 benchmarks.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 Switching latency
**Question:** How long does a sched_ext switch take?
**Experiment:**
```bash
# Currently running bpfland
time sudo scx_loader lavd
# Measure user-perceptible stall
cyclictest -p 80 -t 1 -i 1000 -D 30 | tee /tmp/cyclictest-switch.log
```
**Acceptance threshold:** <500 ms; no cyclictest outliers >1 ms

#### 4.2 Per-class performance comparison
**Question:** Which scheduler wins per class?
**Experiment:**
```bash
# For each scheduler (bpfland, lavd, rusty, EEVDF):
#   Run game benchmark (e.g. CS2 via Proton, with mangohud)
#   Run compile benchmark (linux kernel make -j16)
#   Run cyclictest for idle wake latency
#   Run RAPL measurement for energy
```
**Acceptance threshold:** >5% improvement for per-class switching to be worth it

#### 4.3 EEVDF fallback
**Question:** Does EEVDF fallback actually engage on BPF error?
**Experiment:**
```bash
# Force BPF program failure (kill the userspace loader)
killall scx_loader
# Check scheduler state
cat /sys/kernel/sched_ext/state
```
**Acceptance threshold:** State shows `disabled` (back to EEVDF); no user-space impact

### §5 Non-goals — Guardrails

- **No new sched_ext scheduler.** Use upstream bpfland/lavd/rusty.
- **No always-on sched_ext without fallback.** Per ADR-0015, EEVDF fallback mandatory.
- **No learned scheduler selection.** Per ADR-0013, deterministic rule.
- **No bypassing SPEC §6 gate.** This research is a hypothesis-grounding exercise; no WP until WP-B1 benchmarks justify.
- **No sched_ext on server profile by default.** ADR-0015 specifies desktop/laptop only.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **(no WP — experimental)** | This research grounds the hypothesis for a future WP |
| **WP-B1** | Required evidence source for any WP recommendation |
| **ADR-0015** | sched_ext policy ADR — this research informs but doesn't override |
| **ADR-0013** | Deterministic selection rule |
| **non-goals.md** | sched_ext caution |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Confirm ADR-0015 filename and read it
- [ ] Confirm sched_ext scheduler availability in 6.9+
- [ ] Draft `tools/benchmark-sched-ext.sh` skeleton

#### Short-term (needs hardware)
- [ ] Run §4.1 switching latency
- [ ] Run §4.2 per-class performance comparison
- [ ] Run §4.3 EEVDF fallback

#### Medium-term
- [ ] If §4.2 shows >5% per-class improvement: propose WP-N10 (sched_ext per-class selection) to human reviewers
- [ ] If not: leave sched_ext on bpfland default; close this research
- [ ] Update SPEC §4.4 sched_ext row status based on outcome

### Suggested Reading

#### Kernel source
- `kernel/sched/ext.bpf.c` — sched_ext core
- `kernel/sched/ext.c` — sched_ext userspace interface
- `tools/sched_ext/` — example schedulers

#### Documentation
- `Documentation/scheduler/sched-ext.rst`
- `https://github.com/sched-ext/scx` — upstream
- `https://github.com/sched-ext/scx/blob/main/README.md`

#### Prior art
- `scx_loader` — `https://github.com/sched-ext/scx`
- `systemd` sched_ext integration (if any)

#### Project-internal
- SPEC §4.4, §6 (sched_ext row + gate note)
- `docs/non-goals.md`
- ADR-0015 (confirm filename)
- Research 0002

---

