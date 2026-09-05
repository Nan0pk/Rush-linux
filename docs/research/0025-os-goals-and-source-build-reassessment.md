# Rush OS goals and source-build reassessment

Status: Candidate — source review and repository analysis; hardware hypotheses unvalidated.

Date: 2026-09-05. Repository baseline: `8cdcf99175fac2c7d042410184f129d28a7ae9ad`.

The owner asked whether a custom source-built restart and corrected research
could substantially improve Rush, then authorized an isolated experimental
branch. That authorizes investigation and reversible implementation. It does
not establish Mac parity, approve every recommendation, or certify a release.

## Finding

Preserve the project and its history. Reassess the objectives and test the
smallest complete improvements before selecting a new distribution foundation.
The present evidence does not identify Arch packaging as the cause of Rush's
missing user-visible advantage. Source rebuilding is available within the
existing base; a new kernel, package manager or build framework is not required
to investigate it.

The first product should make everyday foreground work dependable under mixed
load while preserving useful background progress, battery life, sleep behavior
and user-selected display quality. Broader hardware coverage follows repeatable
results on the existing reference laptop and then a second distinct machine.
This is a recommended validation sequence, not withdrawal of the wider OS goal.

## Evidence and its limits

- **Source facts:** upstream documentation cited below, checked on 2026-09-05.
  Rolling documentation describes current upstream interfaces; it does not prove
  availability on Rush's installed kernel, firmware or tool versions.
- **Repository facts:** source and committed reports at the baseline above.
  Existing reports are attributed to their authors, not measurements repeated
  by this researcher.
- **Inference:** which interventions are likely to resolve current gaps.
- **Proposals:** experimental objective clarification and the comparison plan.
- **Unanswered:** the size of any physical advantage, hardware coverage and the
  engineering cost of a shipping desktop. No benchmark percentage is predicted.

The original [Apple inventory](0001-apple-power-stack-analysis.md) is useful as a
question list. Its numerical power shares and specific CLPC/controller claims
are not established by that document's evidence. The more careful
[integrated synthesis](0021-integrated-optid-research-disposition.md) and
[platform review](0022-platform-primitives-disposition.md) should be retained.
This reassessment supplements them; it does not claim to have independently
reproduced every statement in all 24 earlier papers.

## Recheck of the foundational claims

| Earlier idea | Primary-source finding and limitation | Consequence for Rush |
|---|---|---|
| Copy Apple's per-workload power intelligence | Apple documents QoS influencing scheduling, CPU/I/O work and timer latency. Its Apple-silicon guidance describes performance and efficiency cores. These sources do not establish the original inventory's exact CLPC PID implementation. [QoS](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/PrioritizeWorkAtTheTaskLevel.html), [Apple silicon](https://developer.apple.com/news/?id=vk3m204o) | Translate workload intent and integration principles. Do not make an unverified Apple algorithm an implementation requirement. |
| Reproduce Clutch/Edge using sched_ext | Linux supports BPF scheduling with fallback, but that is neither an Apple-equivalence proof nor a stable cross-version scheduler ABI. [sched_ext](https://docs.kernel.org/scheduler/sched-ext.html) | Keep the existing evidence restriction. First establish whether native scheduling is a measured bottleneck. |
| AMD CPPC files provide dynamic thread classification | AMD HFI describes hardware classification, ranking tables and scheduler consumption; this is not a portable CPPC-file event protocol. [AMD HFI](https://docs.kernel.org/arch/x86/amd-hfi.html) | Observe kernel behavior; verify the exact interface before adding a reader or claiming a userspace control. |
| Intel HFI can be read as ordinary files | Linux documents thermal-notification delivery to userspace, with updates rate-limited. [Intel HFI](https://docs.kernel.org/arch/x86/intel-hfi.html) | Do not treat a missing file as absence of the CPU feature. Implement an observer only when a concrete consumer needs it. |
| Detect and control generic ARM MPMM through AMU | The older paper has not established a portable, validated control path. CPUIdle documentation instead describes platform-specific drivers behind generic policy. [CPUIdle](https://docs.kernel.org/admin-guide/pm/cpuidle.html) | Remains unvalidated/deferred; an ARM experiment requires its own platform manual and hardware. No generic actuator is inferred. |
| Idle injection should be the primary thermal response | The kernel already documents a thermal power allocator with feedback control. That does not prove idle injection is superior to power caps on the target workload. [Thermal allocator](https://docs.kernel.org/driver-api/thermal/power_allocator.html) | Compare bounded controllers in the existing simulation first. Preserve native thermal protection and the exclusion of fan writes. |
| Deepest latency-permitted sleep is always optimal | CPUIdle considers target residency as well as exit latency. A short idle interval may not recover a deeper state's transition cost. [CPUIdle](https://docs.kernel.org/admin-guide/pm/cpuidle.html) | Clarify the Northstar. Let the kernel own fine timing; measure actual residency and total energy before adding policy. |
| A PM QoS limit measures device wake latency | PM QoS requests constrain behavior; the API is not a latency measurement rig. Runtime PM still depends on drivers and device state. [PM QoS](https://docs.kernel.org/power/pm_qos_interface.html), [runtime PM](https://docs.kernel.org/power/runtime_pm.html) | Retain provenance-aware contracts. Unknown measurements remain unknown. Validate the complete relevant wake path. |
| PSI can replace all polling and reveal user intent | PSI measures resource stalls and supports pollable threshold notifications. It does not identify a focused app, supply an input-to-display latency measure, or notify every relevant state change. [PSI](https://docs.kernel.org/accounting/psi.html) | Event-driven where supported, sparse bounded polling elsewhere; authenticate session intent separately. |
| A unified controller should own every low-level lever | Runtime PM already coordinates bus/driver operations; thermal and CPU drivers have their own control loops. [Runtime PM](https://docs.kernel.org/power/runtime_pm.html), [CPU frequency](https://docs.kernel.org/admin-guide/pm/cpufreq.html) | Coordinate policy and ownership, not duplicate all kernel/firmware mechanisms. Honor another component's ownership per domain. |
| DRAM-frequency, IRQ and LTR overrides are general power levers | No generic safe cross-platform contract for these proposals is demonstrated in Rush. The hardware findings in paper 0022 show existing ownership and platform limitations. | Preserve as platform-specific questions; no guessed vendor writes or global IRQ reassignment. Lack of proof is not proof of impossibility. |
| Display/media savings imply transparent scaling should be automatic | The existing feasibility study identifies compositor ownership and application-scoped tooling, not a portable transparent desktop control. [Gamescope upstream](https://github.com/ValveSoftware/gamescope) | Keep native resolution and fixed brightness for efficiency comparisons. Render scaling, when requested, is a separate quality/performance trade-off. |
| Specific idle power shares and sleep-drain numbers generalize | The original inventory does not attach the required machine, display, battery, workload and measurement provenance. | Retire these numbers as design constants. Measure the reference machine; report watts, joules, useful work and observation scope separately. |

## What building from source can and cannot change

Source construction controls versions, patches, optional features and compilation.
It does not expose firmware interfaces that do not exist or fix a missing
compositor integration without implementation work. Compiler optimization can
change code generation; more aggressive flags may alter standards compliance.
Profile-guided optimization uses execution profiles and needs representative
training plus separate evaluation workloads. [GCC options](https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html)

Thus the experiment must distinguish:

1. **Rebuilding the same source/configuration:** estimates packaging/toolchain
   differences; never claim this alone adds architecture.
2. **Changing one build option or patch:** tests that specific intervention.
3. **Changing runtime policy:** tests Optid or native service configuration.
4. **Changing the whole system:** tests the resulting product; cannot attribute
   a gain to source compilation alone.

Keep security mitigations, output correctness and supported hardware intact.
Do not select `-march=native` for a general image or globally use `-Ofast` just
because a microbenchmark improves. Keep the build machine's instruction set
separate from the declared target's requirements.

| Foundation | Control available | Trade-off and current disposition |
|---|---|---|
| Arch packages plus selective rebuilds, composed with mkosi | Existing package recipes and patches; mkosi accepts local package directories and snapshot selection | First experiment: smallest disruption and existing desktop ecosystem. Does not by itself prove reproducibility. [mkosi manual](https://github.com/systemd/mkosi/blob/main/mkosi/resources/man/mkosi.1.md), [makepkg manual](https://pacman.archlinux.page/makepkg.8.html) |
| Gentoo/Portage | Source-oriented package construction and selectable features | Credible alternative if broad per-package source configuration becomes a sustained need; migration and binary delivery still require evaluation. [Portage configuration](https://github.com/gentoo/portage/blob/master/man/make.conf.5) |
| Yocto/OpenEmbedded | Separate machine, distribution and software layers; explicit image construction | Strong candidate for a fixed supported hardware product. General desktop package coverage, update operation and maintainer workload must be evaluated first. [Concepts](https://docs.yoctoproject.org/overview-manual/concepts.html), [reproducibility](https://docs.yoctoproject.org/dev/test-manual/reproducible-builds.html) |
| Buildroot | Integrated toolchain, kernel, userspace and image builds | Useful for a bounded appliance/probe. Its whole-rootfs approach does not establish the desired general desktop lifecycle. [Manual](https://buildroot.org/downloads/manual/manual.html) |
| Linux From Scratch or a new custom builder | Manual control over assembly | Useful education or investigation; no demonstrated need to recreate package/update infrastructure for Rush. [LFS](https://www.linuxfromscratch.org/lfs/) |

Recommendation: retain the current base for the experiment. Revisit the base only
when a reproducible requirement cannot reasonably be met through selected
rebuilds, or an alternative demonstrates enough product/maintenance advantage
to justify migration. Freeze package inputs, toolchain, configuration and source
digests for comparisons; a repository date alone is not a complete lock.

## What the existing project really contributes

The ledger reports 12 completed construction packages at the baseline. Persistent
recovery, capability sealing, desired-state reconciliation and simulation are
assets to reuse after checking their applicability. They do not prove the
entire OS or each hardware combination safe.

The current main loop still checks termination through 100 ms sleeps;
foreground subscription explicitly produces no events. September's real sysfs
collection exposed incorrect ABI names, state interpretation and residency
units. Conflict detection uses a list of service names that misses actual
policy providers. These are direct implementation problems; there is no evidence
that changing the package base fixes them.

Current committed evidence includes:

- [August actuation and restoration](../inbox/2026-08-22-enforce-run/README.md):
  a bounded physical control demonstration, not an efficiency comparison.
- [September observability repair](../inbox/2026-09-04-o1-repair-record.md):
  fixtures now tied to actual sysfs structure; fresh independent verification
  remains required for completion.
- [Thermal hardware report](../../release/evidence/t1-thermal-proof/2026-09-04-victus/report.md):
  usable physical observations, with disclosed invocation and fault-case limits.
- [Simulation](../../release/evidence/optid-simulated-evidence/2026-08-29/report.md):
  software fault evidence and model-dependent estimates. Its apparent savings
  include display dimming and a mixed-load throughput loss; those cannot establish
  equivalent-service physical efficiency.

The legacy capture wrapper also changes system state, forces a Fedora profile,
stops broad process names, and runs all baseline cycles before all Optid cycles.
It must not be advertised as the clean source-build comparison without repairing
ownership/restoration and paired ordering. Missing input-latency probes remain
missing even if another latency metric is present.

## Correct the process without losing the evidence

Immediate correction on this branch: distinguish the full OS purpose from the
optimizer objective; permit requested strategy and independent disabled
experiments. Keep the actuation rule, human merge, hardware promotion and
release criteria intact.

Recommended follow-up: scope verification to affected behaviors, with explicit
dependency review. Preserve old receipts as historical facts and mark current
claims pending review, rather than describing previously implemented code as
absent. Permit a coherent cross-component fix to identify all affected packages.
Do not merely stop checking changed files: the September ABI failures show why
test provenance and real production paths matter. Any validator change needs
its own negative regression cases and a reviewed transition policy. Existing
package checks remain enforced during these experiments.

## Foresight and stopping conditions

The OS must be useful when Optid is unavailable. Native policy, a supported
desktop, safe updates and understandable recovery are the fallback product.
Users should receive built artifacts and automatic supported defaults, not a
source compilation or benchmark setup assignment.

After a credible first gain, replicate across a second hardware/firmware class
and after a kernel update. Budget regression maintenance, security updates,
build infrastructure and user-support cost alongside runtime gains. Better
performance on one workload is not universal optimality.

If the measured improvement is within noise, retain the simpler baseline. If
the benefit comes entirely from existing upstream configuration, ship that
configuration rather than an unnecessary new controller. If a source rebuild
helps one component, keep it local until broader rebuilding proves its value.
If a firmware limitation dominates, narrow the supported envelope explicitly
instead of promising software can erase it.

Execution, comparisons, proposed numerical margins and the current environment's
limits are in the [source-build experiment plan](../plans/source-build-experiment.md).
