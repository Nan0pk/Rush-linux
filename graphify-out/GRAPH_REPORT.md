# Graph Report - Rush-linux  (2026-05-31)

## Corpus Check
- 36 files · ~12,695 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 279 nodes · 346 edges · 37 communities (33 shown, 4 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `ba23f10b`
- Run `git rev-parse HEAD` and compare to check if the graph is stale.
- Run `graphify update .` after code changes (no API cost).

## Community Hubs (Navigation)
- [[_COMMUNITY_Community 0|Community 0]]
- [[_COMMUNITY_Community 1|Community 1]]
- [[_COMMUNITY_Community 2|Community 2]]
- [[_COMMUNITY_Community 3|Community 3]]
- [[_COMMUNITY_Community 4|Community 4]]
- [[_COMMUNITY_Community 5|Community 5]]
- [[_COMMUNITY_Community 6|Community 6]]
- [[_COMMUNITY_Community 7|Community 7]]
- [[_COMMUNITY_Community 8|Community 8]]
- [[_COMMUNITY_Community 9|Community 9]]
- [[_COMMUNITY_Community 10|Community 10]]
- [[_COMMUNITY_Community 11|Community 11]]
- [[_COMMUNITY_Community 12|Community 12]]
- [[_COMMUNITY_Community 13|Community 13]]
- [[_COMMUNITY_Community 14|Community 14]]
- [[_COMMUNITY_Community 15|Community 15]]
- [[_COMMUNITY_Community 16|Community 16]]
- [[_COMMUNITY_Community 17|Community 17]]
- [[_COMMUNITY_Community 18|Community 18]]
- [[_COMMUNITY_Community 19|Community 19]]
- [[_COMMUNITY_Community 20|Community 20]]
- [[_COMMUNITY_Community 21|Community 21]]
- [[_COMMUNITY_Community 22|Community 22]]
- [[_COMMUNITY_Community 23|Community 23]]
- [[_COMMUNITY_Community 24|Community 24]]
- [[_COMMUNITY_Community 25|Community 25]]
- [[_COMMUNITY_Community 26|Community 26]]
- [[_COMMUNITY_Community 27|Community 27]]
- [[_COMMUNITY_Community 28|Community 28]]
- [[_COMMUNITY_Community 29|Community 29]]
- [[_COMMUNITY_Community 30|Community 30]]
- [[_COMMUNITY_Community 32|Community 32]]
- [[_COMMUNITY_Community 33|Community 33]]
- [[_COMMUNITY_Community 34|Community 34]]
- [[_COMMUNITY_Community 35|Community 35]]

## God Nodes (most connected - your core abstractions)
1. `run()` - 12 edges
2. `Roadmap` - 12 edges
3. `Option` - 11 edges
4. `Release Plan To v1.0.0` - 11 edges
5. `Self` - 9 edges
6. `AI Continuation` - 9 edges
7. `Rush Linux` - 9 edges
8. `Mode` - 8 edges
9. `Snapshot` - 8 edges
10. `run()` - 8 edges

## Surprising Connections (you probably didn't know these)
- `Snapshot` --references--> `Option`  [EXTRACTED]
  crates/optid/src/main.rs → crates/optid/src/main.rs  _Bridges community 1 → community 0_

## Import Cycles
- 1-file cycle: `crates/optid/src/main.rs -> crates/optid/src/main.rs`
- 1-file cycle: `crates/optctl/src/main.rs -> crates/optctl/src/main.rs`

## Communities (37 total, 4 thin omitted)

### Community 0 - "Community 0"
Cohesion: 0.23
Nodes (9): Decision, Default, Self, ac_cpu_pressure_prefers_performance(), Action, battery_auto_mode_prefers_battery(), critical_thermal_overrides_performance_bias(), Policy (+1 more)

### Community 1 - "Community 1"
Cohesion: 0.11
Nodes (31): Action, Args, Option, Path, Result, main(), print_usage(), run() (+23 more)

### Community 2 - "Community 2"
Cohesion: 0.15
Nodes (12): Phase 0: Repository Foundation, Roadmap, v0.1.0-alpha.1: Compile-Clean Core, v0.2.0-alpha.1: Real Control Plane, v0.3.0-alpha.1: Rootfs And Package Builder MVP, v0.4.0-alpha.1: UKI, Boot, Rollback, Updates, v0.5.0-beta.1: Minimal Installable System, v0.6.0-beta.1: Hardware-Aware optid (+4 more)

### Community 3 - "Community 3"
Cohesion: 0.17
Nodes (11): Acceptance Rules, Agent Workflow, Code-only refresh: no API tokens, Committed Artifacts, Full semantic refresh: may use backend tokens, GitHub Automation, Graphify Knowledge Graph, Install Graphify (+3 more)

### Community 4 - "Community 4"
Cohesion: 0.17
Nodes (11): Release Plan To v1.0.0, v0.1.0-alpha.1: Compile-Clean Core, v0.2.0-alpha.1: Real Control Plane, v0.3.0-alpha.1: Rootfs And Package Builder MVP, v0.4.0-alpha.1: UKI, Boot, Rollback, Updates, v0.5.0-beta.1: Minimal Installable System, v0.6.0-beta.1: Hardware-Aware optid, v0.7.0-beta.1: Desktop, Laptop, Realtime, Server Editions (+3 more)

### Community 5 - "Community 5"
Cohesion: 0.35
Nodes (10): Option, Path, Result, main(), print_usage(), run(), String, Vec (+2 more)

### Community 6 - "Community 6"
Cohesion: 0.20
Nodes (9): Build, Current Implementation Status, Design Rules, First-Class Documentation, GitHub CI, Knowledge Graph For Continuation, Publishing, Repository Layout (+1 more)

### Community 7 - "Community 7"
Cohesion: 0.20
Nodes (9): AI Continuation, Commands And Checks, Current Status, Forbidden Shortcuts, Graphify Continuation Workflow, Mission, Next Task, Repo Layout (+1 more)

### Community 8 - "Community 8"
Cohesion: 0.22
Nodes (8): Channels, Current Version, Meaning, Package Versions, Pre-Release Labels, Project Versions, Tagging Rules, Versioning

### Community 9 - "Community 9"
Cohesion: 0.29
Nodes (6): Actions, Adaptive Engine, Current MVP, Guardrails, Inputs, Policy Ownership

### Community 10 - "Community 10"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Degraded Operation, Firmware And Drivers, Hardware Policy, Hardware Support, Initial Hardware Classes

### Community 11 - "Community 11"
Cohesion: 0.29
Nodes (6): All Releases, Alpha Releases, Beta Releases, RC Releases, Release Checklist, Stable Release

### Community 12 - "Community 12"
Cohesion: 0.29
Nodes (6): Release Blockers, Release Channels, Release Policy, Required Gates By Channel, Signing And Provenance, Test Tiers

### Community 13 - "Community 13"
Cohesion: 0.29
Nodes (6): Before You Change Code, Commit Quality, Contributing, Defaults Policy, Documentation Is Required, Required Checks

### Community 14 - "Community 14"
Cohesion: 0.29
Nodes (6): Acceptance Rule, Implementation Status, Implemented, Known Local Constraints, Not Yet Implemented, Overall State

### Community 15 - "Community 15"
Cohesion: 0.33
Nodes (5): Architecture, Compatibility Position, Documentation Rule, Subsystems, System Boundaries

### Community 16 - "Community 16"
Cohesion: 0.33
Nodes (5): Acceptance Criteria, Boot And Updates, Boot Direction, Rollback Requirements, Update Direction

### Community 17 - "Community 17"
Cohesion: 0.33
Nodes (5): Documentation Policy, Forbidden, Minimum Commit Standard, Required Docs By Change Type, Required For Every Change

### Community 18 - "Community 18"
Cohesion: 0.33
Nodes (5): Acceptance Criteria, Default Kernel, Experimental Scheduler Work, Kernel Policy, Realtime Kernel

### Community 19 - "Community 19"
Cohesion: 0.33
Nodes (5): Build Acceptance Criteria, Current State, Model, Package Backend Direction, Packaging And Builds

### Community 20 - "Community 20"
Cohesion: 0.33
Nodes (5): Benchmark Manifest, Current Validation, Documentation Gate, Release Gates, Testing And Benchmarks

### Community 21 - "Community 21"
Cohesion: 0.33
Nodes (5): Current Product Shape, Engineering Principles, Mission, Project Brief, Success Criteria

### Community 22 - "Community 22"
Cohesion: 0.40
Nodes (4): ADR 0001: Use systemd With Unified cgroup v2, Consequences, Context, Decision

### Community 23 - "Community 23"
Cohesion: 0.40
Nodes (4): ADR 0002: Use Wayland And PipeWire By Default, Consequences, Context, Decision

### Community 24 - "Community 24"
Cohesion: 0.40
Nodes (4): ADR 0003: Use UKI-First Boot With Rollback, Consequences, Context, Decision

### Community 25 - "Community 25"
Cohesion: 0.40
Nodes (4): ADR 0004: Make optid The Adaptive Policy Owner, Consequences, Context, Decision

### Community 26 - "Community 26"
Cohesion: 0.40
Nodes (4): ADR 0005: Avoid Obsolete Defaults, Consequences, Context, Decision

### Community 27 - "Community 27"
Cohesion: 0.40
Nodes (4): Reporting, Security Policy, Security Requirements, Supported Versions

### Community 28 - "Community 28"
Cohesion: 0.47
Nodes (4): graphify-refresh.sh script, graphify-refresh.sh script, PYTHONHASHSEED, run_graphify()

### Community 29 - "Community 29"
Cohesion: 0.50
Nodes (3): Compatibility Is Not Default, Non-Goals, Not Goals

### Community 30 - "Community 30"
Cohesion: 0.50
Nodes (3): Release Ledger, Release Rule, Releases

## Knowledge Gaps
- **149 isolated node(s):** `build-rootfs.sh script`, `PYTHONHASHSEED`, `Args`, `I`, `Display` (+144 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **4 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Mode` connect `Community 1` to `Community 0`?**
  _High betweenness centrality (0.004) - this node is a cross-community bridge._
- **Why does `run()` connect `Community 1` to `Community 0`?**
  _High betweenness centrality (0.004) - this node is a cross-community bridge._
- **What connects `build-rootfs.sh script`, `PYTHONHASHSEED`, `Args` to the rest of the system?**
  _149 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 1` be split into smaller, more focused modules?**
  _Cohesion score 0.11265969802555169 - nodes in this community are weakly interconnected._