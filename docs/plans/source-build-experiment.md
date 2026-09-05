# Source-build experiment

Status: authorized isolated investigation; implementation and physical results
are tracked separately below. Date: 2026-09-05.

Baseline: `8cdcf99175fac2c7d042410184f129d28a7ae9ad`.
Branch: `work/20260905-reassess-os-goals-and-source-build-experiments`.

## Purpose and boundary

Find a repeatable improvement in useful work, responsiveness and energy before
deciding whether Rush needs a different distribution foundation. Preserve the
existing project, native fallback, package evidence and hardware restrictions.
The [research reassessment](../research/0025-os-goals-and-source-build-reassessment.md)
records why a complete restart is not currently justified.

The proposed Northstar amendment separates OS goals from Optid's energy
objective. It is reviewable on this branch, not a claim of permanent acceptance.
This plan does not select another active Optid package or unlock dependencies.
The [current work selector](current-work.md) continues to own that work.

## Three comparisons, kept separate

| Question | Control | Treatment | What may change |
|---|---|---|---|
| Does Optid help? | Same Rush image with native policy and Optid off | Same image with a verified bounded Optid policy | Optid ownership/policy only; record the native owners and actual applied actions |
| Does source rebuilding help? | Pinned Rush package set | Same package set with one selected component rebuilt | One documented build/patch intervention; identical Optid state in both arms |
| Is Rush a better product? | Maintained mainstream system as actually shipped | Rush configuration intended for users | Whole product; report differences, do not attribute the result to source compilation alone |

Do not benchmark the full custom base until the second comparison identifies a
limitation or the full-base option offers a separately demonstrated lifecycle
advantage. Do not use Mac/PC hardware comparisons to isolate OS effects. A later
Mac comparison is an experience comparison with disclosed hardware, display,
battery capacity and application differences.

## First useful experience

Use the existing reference laptop for browser interaction during a fixed local
background build, with fixed display settings and power source. This connects
foreground responsiveness, completed background work and energy in one case.
Use local, hashed content and fixed source/build inputs. Separate input-to-frame
latency from frame timing and scheduler wake latency; none is a substitute for
the others. The current missing input-latency probe blocks that specific claim.

Start with a pilot of five matched pairs to check instrumentation, restoration,
noise and duration. The pilot cannot certify an improvement. Before a separate
confirmatory campaign, freeze the exact workload, one primary outcome, secondary
requirements, sample count, exclusion rules and statistical method. Twenty pairs
is a proposed starting budget, not automatic proof or a fixed universal rule;
use the pilot variance to assess whether it can distinguish the chosen margin.
Do not repeatedly extend sampling until a favorable result appears.

Proposed decision margins for review before confirmatory collection:

- Material benefit: at least 5% lower joules for the same completed work, or
  at least 5% lower foreground p99 latency with energy and work preserved.
- No more than 2% regression in background completion time/throughput, energy
  for equal work, and relevant latency metrics when they are not the primary
  outcome. These are experimental margins, not changes to release criteria.
- Same visible brightness, resolution, refresh rate, content and audio quality;
  no new crashes, lost devices, audio underruns or failed restoration.
- The uncertainty interval must support the claimed benefit and the allowed
  regression margins. Overlap or insufficient measurement means inconclusive,
  not a win. Keep individual results, not only pooled summaries.

For a timed service window, also report completed work. For a fixed-work job,
measure total joules through completion, including documented warmup/cooldown
boundaries. Report package RAPL as package energy, not total system energy.
Use battery discharge or a suitable external meter for platform energy; do not
mix sources across arms. Connected power and charge counters require explicit
handling, not a zero-energy assumption.

## Collection procedure to implement in the existing harness

Reuse `rushbench` and its result records. Extend the canonical preset/capture
path, not the older shell benchmarks or a new parallel framework.

1. Record source SHA, source/recipe digests, build tool versions and flags,
   kernel/configuration, firmware scope, installed package manifest and hashes,
   policy, workload/content hashes and physical-machine identity. Preserve raw
   private inventory locally; publish a sanitized machine alias and safe scope.
2. Inventory control owners by actual service and interface ownership. Restore
   only services/processes started or stopped by this run and their prior
   profiles. Do not use global `pkill`, delete shared recovery state, or disable
   allowlists to manufacture a treatment. An unarmed Optid run is not an enabled
   treatment; a legitimately redundant action is a recorded no-op.
3. Precompute a balanced randomized AB/BA pair order and retain its seed. Pair
   whole independent runs, not adjacent samples within one run. Match battery
   charge band, initial thermal conditions, desktop session and background load.
   Fixed baseline-then-treatment ordering is not adequate.
4. Capture per-run latency distributions, elapsed time, completed work, energy,
   state transitions, errors and restoration outcome. Keep failed attempts and
   reasons. Apply predefined exclusions symmetrically; publish excluded counts.
5. Analyze paired differences/ratios at the run level. Report effect sizes and
   uncertainty. Do not average percentiles as though they were raw samples or
   count correlated frames as independent benchmark runs. Report every service
   quality requirement alongside the primary outcome.
6. Re-run the selected improvement on a second hardware class before broad
   claims, then test a kernel update and suspend/resume. Promote only the measured
   hardware/firmware scope through the existing review path.

The legacy `tools/phase-d-capture.sh` does not implement all of this procedure.
It presently changes native profiles, uses broad process cleanup and fixed arm
ordering. Repair those behaviors before using it for these claims. A dry run
of a build plan is not a dry run of privileged benchmark collection.

## Source-build preparation available now

The existing builder now supports `--plan`, `--snapshot YYYYMMDD` and repeatable
`--package-dir DIR`. The base/profile selection stays the same; Cargo now
requires the committed lockfile and the image emits a JSON package manifest. Snapshot and
local package choices use mkosi's existing interfaces; no signature checks are
disabled. The plan returns before compilation, staging or `--clean` deletion.

From the experiment checkout, preview the common base:

```sh
bash tools/build-mkosi-image.sh --edition server --snapshot 20260904 --plan
```

`20260904` is the proposed fixed archive date for this experiment, not a claim
that archive availability was tested here. Before both real builds, validate
the chosen mkosi version and snapshot availability on the build host. Record
`mkosi summary` and the tool version. For a different confirmed snapshot, use
the same date in both arms and record the change before data collection.

Build the selected package from its pinned upstream Arch recipe using the
standard clean-chroot tooling. Retain source/signature verification, toolchain
versions and build flags. Do not install that package on the developer host.
Put the resulting package and metadata in an external experiment directory,
then pass that directory through the existing builder:

```sh
bash tools/build-mkosi-image.sh --edition server --snapshot 20260904 \
  --package-dir /path/to/rebuilt-packages --plan
```

The directory is a supplied build input, not an automatic package rebuild or
proof of package selection. Real builds use the same command without `--plan`
on the supported build host. Confirm the installed package version and payload
hash in the image; a resolver may otherwise choose the upstream package. Use
separate clean checkouts/output directories for each arm and retain both
artifacts before cleanup. Product desktop/laptop images still use
`build-edition-image.sh` with the corresponding `--base-image`; do not pass a
desktop extension profile to the whole-image builder. Freeze the same extension
inputs in both arms and inspect both full manifests.

The first rebuild candidate is chosen from the pilot's profile: kernel only if
scheduler/driver behavior is implicated, compositor only if presentation is the
bottleneck, or a userspace component if its code dominates the measured work.
No component is selected merely because it is easy to compile. A reproduction
build with unchanged settings precedes an optimization change.

## What is complete and what is not

- Complete on this branch: baseline preservation, source-backed foundational
  reassessment, proposed goal correction, build-input preview and local-package
  selection support, and an explicit comparison/decision procedure.
- Local software validation checks argument handling and forwarding through the
  real builder entrypoint using stub compilers/image builders. It cannot prove
  mkosi resolution, a bootable custom image, physical safety or efficiency.
- Upstream mkosi commit `9539f1771a328054df1b1e6138bee4a46ce89b83`
  parsed the real common-base configuration with the snapshot and package-directory
  options in `summary` mode. It reported Arch, snapshot `20260904`, the supplied
  directory and enabled repository signature checks. This checks configuration
  compatibility with that revision, not package resolution or image construction.
- The review environment lacked a Rust toolchain, mkosi, QEMU and connected
  reference laptop at initial inspection. Upstream mkosi was obtained for the
  configuration check above. No physical treatment, package rebuild, image boot
  or new battery/performance measurement is claimed.
- Next executable work: correct owner/restoration and paired collection in the
  existing benchmark path; verify required probes; profile the reference laptop;
  select and build one source intervention. Keep ordinary OS integration moving
  alongside this work, subject only to its actual dependencies.
- CI results belong to the draft PR and exact tested commit. A green build is
  software/build evidence, not automatic hardware or performance certification.

Stop adding optimization mechanisms when the current evidence cannot distinguish
their effect. Resolve that measurement limit first. Keep the baseline if a
candidate has no material benefit or imposes unjustified maintenance cost.
