# Non-bare-metal optid validation method

Status: candidate research for planned package I2. This paper proposes future
testing work. It does not authorize implementation, change runtime defaults,
advance a package, weaken a release gate, or substitute modeled results for
physical hardware evidence.

Updated: 2026-08-29

## Question

If Rush has no physical test machine, how can the project still test optid's
policy, safety, performance, throughput, power direction, and thermal behavior
without inventing evidence?

## Direct answer

Rush can test most of optid before physical hardware exists, but not with one
kind of virtual machine and not under one generic claim of "cloud testing."
The defensible method has three separate evidence lanes:

1. **Cloud virtual machines** measure real guest workload outcomes: latency,
   throughput, CPU time, pressure, memory behavior, I/O, and Linux resource
   controls that the guest actually exposes.
2. **Deterministic QEMU and simulation-root tests** prove policy decisions,
   requested actions, read-back, restoration, topology changes, crashes, and
   injected failures.
3. **gem5 power and thermal models** estimate the direction and sensitivity of
   power and temperature changes under declared model assumptions.

Kepler's pure-VM power estimate may be recorded as a secondary signal. It must
not decide a pass, prove laptop watts, or be combined with modeled gem5 energy
to create a stronger-looking number.

These lanes can reject bad policies and find promising ones. They cannot prove
physical battery life, fan behavior, suspend/resume, firmware compatibility,
display power, device-link power, or support for a named laptop. Those claims
remain blocked until matching physical evidence exists.

## The problem with a single cloud benchmark

An ordinary cloud VM normally does not expose a laptop battery, backlight,
platform profile, PCIe link power, SATA link power, fan controller, or the
physical energy counter for the guest. A successful sysfs write against a fake
tree proves software behavior. A faster workload in a VM proves a guest
performance result. A simulator can calculate energy from a model. None of
those facts turns into measured laptop power merely because they occurred in
the same pipeline.

The method therefore classifies every result before running the test:

| Evidence class | What it can support | What it cannot support |
|---|---|---|
| Measured guest outcome | Performance, latency, throughput, pressure, resource use, and cloud cost per fixed unit of work on the tested VM class | Laptop energy, battery life, firmware behavior, or physical-device compatibility |
| Deterministic software proof | Policy, state transitions, write intent, read-back, restore, recovery, and failure handling | Real performance benefit or physical energy saving |
| Model-conditional estimate | Direction and sensitivity of energy or temperature inside the declared model and parameter range | Absolute target-hardware watts, temperature, runtime, or support |
| Physical measurement | The matching machine, workload, environment, and interfaces measured | Untested machines or operating conditions |

A report must use these names. It must not relabel an estimate as a
measurement or a deterministic fixture as hardware support.

## Current Rush facts

The repository already contains the right starting boundary:

- optid has a test-only `--simulation-root` path that refuses real writes;
- planned package I2 requires a whole-system scenario and fault matrix;
- the benchmark manifest already names responsiveness, battery, gaming,
  realtime-audio, and server-throughput workloads; and
- release policy already reserves physical hardware and comparative evidence
  for later release tiers.

This paper extends the research basis for I2. It does not change I2 from
`planned` and does not modify those release requirements.

## Proposed test system

### Cloud virtual-machine laboratory

Run the same immutable Rush image and workload bundle on repeatable VM
allocations. Each allocation is one experimental block. The baseline and
candidate run as an adjacent pair inside that allocation, with their order
randomized. This reduces drift from changing cloud hosts, neighbours, clock
conditions, and time of day.

The laboratory must probe capabilities before each run. A capability is usable
only when the guest can read it, change it, read the requested value back, and
restore the prior value. A missing or inert interface is `unsupported`, not a
pass and not a zero effect.

Cloud tests should use fixed work where possible: compile the same source,
serve the same number of requests, transfer the same bytes, execute the same
database transactions, or complete the same rendering trace. This permits
time-per-work, vCPU-seconds-per-work, and cost-per-work comparisons without
calling them electrical energy.

The minimum capability record is:

- provider, region, zone, instance family, allocation identifier, and price;
- image digest, Rush commit, kernel, boot arguments, optid config, and daemon
  state;
- reported CPU model, vCPU topology, NUMA layout, memory, storage, and network;
- hypervisor and nested-virtualization status when visible;
- cgroup v2 controller availability and delegation;
- PMU, PSI, clock, RAPL, and steal/interference visibility;
- start time, duration, benchmark versions, workload hashes, and random seed;
  and
- all detected competing daemons or host policies visible to the guest.

### Deterministic QEMU and simulation-root laboratory

Use simulation-root fixtures as the canonical representation of optid-owned
inputs and outputs. Use QEMU full-system execution for boot, service lifecycle,
reboot, hotplug, and guest-kernel interactions. QEMU record/replay or
instruction-counted execution may reproduce failures, but instruction-counted
virtual time must never be reported as wall-clock performance.

Every actuation attempt produces an action receipt with:

- domain and resolved target;
- value before the action;
- requested value;
- read-back value;
- classification: applied, unchanged, unsupported, denied, malformed, or
  failed;
- restoration trigger; and
- final restored value or explicit restoration failure.

The fault matrix must cover, at minimum:

- idle, interactive, latency-critical, and throughput policy states;
- AC and battery transitions;
- thermal rise, alarm, recovery, missing data, and stale data;
- foreground arrival/loss and GameMode arrival/loss;
- device and CPU hotplug/removal;
- unsupported hardware and an empty allowlist match;
- permission denial, partial write, short write, malformed sysfs, and changed
  value between observation and actuation;
- config reload success/failure and capability-seal failure;
- daemon crash before write, during a multi-write transaction, and after write;
- recovery crash, circuit opening/canary/closing, and reboot recovery; and
- multiple domains requesting changes together, including a failure partway
  through the set.

Each scenario runs with all domains off, all domains observing, one mock domain
actuating at a time, and all supported mock domains actuating together. The
expected policy decision, action receipt, journal state, circuit state, and
final restored state are part of the fixture oracle.

QEMU can add and remove virtual CPUs and devices through QMP. The harness must
wait for and verify the guest-observed topology result because an unplug
request does not guarantee that the guest completed it.

### gem5 power and thermal laboratory

Use gem5 full-system execution for a small, representative workload set after
the cloud lane has identified candidate policies. KVM fast-forward may reach
the measured region, followed by a detailed CPU model for that region.

The first model is not a laptop claim. It is a declared reference model with:

- CPU microarchitecture and topology;
- clock and voltage states;
- active, idle, and sleep power expressions;
- memory and relevant component energy assumptions;
- thermal domains, resistance, capacitance, ambient temperature, and initial
  temperature;
- mapping from an optid state or mock action to a modeled state transition;
  and
- source, units, plausible range, and confidence for every parameter.

Run sensitivity analysis over the plausible parameter range. A policy may be
called promising only when the direction of its fixed-work energy result is
stable across the declared range and it does not violate the workload's
latency or throughput floor. If the sign changes with plausible assumptions,
the result is `model-sensitive` and must not guide default policy.

Before physical calibration exists, allowed wording is:

> In the declared reference model and parameter range, policy P reduced
> modeled energy per fixed work while satisfying metric floors.

Disallowed wording includes "saves X watts," "adds X hours of battery," or
"runs X degrees cooler" on a real system.

### Kepler as a secondary estimate

Kepler's pure-VM path estimates power from a learned model when direct energy
sources are absent. Its documentation also describes important missing
factors, including instruction mix, power states, memory, I/O, and GPU. A
target-specific model requires target measurement data that Rush does not have
under the no-physical-hardware assumption.

Therefore Kepler may be logged only as:

- an additional estimate beside raw utilization and work counters;
- a way to detect gross internal inconsistencies; or
- a later comparison target after physical calibration is available.

It must not be averaged with gem5, converted into battery duration, or used as
the only reason to accept or reject a policy.

## Domain-by-domain method

| optid domain | Deterministic proof | Real cloud outcome | Model-conditional estimate | Deferred physical proof |
|---|---|---|---|---|
| CPU EPP | Discovery, policy, requested value, read-back, restore, denial, hotplug | Only if a provider exposes a working guest interface; otherwise unsupported | Map policy states to explicit DVFS/power states in gem5 | Firmware response, package energy, battery effect |
| Platform profile | Discovery, allowlist, write/read-back/restore, invalid values | Usually unsupported | Map profiles to declared model parameter sets | Firmware behavior, fan/thermal/performance effect |
| VM sysctl | Write/read-back/restore and malformed/denied paths | Memory pressure, PSI, latency, throughput, faults, CPU time | Optional memory-system sensitivity | Workload generality and target memory behavior |
| CPU DMA latency | Open/close lifecycle, crash recovery, conflicting requests | Guest wakeup/tail-latency effect where supported | Idle-state and wake-cost assumptions | Physical idle-state residency and energy |
| Device resume latency | Device selection, request lifecycle, removal, restore | Virtual-device resume behavior if observable | Injected wake delay and component power states | Real device wake latency and power |
| Runtime PM | Device state, hotplug, denied/malformed state, restore | Virtual-device functionality and workload impact | Component active/idle energy model | Driver/device quirks, suspend, real energy |
| PCIe ASPM | Policy, topology, unsupported/denied state, restore | Functional behavior only when a virtual interface exists | Link-state transition and energy assumptions | Link stability, device compatibility, physical energy |
| SATA ALPM | Policy, target selection, unsupported/denied state, restore | Virtual-storage workload outcome only when exposed | Link-state transition and energy assumptions | Drive compatibility, latency, physical energy |
| Backlight | Ownership, bounds, zero-value protection, restore | No defensible ordinary-cloud power result | Declared display-load model only | Brightness quality, panel power, session ownership |
| Cgroup reweight | Scope resolution, value lifecycle, removal, restore | Real cgroup v2 latency/throughput/PSI outcome under contention | Usually unnecessary | Cross-hardware scheduler generality |
| Thermal observation | Missing/stale/malformed/alarm/hysteresis/recovery logic | Guest workload temperature only if genuinely exposed and relevant | Feed gem5 thermal output into simulation root | Sensor identity, cooling, fan, skin temperature |

No domain passes because its expected file existed. Passing software behavior
requires an action receipt. Passing a benefit hypothesis requires a measured
outcome or a separately labeled model estimate.

## Workload and metric matrix

The existing benchmark scenarios remain useful, but their non-physical claims
must be narrowed.

| Scenario | Cloud workload examples | Primary outcomes | Model use | Mandatory limitation |
|---|---|---|---|---|
| Mixed-load responsiveness | Foreground request or launch probe against oversubscribed background CPU and I/O | p50/p95/p99 latency, throughput of both classes, CPU/IO PSI, CPU time | CPU state and contention sensitivity | VM latency does not prove laptop input latency |
| Laptop battery proxy | Fixed CPU/memory/I/O work plus defined idle intervals | work completed, elapsed time, vCPU-seconds per work, cost per work | Modeled energy per fixed work and modeled idle energy | Never call this battery life or measured watts |
| Gaming frame-time proxy | CPU-side simulation or replayed frame workload; optional constrained GPU model | frame-work completion and p95/p99 scheduling delay | GPU work only in an explicit supported model | Ordinary cloud GPU results do not represent the target display/GPU stack |
| Realtime audio | `cyclictest`-style scheduling load or a deterministic audio deadline workload | missed deadlines, p95/p99/max wake latency, background throughput | Idle/wake-state sensitivity | Hypervisor scheduling can dominate tails |
| Server throughput | HTTP, database, compile, compression, network, and storage fixed-work tests | RPS/TPS/IOPS, p95/p99, bytes, CPU time, PSI, cost per work | Modeled energy per request or transaction | Applies to tested VM class and workload only |

Every scenario names one primary outcome before the run. Secondary metrics must
not replace a failed or missing primary outcome.

## Experimental protocol

### Screening phase

1. Probe all capabilities and preserve the machine record.
2. Verify benchmark output against a known-good fixture.
3. Run optid off, observe, and apply with a no-op policy.
4. Run each usable domain separately.
5. Run the combined policy only after individual-domain behavior is known.
6. Run a deliberately harmful control to show that the harness detects a
   regression rather than always reporting success.

Screening locates failures and estimates variance. It does not authorize a
benefit claim.

### Confirmatory phase

1. Define the primary outcome, minimum worthwhile effect, confidence level,
   precision target, invalid-run rules, and maximum run budget before looking
   at confirmatory results.
2. Use pilot variance to choose the starting repetition count. Do not assume
   that three, five, twenty, or any other fixed number is automatically enough.
3. Warm up the image and workload using the same rule for both treatments.
4. Randomize the order within balanced baseline/candidate pairs and interleave
   pairs over the allocation lifetime.
5. Keep workload input, image, VM allocation, and observer settings constant
   within each pair.
6. Stop only at the predeclared maximum or when the predeclared precision rule
   is met. Do not stop when a desired significance threshold first appears.
7. Repeat the confirmatory experiment across fresh allocations, times, and at
   least two materially different instance families. Prefer a second provider
   when cost and access permit.
8. Analyze paired effects within a VM class. Keep materially different CPU or
   provider classes separate; do not hide disagreement inside one average.

Report raw values, paired differences or ratios, an uncertainty interval,
invalid pairs, and the complete observation conditions. Tail metrics retain
their sample distributions; a zero or missing percentile is not a valid
measurement.

## Automatic invalidation rules

A run is invalid and the pipeline fails closed when:

- a primary metric is missing, `NA`, `NaN`, infinite, impossible, or zero when
  zero cannot be a real result;
- the workload completed zero iterations or its output failed validation;
- an intended action lacks a before/request/read-back/restore receipt;
- the requested lever did not change and the arm was not predeclared as a
  no-op control;
- the image, kernel, VM shape, workload input, or benchmark version changes
  within a pair;
- optid or the workload crashes outside an intentional fault scenario;
- restoration fails;
- another guest policy daemon conflicts with the test;
- clock, topology, PMU, cgroup, or other required capability changes within a
  pair; or
- a predeclared interference threshold is exceeded.

Interference rejection must be symmetric. The harness may not discard a slow
candidate run while retaining an equally noisy baseline run. Invalid results
remain in the evidence bundle with their reason.

## Controls that prove the harness

The harness itself must be falsifiable:

- **No-op control:** baseline and candidate request identical behavior. The
  method should report no meaningful effect within its precision.
- **Known regression:** intentionally reduce foreground CPU weight or add a
  bounded delay. The method must detect the expected harm.
- **Inactive lever:** request a nonexistent or inert virtual interface. The
  result must be `unsupported`, never improvement, regression, or pass.
- **Corrupt metric:** emit `NA`, zero iterations, or a false zero latency. The
  entire arm must fail.
- **Failed restore:** inject a restore denial. The safety result must fail even
  if workload performance improved.
- **Model sign change:** choose plausible low/high model inputs that reverse an
  energy result. The result must be `model-sensitive`, not accepted.

A harness that cannot reject these controls is not ready to assess optid.

## Evidence bundle and verdicts

Each campaign should emit one immutable bundle containing:

- experiment plan and predeclared thresholds;
- image, source, configuration, workload, and model digests;
- capability and environment records;
- randomized schedule and seeds;
- stdout, stderr, journal, action receipts, and recovery records;
- raw per-iteration metrics and invalidation reasons;
- analysis code and generated tables;
- model files, parameter sources, ranges, and sensitivity output; and
- a verdict for each claim, not only a campaign-wide verdict.

Allowed verdicts are:

- `software-pass` or `software-fail`;
- `measured-improvement`, `measured-regression`, `measured-no-clear-effect`, or
  `measured-invalid`;
- `model-favorable`, `model-unfavorable`, `model-sensitive`, or
  `model-invalid`; and
- `unsupported`.

No overall score may turn several weak proxies into one strong claim.

## Proposed future implementation sequence

This is proposed future work under I2, not work authorized by this paper:

1. Define the evidence schema, claim classes, action receipt, invalidation
   rules, and negative controls before adding a cloud provider.
2. Complete the simulation-root scenario and fault matrix locally and in CI.
3. Add a QEMU controller for reboot, topology change, virtual power-supply
   state, and deterministic reproduction.
4. Add a provider-neutral cloud runner that consumes an existing VM rather
   than embedding provider credentials or lifecycle policy in optid.
5. Automate paired randomized workloads and guest capability capture.
6. Add per-domain screening and the confirmatory statistical report.
7. Build one declared gem5 reference model and sensitivity pipeline only for
   policies that survive cloud performance screening.
8. Add Kepler only as an optional secondary estimate.
9. Preserve all unsupported areas as explicit later physical validation work.
10. When a physical machine becomes available, calibrate or reject the model
    and measure the still-open hardware claims. Do not rewrite old estimates as
    measurements.

The provider-neutral boundary prevents this research from selecting AWS, GCP,
Azure, or another service as project policy. Provider choice depends on future
access, cost, exposed capabilities, and reproducibility probes.

## Failure modes this method is designed to prevent

- treating host billing or a VM estimator as laptop watts;
- reporting a fake sysfs write as hardware support;
- measuring optid when none of its intended levers changed;
- accepting `NA`, zero iterations, or impossible zero latency as a pass;
- hiding noisy or unfavorable samples;
- tuning until a desired result appears and testing on the same data;
- allowing a combined policy to conceal one harmful lever;
- trusting one cloud allocation, CPU family, time, or provider;
- using one assumed thermal or voltage model without sensitivity analysis;
- optimizing a model that has never been validated against the physical
  application; and
- weakening physical release gates because simulation became convenient.

## Sourced facts

### Linux interfaces

- Linux cgroup v2 defines CPU weight as a real distribution control for CPU
  cycles. PSI measures time lost to CPU, memory, and I/O resource contention.
  These are valid guest outcome interfaces when the VM exposes them.
- Linux powercap exposes physical energy counters such as RAPL when the
  platform provides them. Their absence in a guest is not evidence of zero
  energy.
- The Linux `test_power` driver provides synthetic AC, battery, and USB
  power-supply state for testing.
- systemd resource-control properties activate and configure matching cgroup
  controllers when the hierarchy supports them.

### Emulation and modeling

- QEMU record/replay records nondeterministic events, and TCG instruction
  counting supports deterministic system-emulation timing. This does not make
  virtual time a wall-clock or energy result.
- QEMU QMP supports virtual CPU hotplug and unplug requests; unplug completion
  can require guest cooperation.
- gem5 supports full-system x86 execution, statistics, power-model states and
  expressions, voltage/clock domains, and thermal networks.
- Kepler documents a pure-VM estimation path and limits in its power
  attribution, including CPU-state, memory, I/O, and GPU effects.

### Experimental method

- NIST describes blocking known nuisance factors and randomizing remaining
  ones, paired analysis through corresponding measurements, and confidence
  intervals as a statement of precision.
- Google Benchmark supports repetitions and randomized interleaving to reduce
  system-state drift.
- SPEC performance-reporting rules require repeated runs, output validation,
  and disclosure of performance-relevant observation conditions.
- Model-credibility guidance distinguishes verification of implementation,
  validation against the real application, and quantification of parameter and
  result uncertainty.

## Measurements made by Rush

No new Rush measurement was made for this research paper. It proposes a method
and records no cloud, QEMU, gem5, Kepler, performance, power, or thermal result.

## Assumptions

- Future agents can obtain ordinary Linux cloud VMs but may have no physical
  laptop or server under their control.
- The VM permits required guest software, cgroup v2, and privileged guest
  operations; optional interfaces are capability-probed rather than assumed.
- `--simulation-root` remains test-only and cannot resolve to the real host
  root during these scenarios.
- A first gem5 reference model will be intentionally generic until physical
  calibration data exists.
- Provider availability and pricing will change, so this method defines
  capability requirements rather than naming a permanent provider.

## Proposals

- Adopt the three-lane evidence classification for future I2 design.
- Require action receipts and fail-closed metric validation.
- Use paired randomized blocked experiments for cloud A/B work.
- Require negative controls before accepting optid benchmark results.
- Treat power and thermal output as model-conditional until physical
  calibration and validation exist.
- Preserve T3/T4 physical and comparative release evidence without exception.

These proposals need maintainer acceptance through the normal package and
decision workflow before implementation.

## Unanswered questions

- Which cloud services expose stable CPU identity, steal/interference signals,
  nested virtualization, and useful PMU counters at an acceptable cost?
- Which minimum worthwhile effects and latency/throughput floors should govern
  each benchmark scenario?
- Which CPU model and parameter sources should define the first gem5 reference
  system?
- How should optid states map to modeled voltage, frequency, idle, device, and
  display states without smuggling unverified hardware assumptions into the
  model?
- Is a GPU full-system model worth its cost before any physical GPU target is
  nominated?
- Which later physical systems will provide calibration and external
  validation, and what range of model applicability will they support?

## Package-state consequence

This paper does not advance or complete I2. It supplies a researched candidate
method for the future I2 whole-system simulation and fault matrix. No dependency
is unlocked, no implementation is claimed, and no release evidence is added.

## Sources

- Linux kernel documentation, **Control Group v2**:
  <https://docs.kernel.org/admin-guide/cgroup-v2.html>
- Linux kernel documentation, **PSI — Pressure Stall Information**:
  <https://docs.kernel.org/accounting/psi.html>
- Linux kernel documentation, **Power Capping Framework**:
  <https://docs.kernel.org/power/powercap/powercap.html>
- Linux kernel source, **test_power.c — power-supply test driver**:
  <https://github.com/torvalds/linux/blob/master/drivers/power/supply/test_power.c>
- systemd documentation, **systemd.resource-control**:
  <https://www.freedesktop.org/software/systemd/man/systemd.resource-control.html>
- QEMU documentation, **Record/replay**:
  <https://qemu.readthedocs.io/en/v8.2.10/system/replay.html>
- QEMU documentation, **TCG instruction counting**:
  <https://qemu.readthedocs.io/en/v8.2.10/devel/tcg-icount.html>
- QEMU documentation, **Virtual CPU hotplug**:
  <https://qemu.readthedocs.io/en/v8.2.10/system/cpu-hotplug.html>
- gem5 documentation, **x86 full-system tutorial**:
  <https://www.gem5.org/documentation/gem5-stdlib/x86-full-system-tutorial>
- gem5 documentation, **Power and thermal model**:
  <https://www.gem5.org/documentation/general_docs/thermal_model>
- gem5 documentation, **ARM DVFS support**:
  <https://www.gem5.org/documentation/learning_gem5/part2/arm_dvfs_support/>
- gem5 documentation, **ARM power modelling**:
  <https://www.gem5.org/documentation/learning_gem5/part2/arm_power_modelling/>
- gem5 documentation, **gem5 statistics**:
  <https://www.gem5.org/documentation/learning_gem5/part1/gem5_stats/>
- Kepler documentation, **Power model**:
  <https://sustainable-computing.io/archive/design/power_model/>
- Kepler documentation, **Power attribution**:
  <https://sustainable-computing.io/kepler/usage/power-attribution/>
- Kepler documentation, **Power-estimation model server**:
  <https://sustainable-computing.io/archive/kepler_model_server/power_estimation/>
- NIST/SEMATECH e-Handbook, **Randomized block designs**:
  <https://www.itl.nist.gov/div898/handbook/pri/section3/pri332.htm>
- NIST/SEMATECH e-Handbook, **Paired and two-sample tests**:
  <https://www.itl.nist.gov/div898/handbook/eda/section3/eda353.htm>
- NIST/SEMATECH e-Handbook, **Confidence limits for the mean**:
  <https://www.itl.nist.gov/div898/handbook/eda/section3/eda352.htm>
- Google Benchmark documentation, **User Guide**:
  <https://google.github.io/benchmark/user_guide.html>
- Standard Performance Evaluation Corporation, **CPU 2017 Run and Reporting
  Rules**:
  <https://www.spec.org/cpu2017/Docs/runrules.html>
- Papadopoulos et al., IEEE Transactions on Software Engineering, 2021,
  **Methodological Principles for Reproducible Performance Evaluation in Cloud
  Computing**:
  <https://research.vu.nl/en/publications/methodological-principles-for-reproducible-performance-evaluation/>
- ASME, **Verification, Validation and Uncertainty Quantification**:
  <https://www.asme.org/codes-standards/publications-information/verification-validation-uncertainty>
- NASA, **Credibility Assessment Scale**:
  <https://ntrs.nasa.gov/api/citations/20090005963/downloads/20090005963.pdf>
- U.S. Department of Energy, **Predictive Simulation**:
  <https://www.energy.gov/ne/predictive-simulation>
