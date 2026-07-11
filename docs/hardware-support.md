# Hardware Support

Rush Linux targets mainstream upstream-supported x86_64 and ARM64 hardware
first. "Any system" means practical coverage through upstream Linux support and
hardware-specific policy layers, not one unsafe universal tuning profile.

## Initial Hardware Classes

Required lab classes:

- Intel laptop.
- AMD laptop.
- desktop with discrete GPU.
- low-RAM system.
- NVMe workstation.
- headless server.

## Firmware And Drivers

Practical hardware support may require optional proprietary firmware where
legally redistributable. Firmware policy must be explicit and separable from
the base architecture.

## Hardware Policy

`optid` may apply generic safe policy by default. Riskier device-specific
settings require allowlists, for example:

- GPU runtime power management.
- USB autosuspend overrides.
- NVMe APST changes.
- PCIe ASPM changes.
- Wi-Fi power save.
- platform-specific thermal or fan behavior.

An allowlist entry marked `verified = false` is a candidate, not permission to
write. Rush may detect it, explain it, and collect observations, but automatic
actuation remains denied. Promotion to `verified = true` requires matching
hardware evidence and maintainer approval.

Testing does not need to stop while a device is unverified. Safe paths are:

- normal dry-run and explanation output;
- read-only Rush Doctor or telemetry collection;
- simulation and unit tests;
- an isolated, owner-approved `--apply --once --no-allowlist` experiment on a
  recoverable test machine.

The last path disables the hardware gate for one run only. It must never be a
normal service setting or be treated as proof that the hardware is safe.

## Degraded Operation

Missing or broken firmware interfaces are expected. The correct behavior is to
skip unsupported actions, log the reason, and keep the system stable.

## Acceptance Criteria

Hardware support changes must include:

- safe fallback behavior;
- explainable skipped actions;
- benchmark or test plan updates;
- docs updates when a new hardware class or risky knob is introduced.
