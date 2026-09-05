# Rush Linux direction and progress

Assessment: 2026-09-05, experimental branch, baseline `8cdcf99`.

Verdict: substantial foundations; product advantage still unproven. The old
June 26 assessment is preserved in Git history. A successful scheduled workflow
does not substitute for an updated strategic judgment.

## What the project is for

An adaptive Linux OS that feels responsive, uses energy efficiently, preserves
useful throughput and requires little user effort. Optid is a means toward that
goal alongside desktop integration, native power policy, updates and recovery.
The [project brief](../PROJECT_BRIEF.md) describes the whole product; the proposed
Northstar amendment separates it from Optid's narrower optimization objective.

## What the history shows

| Period | Actual progress | Remaining limitation |
|---|---|---|
| May 25 | Initial adaptive Linux scaffold and broad OS goals | No finished system |
| June | D-Bus, image/boot/install paths and committed VM acceptance records | Those records do not prove hardware efficiency |
| Late June–July | testOS/LiveDev, collection automation and repeated integration repairs | Development infrastructure grew before a clean user-benefit comparison |
| July 22 onward | Revised recovery/capability architecture and construction plan | Initially merged modules required runtime integration repairs |
| August | Safety implementation, bounded physical actuation and production-loop simulation | Software correctness and modeled gains remain distinct from physical gains |
| September 4 | Thermal observations and correction of real sysfs assumptions | Runtime observability needs fresh verification; conflict ownership remains incomplete |

At this baseline the Optid ledger records 12 completed packages, one candidate,
one merged but incomplete, three ready for parallel work and thirteen planned.
This is not an OS completion percentage. Read the
[current work selector](../plans/current-work.md) for live construction state.

## What should be preserved

Upstream kernel/userspace reuse, persistent recovery, bounded writes, independent
proof for high-risk claims, simulation tied to production paths, and truthful
hardware promotion. The September sysfs failures demonstrate why tests based
only on invented interfaces are insufficient.

The [primary-source reassessment](../research/0025-os-goals-and-source-build-reassessment.md)
finds no evidence that Arch packaging is the main obstruction. Local source
rebuilds can be evaluated within the existing build system.

## What changes on this branch

- Propose a full-OS objective clarification and remove the obsolete ban on
  requested strategic recommendations.
- Provide snapshot/local-package selection and a no-build plan through the
  existing whole-image builder.
- Specify separate comparisons for Optid, source rebuilding and the whole
  product, with equivalent work and quality.
- Mark obsolete research/status prose so it is not mistaken for current truth.

Package verification, privileged defaults, hardware allowlists and release
milestones remain under their existing rules. Narrower behavior-based
verification is a researched follow-up, not a silently disabled gate.

## Next decision and evidence

Execute the [source-build experiment plan](../plans/source-build-experiment.md):
repair the existing capture path, validate probes, measure the existing laptop,
then choose one intervention from its actual bottleneck. A new source-built base
is justified only by a demonstrated limitation or a concrete lifecycle benefit.

Keep native desktop/install/update integration moving independently of optional
optimizer domains. A usable OS with a modest measured improvement is a better
first product than a comprehensive unmeasured controller. No finish date or
Mac-parity percentage is supported by the present evidence.
