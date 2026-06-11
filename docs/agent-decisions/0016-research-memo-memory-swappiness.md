# Research Memo 003: Memory & Swappiness

| Field | Value |
|---|---|
| ID | RM-003 |
| Strategic Questions | Q6, B2 |
| Track | Track B: Kernel & Scheduler |
| Complexity Class | Complicated |
| Date | 2026-06-12 |
| Driver | Arena Agent |

## 1. Context & Hypothesis
Rush Linux aims to maximize the utility of available RAM. Traditional `vm.swappiness=60` was tuned for slow HDDs. We hypothesize that on modern systems with **ZRAM** and **MGLRU**, a much higher swappiness (100–200) improves responsiveness by aggressively compressing anonymous memory to keep the filesystem cache (Page Cache) large and "warm".

## 2. Methodology
- **Audit:** Reviewed MGLRU (Multi-Gen LRU) behavior in Linux kernels 6.1+.
- **Comparative Research:** Analyzed Android (which uses `swappiness=100` and ZRAM) and Fedora (which uses `swappiness=100` with ZRAM).

## 3. Evidence & Data
- **Swappiness Meaning:** At `swappiness=100`, the kernel treats the cost of swapping and the cost of dropping a page cache entry as equal. At `swappiness > 100`, it prefers swapping (compressing to ZRAM) over dropping page cache.
- **ZRAM Efficiency:** Since ZRAM is in-memory and compressed, the "seek time" is zero and throughput is limited only by CPU/Memory bandwidth. The cost is significantly lower than SSD/HDD swap.
- **MGLRU Synergy:** MGLRU is better at identifying "cold" anonymous pages than the traditional LRU. Combined with high swappiness, it can move cold data to ZRAM earlier, preventing "thrashing" when an application suddenly needs a large page cache (e.g., loading a large binary or video file).
- **The "Gap":** Current Rush `policy.toml` defines `vm_swappiness`, but `optid` ignores it.

## 4. Option Comparison

| Option | Pros | Cons | MCDA Score (H/M/L) |
|---|---|---|---|
| **A: Static High (100)** | Simple, matches Fedora/Android. | Suboptimal for disk-swap users. | **M** |
| **B: Dynamic Adaptive** | Matches the "Rush" philosophy. | Complexity in actuation logic. | **H** (Recommended) |
| **C: Conservative (60)** | Safe for all hardware. | Leaves performance on the table for ZRAM. | **L** |

## 5. Pre-Mortem Analysis
**Failure Scenario:** A user has a tiny ZRAM device and a slow HDD swap. High swappiness causes the kernel to quickly fill ZRAM and then spill over to the slow HDD, causing a sudden system crawl.
**Mitigation:** `optid` must check `/proc/swaps` or `/sys/block/zram0`. If no ZRAM is detected, it must cap `vm.swappiness` at 60.

## 6. Decision Hint
- **Q6 (Swappiness Gap):** Close the gap. Implement `vm.*` actuation in `optid` v0.6.
- **Conditional Trigger:** Apply `swappiness=150` (or the policy value) ONLY if a ZRAM device is the highest priority swap.

## 7. Reversal Plan
If high swappiness causes regressions, a single command `optctl mode balanced` (which sets swappiness back to 60) immediately reverts the kernel state.
