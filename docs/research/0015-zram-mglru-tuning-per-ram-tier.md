# 0015 — zram and MGLRU Tuning per RAM Tier

*This document is a RESEARCH BRIEF — findings are tagged [PROVEN] (reproducible evidence) or
[HYPOTHESIS] (design inference, needs empirical confirmation). Do not ship production code based
solely on [HYPOTHESIS] findings without running the acceptance experiments in §4.*

**Status:** WIP
**Author:** Claude (research synthesis)
**Date:** 2026-06-19
**Depends:** docs/SPEC-northstar.md
**Code:** crates/optid/src/actuators/memory.rs, crates/optid/src/sensors/memory.rs

* * *

## 0. Motivation

Memory pressure forces paging to storage, which increases latency, raises storage power
(activating NVMe from PS3), and degrades user experience. Two kernel mechanisms address
this on battery-constrained laptops:

1. **zram** — a compressed RAM-backed block device used as swap; compression reduces
   effective swap I/O by 2–4× and keeps paging within DRAM rather than NVMe, saving
   storage power and reducing latency.
2. **MGLRU** (Multi-Generation LRU, kernel ≥ 6.1) — a new page reclaim algorithm that
   better distinguishes hot from cold pages, reducing unnecessary reclaim and improving
   hit rate in the page cache.

The optimal tuning of these mechanisms depends on total RAM: a 4 GB system should use
aggressive zram (e.g., 75 % compression target) to avoid NVMe swap; a 32 GB system can
afford a larger page cache and less aggressive zram since memory pressure is unlikely.

Research questions: How is zram configured from userspace? What MGLRU sysfs knobs exist?
What are the right parameters per RAM tier? How does optid interact with systemd-zram-setup?

* * *

## 1. Findings

### 1.1 zram Configuration

**Q: How is zram configured and what parameters does optid control?**

zram devices are created via the `zram` kernel module and configured via sysfs [PROVEN —
`Documentation/admin-guide/blockdev/zram.rst`]:

```bash
# Create zram device
echo 1 > /sys/class/zram-control/hot_add    # returns device number N

# Configure before first use
echo lz4 > /sys/block/zramN/comp_algorithm  # compression algorithm
echo 4G  > /sys/block/zramN/disksize        # logical maximum size
echo 2   > /sys/block/zramN/max_comp_streams # parallel compression threads

# Format and add as swap
mkswap /dev/zramN
swapon -p 100 /dev/zramN                    # priority 100 > default swap
```

**Compression algorithms** [PROVEN — `lib/crypto/` in kernel]:
- `lzo-rle` — fast, ~2× compression; available everywhere; default for many distros
- `lz4` — faster than lzo-rle at similar ratio; preferred for low-latency systems
- `zstd` — better ratio (2.5–3×) at slightly higher CPU cost; best for memory-constrained
  systems where compression ratio matters more than CPU time
- `lz4hc` — high-compression lz4 variant; slower compress, fast decompress

**optid recommendation by RAM tier** [HYPOTHESIS]:

| Total RAM | comp_algorithm | disksize | Priority |
|-----------|---------------|----------|----------|
| ≤ 4 GB    | `zstd`        | RAM × 1.0 | 200 (highest) |
| 8 GB      | `lz4`         | RAM × 0.75 | 100 |
| 16 GB     | `lz4`         | RAM × 0.5  | 100 |
| ≥ 32 GB   | `lz4`         | RAM × 0.25 | 50 (low; prefer no swap) |

For ≥ 32 GB systems, zram provides a safety net for memory spikes but should not be the
primary swap target — large in-memory page caches are more valuable.

**Interaction with systemd-zram-setup** [PROVEN — systemd ≥ 253]:

systemd ships a `systemd-zram-setup@zram0.service` that creates a zram swap device using
`/etc/systemd/zram-generator.conf`. optid should detect whether this service is active and:
- If active: do NOT create a separate zram device; instead adjust `max_comp_streams` and
  `comp_algorithm` on the existing device if the RAM tier warrants it [HYPOTHESIS —
  live algorithm change requires resetting the device; may not be practical without service restart]
- If inactive: optid creates and manages its own zram device at startup

**Recommended approach**: Rush Linux ships a `zram-generator.conf` tuned per RAM tier
(via the mkosi build) and leaves runtime management to `systemd-zram-setup`; optid's
role is auditing the configuration and reporting suboptimal settings via telemetry.

### 1.2 MGLRU Configuration

**Q: What MGLRU sysfs knobs exist and what do they control?**

MGLRU is enabled by default in kernel 6.1+ when compiled with `CONFIG_LRU_GEN=y` [PROVEN]:

```
/sys/kernel/mm/lru_gen/
├── enabled        # bitmask: 0x0001=MGLRU, 0x0002=mm_walk, 0x0004=page_table_aging
├── min_ttl_ms     # minimum time (ms) a generation must age before being evicted
└── /sys/kernel/mm/lru_gen/debug/  # (optional, with CONFIG_LRU_GEN_STATS)
```

**`enabled` bitmask** [PROVEN — `mm/vmscan.c`, commit introducing LRU_GEN]:
- Bit 0 (`0x01`): Enable MGLRU page eviction policy. Default: 1.
- Bit 1 (`0x02`): Enable memory-mapped file walk (scans page tables for access bits).
  Default: 1. CPU-intensive on large address spaces; disable on low-RAM systems to
  save CPU [HYPOTHESIS — disable mm_walk saves ~0.5 % CPU on 4 GB system under pressure].
- Bit 2 (`0x04`): Enable page-table-based aging. Works alongside bit 1. Default: 1.

**`min_ttl_ms`** [PROVEN — kernel documentation]:
- Default: 0 (no minimum; reclaim immediately if needed)
- Setting to 1000 ms means pages survive at least 1 s before being evicted; reduces
  excessive reclaim oscillation under moderate pressure [HYPOTHESIS — THP-style
  hysteresis; value needs calibration per workload]
- Setting too high delays reclaim → OOM risk on low-RAM systems

**optid tuning per RAM tier** [HYPOTHESIS]:

| Total RAM | `enabled` | `min_ttl_ms` | Rationale |
|-----------|-----------|-------------|-----------|
| ≤ 4 GB    | `0x01` (MGLRU only; no mm_walk) | 0 | Save CPU; aggressive reclaim needed |
| 8 GB      | `0x07` (all) | 500 | Balanced; moderate reclaim |
| 16 GB     | `0x07` | 1000 | Allow pages to age longer; less reclaim churn |
| ≥ 32 GB   | `0x07` | 2000 | Generous aging; abundant RAM |

### 1.3 swappiness

**Q: What swappiness value does optid set and why?**

`/proc/sys/vm/swappiness` controls the kernel's preference for swapping vs. dropping
page cache [PROVEN — kernel `Documentation/admin-guide/sysctl/vm.rst`]:
- `0` = never swap; drop page cache only
- `100` = equal preference for swapping and page cache drop (default 60)
- `200` = prefer swapping over page cache drop (new semantics in kernel 5.8+ with zram)

**With zram**: setting `swappiness=200` tells the kernel to swap to zram (compressed,
in-DRAM) aggressively and preserve the page cache (beneficial for interactive workloads)
[PROVEN — kernel 5.8 introduced swappiness values > 100 specifically for zram; systemd
zram-generator documentation recommends 200].

**Without zram** (or with NVMe swap only): keep `swappiness=10–20` to prefer keeping
pages in RAM and only swap under severe pressure [PROVEN — widely recommended for SSDs].

**optid setting**:
```bash
# With zram active:
echo 200 > /proc/sys/vm/swappiness
# Without zram:
echo 10 > /proc/sys/vm/swappiness
```

### 1.4 Memory Pressure Detection

**Q: How does optid detect memory pressure to adjust parameters at runtime?**

**PSI (Pressure Stall Information)** [PROVEN — kernel ≥ 4.20, `Documentation/accounting/psi.rst`]:
```bash
cat /proc/pressure/memory
# some avg10=0.12 avg60=0.08 avg300=0.02 total=123456
# full avg10=0.00 avg60=0.00 avg300=0.00 total=0
```

- `some`: fraction of time where at least one task was stalled on memory
- `full`: fraction of time where ALL tasks were stalled (severe pressure)

optid uses PSI `memory some avg10 > 5 %` as "moderate pressure" signal and `full avg10 > 1 %`
as "severe pressure" signal [HYPOTHESIS — thresholds; systemd-oomd uses similar thresholds].

Under severe pressure with zram active, optid may increase compression aggressiveness
(switch algorithm from `lz4` to `zstd` if possible) or call `sync; echo 3 > /proc/sys/vm/drop_caches`
to reclaim clean page cache [HYPOTHESIS — `drop_caches` is a heavy hammer; use sparingly].

### 1.5 RAM Tier Detection

**Q: How does optid determine the total RAM tier at startup?**

```bash
# Total physical RAM from MemTotal in /proc/meminfo (kB):
grep MemTotal /proc/meminfo
# MemTotal:       16218040 kB → 15.5 GB → tier "16GB"
```

Tier boundaries [HYPOTHESIS — round numbers for simplicity]:
- `≤ 6 GB` → 4 GB tier
- `6–12 GB` → 8 GB tier
- `12–24 GB` → 16 GB tier
- `≥ 24 GB` → 32 GB tier

RAM tier is read once at startup and cached; does not change at runtime (no hot-plug RAM on
target laptop hardware).

* * *

## 2. Architecture Decisions

### Decision A: optid Manages zram vs. Defer to zram-generator

**Selected: Defer to systemd-zram-setup + zram-generator.conf shipped by Rush Linux
packaging; optid audits and reports, not manages** [HYPOTHESIS — runtime algorithm change
requires device reset which disrupts swap; better to get it right at boot via static config].

### Decision B: MGLRU Tuning — Static (boot) vs. Dynamic

**Selected: Static per-tier values written at optid startup (once)** [HYPOTHESIS — MGLRU
values are not hot-path control surfaces; setting them once at boot is sufficient].

### Decision C: swappiness with/without zram

**Selected: `swappiness=200` when zram is present; `swappiness=10` without** [PROVEN —
kernel 5.8+ semantics make this the correct choice; zram-generator documentation confirms].

* * *

## 4. Evidence Gaps

| Gap | Acceptance threshold | Experiment |
|-----|---------------------|------------|
| zram compression ratio by algorithm | `lz4` ≥ 2.0×, `zstd` ≥ 2.5× on typical workload | `cat /sys/block/zram0/mm_stat` before/after 30min mixed browser+code session |
| MGLRU min_ttl benefit | ≥ 10 % fewer major faults vs. min_ttl=0 at 8 GB under pressure | `perf stat -e major-faults` with `stress-ng --vm 1 --vm-bytes 6G` for 60s; compare min_ttl=0 vs. 1000ms |
| mm_walk CPU overhead at 4 GB | Confirm CPU overhead with bit1=1 vs. bit1=0 under pressure | `perf stat -a -e cpu-cycles` while `stress-ng --vm 1 --vm-bytes 3G` |
| swappiness=200 page cache benefit | ≥ 15 % higher page cache hit rate vs. swappiness=60 | `/proc/vmstat pgpgin/pgpgout` ratio over 1h browser session with zram |
| PSI threshold calibration | Confirm `some avg10 > 5%` correlates with user-visible jank | Correlate PSI `some avg10` with compositor frame drops during 1h mixed workload |

* * *

## 5. Non-Goals

- optid does not manage swap partition (non-zram NVMe swap) — that is the installer's job.
- optid does not implement memory compaction (`/proc/sys/vm/compact_memory`) — too disruptive.
- optid does not configure transparent huge pages (THP) — separate tuning domain.
- optid does not manage cgroup memory limits per application — that is a container/sandbox concern.
- optid does not implement NUMA memory policies (not applicable to single-socket laptop CPUs).

* * *

## 6. WP Relationship Map

| WP tag | How this brief addresses it |
|--------|-----------------------------|
| WP-N12 | zram compression keeps swap in DRAM, reducing NVMe power-on events (feeds 0008/0009) |
| WP-N13 | MGLRU tuning reduces page cache churn, lowering memory bus activity and power |

* * *

## 7. Next Steps

**Immediate**
- Implement `crates/optid/src/sensors/memory.rs`: read `MemTotal`, current zram stats
  (`/sys/block/zram*/mm_stat`), PSI memory pressure, MGLRU `enabled` and `min_ttl_ms`.
- Implement `crates/optid/src/actuators/memory.rs`: write MGLRU params and swappiness
  at startup based on RAM tier.

**Short-term**
- Ship `zram-generator.conf` in Rush Linux packaging with tier-aware configuration.
- Run zram compression ratio experiment (§4 gap #1).

**Medium-term**
- Evaluate PSI-based dynamic zram algorithm switching if static boot config proves insufficient.
- Implement `optctl memory --status` showing zram compression ratio and PSI trends.

* * *

## Appendix: Suggested Reading

- Kernel docs: `Documentation/admin-guide/blockdev/zram.rst`
- Kernel docs: `Documentation/admin-guide/sysctl/vm.rst` — swappiness
- Kernel docs: `mm/vmscan.c` MGLRU implementation comments (Yu Zhao's extensive inline docs)
- systemd zram-generator: `zram-generator.conf` manual page
- LWN.net: "Multi-generational LRU: the guts of the thing" (2022)
- PSI documentation: `Documentation/accounting/psi.rst`
- Meta Engineering Blog: "Linux memory management at Facebook" (PSI usage at scale)
