# Benchmarks Results Directory

This directory stores the measurement results produced by the `rushbench` measurement rig.

## Directory Layout

Results are structured hierarchically:
`benchmarks/results/<UTC-date>/<host-fingerprint>/<class>/<workload>.json`

Where:
- `<UTC-date>`: The date the run was performed (format: `YYYY-MM-DD`).
- `<host-fingerprint>`: The hostname or identifier of the system.
- `<class>`: The SPEC §1 workload class (e.g. `idle`, `interactive`, `latency-critical`).
- `<workload>`: The workload scenario name (e.g. `cyclictest`, `foreground-launch`).

## Result Files

All result JSON files follow `schema_version: 1` as defined in the `rushbench` implementation. These files are committed as human-reviewed snapshots of system behavior, rather than temporary CI artifacts. No agent-fabricated result data should be committed here.
