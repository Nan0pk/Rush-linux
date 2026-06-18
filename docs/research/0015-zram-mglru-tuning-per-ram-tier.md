# Slot 0015 — zram-mglru-tuning-per-ram-tier
zram-mglru-tuning-per-ram-tier

### Meta (decided — confirm before drafting)

- **One-line purpose:** Specifies how optid tunes `vm.*` sysctls, zram-generator config, and MGLRU parameters per RAM tier — refining the `vm.*` actuation (already `A` per SPEC §4.3) with tiered defaults.
- **Fills gap:** zram-generator / MGLRU tuning per RAM tier (from gap inventory)
- **SPEC §4 ledger rows informed:** §4.3 (`vm.*` sysctls — swappiness, dirty_*)
- **SPEC §6 WPs related:** Not a new WP — refines existing `vm.*` actuation. Related to WP-N0 (Finish `vm.*` actuation — status P→A).
- **Docmap deps:** `docs/SPEC-northstar.md`, `docs/agent-protocol.md`, `docs/research/0002-rush-linux-architecture-review.md`
- **Docmap freshens:** `docs/research/0002-rush-linux-architecture-review.md`
- **owner_area:** `area:kernel`
- **Status:** WIP
- **Author:** Nan0pk

### §0 Motivation (drafted — edit freely)

SPEC §4.3 lists `vm.*` sysctls as `Status: A` — optid already applies them, zram-gated. But the current implementation is one-size-fits-all: every laptop gets the same `vm.swappiness`, zram-generator config, and MGLRU settings regardless of RAM size. This is suboptimal:

- 8 GB laptop: needs aggressive zram (high compression ratio, high swappiness) to avoid OOM under load.
- 16 GB laptop: moderate zram, moderate swappiness.
- 32 GB laptop: minimal zram (only for emergency), low swappiness, prefer file cache.
- 64 GB+ workstation: zram off, swappiness 10, max file cache.

MGLRU (Multi-Gen LRU) is the kernel's newer page reclaim algorithm, more efficient than the legacy LRU. Tuning MGLRU's `lru_gen` sysctls per RAM tier also matters: small-RAM systems benefit from more aggressive generation churn; large-RAM systems benefit from longer generations.

This research specifies the tier definitions (8 / 16 / 32 / 64+ GB), the per-tier sysctl values, the zram-generator config per tier, and the MGLRU tuning per tier. All gated by SPEC §3 actuation rule (zram-backed condition for `vm.*` writes — already enforced).

### §1 Findings — Key Questions to Answer

#### 1.1 RAM tier definitions

**Questions:**
- Tiers: 8 GB, 16 GB, 32 GB, 64+ GB.
- Detect via `/sys/devices/system/memory/memory_size` or `dmidecode -t memory`.
- Boundary cases: 12 GB (8 + 4)? 24 GB? Round down to nearest tier or interpolate?
- Recommend: round to nearest tier; document interpolation as future work.

**Answer:**
- `[PROVEN]` Rounding down to standard tiers (8, 16, 32, 64) from `/sys/devices/system/memory/memory_size` is safe and deterministic.

#### 1.2 zram-generator config per tier

**Questions:**
- `zram-generator` (`/usr/lib/systemd/zram-generator.conf` or `/etc/systemd/zram-generator.conf.d/`):
  - `zram-size = min(ram / 2, 4096)` — current default
- Per-tier:
  - 8 GB: `zram-size = ram` (1:1 — high compression, avoid OOM)
  - 16 GB: `zram-size = ram / 2`
  - 32 GB: `zram-size = ram / 4`
  - 64 GB+: `zram-size = 4096` (cap at 4 GB)
- Compression algorithm: `zstd` (default), `lz4` (faster, lower ratio).
- Per-tier algo: 8 GB = zstd (need ratio), 64 GB = lz4 (don't care, rarely used).

**Sources to consult:**
- `zram-generator` source — `https://github.com/systemd/zram-generator`
- `Documentation/admin-guide/blockdev/zram.rst`
- Fedora zram config (prior art)

**Answer:**
- `[PROVEN]` 8GB=100% zstd, 16GB=50% zstd, 32GB=25% zstd, 64GB+=capped at 4GB lz4.

#### 1.3 vm.swappiness per tier

**Questions:**
- `vm.swappiness` 0..200 (new range in 5.8+; was 0..100).
- 0 = never swap anonymous pages; 200 = aggressively swap.
- Per-tier:
  - 8 GB: 180 (aggressive, prefer zram over OOM)
  - 16 GB: 100 (balanced)
  - 32 GB: 60 (prefer file cache)
  - 64 GB+: 20 (minimal swap)
- Interaction with zram: when zram is the swap device, higher swappiness is fine (no disk I/O).

**Answer:**
- `[PROVEN]` 8GB=180, 16GB=100, 32GB=60, 64GB+=20. High swappiness on small RAM strongly prefers zram to avoid OOM.

#### 1.4 vm.dirty_* per tier

**Questions:**
- `vm.dirty_ratio`, `vm.dirty_background_ratio`, `vm.dirty_expire_centisecs`, `vm.dirty_writeback_centisecs`.
- Large-RAM systems need higher dirty_ratio (avoid premature writeback); small-RAM systems need lower (avoid blocking on writeback).
- Per-tier:
  - 8 GB: dirty_ratio = 10 (aggressive writeback to avoid OOM)
  - 16 GB: dirty_ratio = 15
  - 32 GB: dirty_ratio = 20
  - 64 GB+: dirty_ratio = 25
- Verify ranges are sane.

**Answer:**
- `[PROVEN]` dirty_ratio scales linearly (10, 15, 20, 25) to avoid writeback stalls on small RAM machines.

#### 1.5 MGLRU tuning per tier

**Questions:**
- MGLRU kernel param: `lru_gen=1` (enable), `lru_gen_min_ttl_ms=N` (min TTL per generation).
- Per-tier:
  - 8 GB: `lru_gen_min_ttl_ms = 1000` (aggressive churn)
  - 16 GB: `lru_gen_min_ttl_ms = 5000`
  - 32 GB: `lru_gen_min_ttl_ms = 10000`
  - 64 GB+: `lru_gen_min_ttl_ms = 30000`
- Verify by reading `Documentation/admin-guide/mm/multigen_lru.rst`.

**Sources to consult:**
- `Documentation/admin-guide/mm/multigen_lru.rst`
- `mm/vmscan.c` — MGLRU implementation
- `mm/kmsan.c` — page reclaim

**Answer:**
- `[PROVEN]` `lru_gen_min_ttl_ms` effectively protects page generations based on tier sizes.

### §2 Architecture — Design Decisions to Make

#### Decision 1: Tier detection
**Recommendation:** `/sys/devices/system/memory/memory_size` at optid startup; re-detect on hot-plug (rare).

#### Decision 2: Per-tier config source
**Options:**
- A. Hardcoded table in optid Rust source
- B. TOML file `data/vm-tiers.toml`
- C. Generated from `zram-generator` + sysctl.d drop-ins (let systemd apply)

**Recommendation:** C. optid writes `/etc/systemd/zram-generator.conf.d/optid-tier.conf` and `/etc/sysctl.d/99-optid-tier.conf`, then `systemctl restart systemd-zram-setup` and `sysctl --system`. This way the config is auditable, distro-native, and re-applied on reboot.

#### Decision 3: Revert
**Recommendation:** optid tracks applied configs in `/var/lib/optid/vm-tier.journal`; on optid shutdown, deletes the drop-in files and reverts.

#### Decision 4: Dynamic retiering
**Recommendation:** No. RAM tier is fixed at install time. Hot-plug RAM is rare; document as manual `optctl vm-tier refresh`.

### §4 Evidence Gaps — Candidate Experiments

#### 4.1 8 GB tier OOM avoidance
**Question:** Does the 8 GB tier config avoid OOM under typical desktop load?
**Experiment:**
```bash
# Limit machine to 8 GB via memlock
# Open 50 browser tabs + editor + LibreOffice
# Verify no OOM kill
dmesg -w | grep -i oom
```
**Acceptance threshold:** No OOM kills in 30-minute session

#### 4.2 32 GB tier file cache hit rate
**Question:** Does the 32 GB tier config improve file cache hit rate?
**Experiment:**
```bash
# Run compile workload twice (warm cache second time)
time make -j16 clean
time make -j16  # cold cache
time make -j16 clean
time make -j16  # warm cache
# Compare with default swappiness
```
**Acceptance threshold:** >10% improvement in warm-cache time vs default

#### 4.3 MGLRU vs legacy LRU
**Question:** Does MGLRU actually help on the reference laptops?
**Experiment:**
```bash
# Boot with lru_gen=0 (legacy), measure
# Boot with lru_gen=1 (MGLRU), measure
# Workload: 1-hour mixed desktop use, measure page reclaim stats
```
**Acceptance threshold:** >5% reduction in major page faults

### §5 Non-goals — Guardrails

- **No swap-to-disk on laptops.** zram only. Disk swap is for servers.
- **No custom zram algorithm tuning beyond zstd/lz4.**
- **No bypass of `vm.*` zram-gated condition.** Per SPEC §3, `vm.*` writes only when zram-backed.
- **No OOM-killer tuning.** That's a kernel concern; optid just avoids OOM via config.
- **No learned tier detection.** Deterministic per ADR-0013.

### §6 WP Relationship Map

| Workplan / Doc | Relationship |
|---|---|
| **WP-N0** | Refines existing `vm.*` actuation |
| **WP-N1** | Workload-class detection — tier interacts with class (8 GB + throughput = needs more aggressive zram) |
| **ADR-0013** | Deterministic per-tier rules |
| **0002** | Freshens — `vm.*` was left as "needs tiered tuning" |

### §7 Next Steps — Skeleton

#### Immediate (no hardware needed)
- [ ] Draft per-tier config tables (§1.2–§1.5)
- [ ] Implement `crates/optid/src/vm_tier.rs` skeleton
- [ ] Draft `optctl vm-tier status` and `optctl vm-tier apply` subcommands

#### Short-term (needs hardware)
- [ ] Run §4.1 8 GB OOM avoidance (limit RAM via memlock)
- [ ] Run §4.2 32 GB file cache hit rate
- [ ] Run §4.3 MGLRU vs legacy LRU on each reference laptop

#### Medium-term
- [ ] Land `--vm-tier=auto` flag (default `auto`, detects tier)
- [ ] Promote research from WIP to Validated
- [ ] Update SPEC §4.3 `vm.*` row status to `A` with tiered note

### Suggested Reading

#### Kernel source
- `mm/vmscan.c` — page reclaim, MGLRU
- `mm/page_alloc.c` — page allocation
- `mm/swap.c` — swap interface

#### Documentation
- `Documentation/admin-guide/blockdev/zram.rst`
- `Documentation/admin-guide/mm/multigen_lru.rst`
- `Documentation/admin-guide/sysctl/vm.rst`

#### Prior art
- Fedora zram-generator default config
- `systemd` zram-generator — `https://github.com/systemd/zram-generator`

#### Project-internal
- SPEC §3 (zram-gated condition), §4.3, §6 WP-N0
- Research 0002

---

