# ADR 0002: Use Wayland And PipeWire By Default

Status: accepted

## Context

The distro needs a desktop stack aligned with current Linux graphics, screen
capture, audio, video routing, sandboxing, and realtime audio direction.

## Decision

Use Wayland-first sessions and PipeWire with WirePlumber by default. KDE Plasma
Wayland is the first polished desktop edition.

## Consequences

- X11 is compatibility only.
- PulseAudio and standalone JACK are compatibility only.
- Realtime audio policy should integrate through PipeWire/RTKit first.
- Desktop edition recipes must keep Wayland and PipeWire as defaults.

