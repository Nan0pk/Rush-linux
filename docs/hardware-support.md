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

## Degraded Operation

Missing or broken firmware interfaces are expected. The correct behavior is to
skip unsupported actions, log the reason, and keep the system stable.

## Acceptance Criteria

Hardware support changes must include:

- safe fallback behavior;
- explainable skipped actions;
- benchmark or test plan updates;
- docs updates when a new hardware class or risky knob is introduced.

