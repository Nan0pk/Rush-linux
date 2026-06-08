# Graph Report - Rush-linux  (2026-06-08)

## Corpus Check
- 64 files · ~30,905 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 603 nodes · 746 edges · 74 communities (69 shown, 5 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS · INFERRED: 1 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Graph Freshness
- Built from commit: `68306517`
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
- [[_COMMUNITY_Community 31|Community 31]]
- [[_COMMUNITY_Community 32|Community 32]]
- [[_COMMUNITY_Community 33|Community 33]]
- [[_COMMUNITY_Community 34|Community 34]]
- [[_COMMUNITY_Community 35|Community 35]]
- [[_COMMUNITY_Community 37|Community 37]]
- [[_COMMUNITY_Community 38|Community 38]]
- [[_COMMUNITY_Community 39|Community 39]]
- [[_COMMUNITY_Community 40|Community 40]]
- [[_COMMUNITY_Community 41|Community 41]]
- [[_COMMUNITY_Community 42|Community 42]]
- [[_COMMUNITY_Community 43|Community 43]]
- [[_COMMUNITY_Community 44|Community 44]]
- [[_COMMUNITY_Community 45|Community 45]]
- [[_COMMUNITY_Community 46|Community 46]]
- [[_COMMUNITY_Community 49|Community 49]]
- [[_COMMUNITY_Community 50|Community 50]]
- [[_COMMUNITY_Community 51|Community 51]]
- [[_COMMUNITY_Community 52|Community 52]]
- [[_COMMUNITY_Community 53|Community 53]]
- [[_COMMUNITY_Community 54|Community 54]]
- [[_COMMUNITY_Community 55|Community 55]]
- [[_COMMUNITY_Community 56|Community 56]]
- [[_COMMUNITY_Community 57|Community 57]]
- [[_COMMUNITY_Community 58|Community 58]]
- [[_COMMUNITY_Community 59|Community 59]]
- [[_COMMUNITY_Community 60|Community 60]]
- [[_COMMUNITY_Community 61|Community 61]]
- [[_COMMUNITY_Community 62|Community 62]]
- [[_COMMUNITY_Community 63|Community 63]]
- [[_COMMUNITY_Community 64|Community 64]]
- [[_COMMUNITY_Community 65|Community 65]]
- [[_COMMUNITY_Community 66|Community 66]]
- [[_COMMUNITY_Community 68|Community 68]]
- [[_COMMUNITY_Community 69|Community 69]]
- [[_COMMUNITY_Community 70|Community 70]]
- [[_COMMUNITY_Community 71|Community 71]]

## God Nodes (most connected - your core abstractions)
1. `run()` - 14 edges
2. `String` - 13 edges
3. `Option` - 12 edges
4. `Roadmap` - 12 edges
5. `Release Plan To v1.0.0` - 12 edges
6. `Roadmap` - 12 edges
7. `Result` - 11 edges
8. `Contributing to Rush Linux` - 11 edges
9. `run()` - 10 edges
10. `Self` - 10 edges

## Surprising Connections (you probably didn't know these)
- `format_status_as_json()` --calls--> `parse_pressure()`  [INFERRED]
  crates/optctl/src/main.rs → crates/optid/src/main.rs

## Import Cycles
- 1-file cycle: `crates/optctl/src/main.rs -> crates/optctl/src/main.rs`
- 1-file cycle: `crates/optid/src/main.rs -> crates/optid/src/main.rs`

## Communities (74 total, 5 thin omitted)

### Community 0 - "Community 0"
Cohesion: 0.15
Nodes (12): Phase 0: Repository Foundation, Roadmap, v0.1.0-alpha.1: Compile-Clean Core, v0.2.0-alpha.1: Real Control Plane, v0.3.0-alpha.1: Rootfs And Package Builder MVP, v0.4.0-alpha.1: UKI, Boot, Rollback, Updates, v0.5.0-beta.1: Minimal Installable System, v0.6.0-beta.1: Hardware-Aware optid (+4 more)

### Community 1 - "Community 1"
Cohesion: 0.08
Nodes (47): Action, Args, Option, Path, Result, main(), print_usage(), run() (+39 more)

### Community 2 - "Community 2"
Cohesion: 0.15
Nodes (12): Phase 0: Repository Foundation, Roadmap, v0.1.0-alpha.1: Compile-Clean Core, v0.2.0-alpha.1: Real Control Plane, v0.3.0-alpha.1: Rootfs And Package Builder MVP, v0.4.0-alpha.1: UKI, Boot, Rollback, Updates, v0.5.0-beta.1: Minimal Installable System, v0.6.0-beta.1: Hardware-Aware optid (+4 more)

### Community 3 - "Community 3"
Cohesion: 0.15
Nodes (11): Acceptance Rules, Agent Workflow, Code-only refresh: no API tokens, Committed Artifacts, Full semantic refresh: may use backend tokens, GitHub Automation, Graphify Knowledge Graph, Install Graphify (+3 more)

### Community 4 - "Community 4"
Cohesion: 0.15
Nodes (11): Release Plan To v1.0.0, v0.1.0-alpha.1: Compile-Clean Core, v0.2.0-alpha.1: Real Control Plane, v0.3.0-alpha.1: Rootfs And Package Builder MVP, v0.4.0-alpha.1: UKI, Boot, Rollback, Updates, v0.5.0-beta.1: Minimal Installable System, v0.6.0-beta.1: Hardware-Aware optid, v0.7.0-beta.1: Desktop, Laptop, Realtime, Server Editions (+3 more)

### Community 5 - "Community 5"
Cohesion: 0.28
Nodes (14): Option, Path, Result, main(), print_usage(), run(), String, Vec (+6 more)

### Community 6 - "Community 6"
Cohesion: 0.20
Nodes (9): Build, Current Implementation Status, Design Rules, First-Class Documentation, GitHub CI, Knowledge Graph For Continuation, Publishing, Repository Layout (+1 more)

### Community 7 - "Community 7"
Cohesion: 0.20
Nodes (9): AI Continuation, Commands And Checks, Current Status, Forbidden Shortcuts, Graphify Continuation Workflow, Mission, Next Task, Repo Layout (+1 more)

### Community 8 - "Community 8"
Cohesion: 0.20
Nodes (8): Channels, Current Version, Meaning, Package Versions, Pre-Release Labels, Project Versions, Tagging Rules, Versioning

### Community 9 - "Community 9"
Cohesion: 0.25
Nodes (6): Actions, Adaptive Engine, Current MVP, Guardrails, Inputs, Policy Ownership

### Community 10 - "Community 10"
Cohesion: 0.25
Nodes (6): Acceptance Criteria, Degraded Operation, Firmware And Drivers, Hardware Policy, Hardware Support, Initial Hardware Classes

### Community 11 - "Community 11"
Cohesion: 0.25
Nodes (6): All Releases, Alpha Releases, Beta Releases, RC Releases, Release Checklist, Stable Release

### Community 12 - "Community 12"
Cohesion: 0.25
Nodes (6): Release Blockers, Release Channels, Release Policy, Required Gates By Channel, Signing And Provenance, Test Tiers

### Community 13 - "Community 13"
Cohesion: 0.29
Nodes (6): Before You Change Code, Commit Quality, Contributing, Defaults Policy, Documentation Is Required, Required Checks

### Community 14 - "Community 14"
Cohesion: 0.29
Nodes (6): Acceptance Rule, Implementation Status, Implemented, Known Local Constraints, Not Yet Implemented, Overall State

### Community 15 - "Community 15"
Cohesion: 0.29
Nodes (5): Architecture, Compatibility Position, Documentation Rule, Subsystems, System Boundaries

### Community 16 - "Community 16"
Cohesion: 0.29
Nodes (5): Acceptance Criteria, Boot And Updates, Boot Direction, Rollback Requirements, Update Direction

### Community 17 - "Community 17"
Cohesion: 0.25
Nodes (6): Doc Management System, Documentation Policy, Forbidden, Minimum Commit Standard, Required Docs By Change Type, Required For Every Change

### Community 18 - "Community 18"
Cohesion: 0.29
Nodes (5): Acceptance Criteria, Default Kernel, Experimental Scheduler Work, Kernel Policy, Realtime Kernel

### Community 19 - "Community 19"
Cohesion: 0.25
Nodes (6): Build Acceptance Criteria, Current State, Model, Package Backend Direction, Packaging And Builds, Recipe Schema Versioning

### Community 20 - "Community 20"
Cohesion: 0.29
Nodes (5): Benchmark Manifest, Current Validation, Documentation Gate, Release Gates, Testing And Benchmarks

### Community 21 - "Community 21"
Cohesion: 0.33
Nodes (5): Current Product Shape, Engineering Principles, Mission, Project Brief, Success Criteria

### Community 22 - "Community 22"
Cohesion: 0.33
Nodes (4): ADR 0001: Use systemd With Unified cgroup v2, Consequences, Context, Decision

### Community 23 - "Community 23"
Cohesion: 0.33
Nodes (4): ADR 0002: Use Wayland And PipeWire By Default, Consequences, Context, Decision

### Community 24 - "Community 24"
Cohesion: 0.33
Nodes (4): ADR 0003: Use UKI-First Boot With Rollback, Consequences, Context, Decision

### Community 25 - "Community 25"
Cohesion: 0.29
Nodes (5): ADR 0004: Make optid The Adaptive Policy Owner, Boundary clarification (2026-06), Consequences, Context, Decision

### Community 26 - "Community 26"
Cohesion: 0.33
Nodes (4): ADR 0005: Avoid Obsolete Defaults, Consequences, Context, Decision

### Community 27 - "Community 27"
Cohesion: 0.40
Nodes (4): Reporting, Security Policy, Security Requirements, Supported Versions

### Community 28 - "Community 28"
Cohesion: 0.46
Nodes (6): graphify-refresh.sh script, graphify-refresh.sh script, graphify-refresh.sh script, find_graphify(), PYTHONHASHSEED, run_graphify()

### Community 29 - "Community 29"
Cohesion: 0.40
Nodes (3): Compatibility Is Not Default, Non-Goals, Not Goals

### Community 30 - "Community 30"
Cohesion: 0.50
Nodes (3): Release Ledger, Release Rule, Releases

### Community 31 - "Community 31"
Cohesion: 0.60
Nodes (3): Assert-Contains(), Assert-File(), Assert-NotContains()

### Community 35 - "Community 35"
Cohesion: 0.40
Nodes (3): build-rootfs.sh script, build-rootfs.sh script, build-rootfs.sh script

### Community 37 - "Community 37"
Cohesion: 0.17
Nodes (11): AI Continuation, Before making any changes, Commands And Checks, Current Status, Forbidden Shortcuts, Graphify Continuation Workflow, Mission, Next Task (+3 more)

### Community 38 - "Community 38"
Cohesion: 0.18
Nodes (10): Build, Community, Current Implementation Status, Design Rules, First-Class Documentation, GitHub CI, Knowledge Graph For Continuation, Publishing (+2 more)

### Community 39 - "Community 39"
Cohesion: 0.11
Nodes (18): 1. Get the code, 2. Start a work session, 3. Build, 4. Make a change, 5. Validate and finish, Code of Conduct, Communication, Contributing to Rush Linux (+10 more)

### Community 40 - "Community 40"
Cohesion: 0.29
Nodes (6): Acceptance Rule, Implementation Status, Implemented, Known Local Constraints, Not Yet Implemented, Overall State

### Community 41 - "Community 41"
Cohesion: 0.33
Nodes (5): Current Product Shape, Engineering Principles, Mission, Project Brief, Success Criteria

### Community 42 - "Community 42"
Cohesion: 0.33
Nodes (5): ADR 0006: Integrate Latency-Focused Performance Tweaks, Amendment 2026-06: Resolve ADR 0004 conflict, Consequences, Context, Decision

### Community 43 - "Community 43"
Cohesion: 0.40
Nodes (4): Reporting, Security Policy, Security Requirements, Supported Versions

### Community 44 - "Community 44"
Cohesion: 0.50
Nodes (3): Release Ledger, Release Rule, Releases

### Community 45 - "Community 45"
Cohesion: 0.22
Nodes (8): 1. Start: `bash tools/start-work.sh "what you're about to do"`, 2. Work: Make your changes, 3. Finish: `bash tools/finish-work.sh "commit message"`, Agent Instructions, Doc Management (REQUIRED), graphify, If you must leave mid-work, Session Lifecycle (MANDATORY)

### Community 46 - "Community 46"
Cohesion: 0.35
Nodes (10): check_schema_version(), cmd_build(), cmd_build_uki(), cmd_repo_init(), cmd_rootfs_create(), cmd_vm_image(), helper_extract_from_deb(), main() (+2 more)

### Community 49 - "Community 49"
Cohesion: 0.83
Nodes (3): download_with_progress(), get_sha256(), main()

### Community 50 - "Community 50"
Cohesion: 0.29
Nodes (6): ADR 0009: optid Security Boundary And Threat Model, Consequences, Context, Decision (proposed), Follow-ups, Threat model

### Community 51 - "Community 51"
Cohesion: 0.29
Nodes (6): Adding a new ADR, Architecture Decision Records, Current proposed ADRs awaiting ratification, Lifecycle and states, Ratifying a proposed ADR, Who ratifies

### Community 52 - "Community 52"
Cohesion: 0.33
Nodes (5): ADR 0008: Software Delivery And Packaging Strategy, Alternatives considered, Consequences, Context, Decision (proposed)

### Community 53 - "Community 53"
Cohesion: 0.33
Nodes (5): C1 — Contributor model, governance, community, C2 — Canonical development environment is Linux, C8 — Hardware test lab (required for beta, T3), Cross-reference, Project Sustainability

### Community 54 - "Community 54"
Cohesion: 0.40
Nodes (4): AD-0001: Expert-review remediation, batch 1, Changes, Decisions, Follow-ups

### Community 55 - "Community 55"
Cohesion: 0.40
Nodes (4): ADR 0007: Project And D-Bus Naming, Consequences, Context, Decision

### Community 56 - "Community 56"
Cohesion: 0.40
Nodes (4): ADR 0010: Realtime Edition Kernel Policy, Consequences, Context, Decision (proposed)

### Community 57 - "Community 57"
Cohesion: 0.40
Nodes (4): ADR 0011: Benchmark Methodology And Baselines, Consequences, Context, Decision (proposed)

### Community 58 - "Community 58"
Cohesion: 0.40
Nodes (4): ADR 0012: Reproducible Build Discipline, Consequences, Context, Decision (proposed)

### Community 59 - "Community 59"
Cohesion: 0.40
Nodes (4): ADR 0013: Workload Detection And The ML Boundary, Consequences, Context, Decision (proposed)

### Community 60 - "Community 60"
Cohesion: 0.50
Nodes (3): Agent Decision Log, Format, When to add an entry

### Community 61 - "Community 61"
Cohesion: 0.50
Nodes (3): Code of Conduct, Enforcement, Our pledge

### Community 62 - "Community 62"
Cohesion: 0.09
Nodes (22): 1. Set Up Your Environment, 2. Find Something to Work On, 3. Understand the Codebase, 4. Make Your Change, 5. Open a Pull Request, 6. Celebrate!, Build System, Clone and Build (+14 more)

### Community 63 - "Community 63"
Cohesion: 0.25
Nodes (7): Checklist, Documentation, If no doc update is needed, explain why:, Motivation, Testing, Type of Change, What Does This Change?

### Community 64 - "Community 64"
Cohesion: 0.25
Nodes (7): Actual Behavior, Additional Context, Describe the Bug, Environment, Expected Behavior, Relevant Logs, Steps to Reproduce

### Community 65 - "Community 65"
Cohesion: 0.29
Nodes (6): Acceptance Criteria, Affected Subsystems, Alternatives Considered, Problem Statement, Proposed Solution, Related

### Community 66 - "Community 66"
Cohesion: 0.50
Nodes (3): Context, Question, Related Documentation

### Community 68 - "Community 68"
Cohesion: 0.20
Nodes (24): check_adr_status(), check_all_docs_exist(), check_deps_exist(), check_docmap_loads(), check_last_verified(), check_markdown_links(), check_optid_doc_sync(), check_stale_patterns() (+16 more)

### Community 69 - "Community 69"
Cohesion: 0.12
Nodes (15): 1. Doc Registry: `docs/docmap.toml`, 2. Automated Sync Validator: `tools/validate-doc-sync.py`, 3. CI Integration, Adding a new ADR, Adding a new doc, Bumping the version, Changing kernel config, Changing `optid` behavior (+7 more)

## Knowledge Gaps
- **312 isolated node(s):** `Optid`, `Vec`, `Option`, `Args`, `I` (+307 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **5 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `parse_pressure()` connect `Community 1` to `Community 5`?**
  _High betweenness centrality (0.007) - this node is a cross-community bridge._
- **Why does `format_status_as_json()` connect `Community 5` to `Community 1`?**
  _High betweenness centrality (0.006) - this node is a cross-community bridge._
- **What connects `Optid`, `Vec`, `Option` to the rest of the system?**
  _322 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Community 1` be split into smaller, more focused modules?**
  _Cohesion score 0.08181126331811263 - nodes in this community are weakly interconnected._
- **Should `Community 39` be split into smaller, more focused modules?**
  _Cohesion score 0.10526315789473684 - nodes in this community are weakly interconnected._
- **Should `Community 62` be split into smaller, more focused modules?**
  _Cohesion score 0.08695652173913043 - nodes in this community are weakly interconnected._
- **Should `Community 69` be split into smaller, more focused modules?**
  _Cohesion score 0.125 - nodes in this community are weakly interconnected._