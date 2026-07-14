# v0.5.0-beta.1 — Evidence Directory

Per `docs/agent-protocol.md` (Evidence Rule), each `criteria_status` entry in
`release/milestones.toml` may only be set `verified = true` when a literal
command transcript exists on disk. This directory holds those transcripts for
the four `v0.5.0-beta.1` exit criteria.

## Layout convention

Each subdirectory mirrors the existing `release/evidence/host-bench/<date>-<host>/`
shape:

```
<criterion-slug>/
├── meta.txt        # date, host, kernel, cpu, git_commit, tool versions
├── transcript.log  # literal stdout+stderr of the verification command
└── <artifact>      # optional: csv, image hash, sha256 manifest, etc.
```

`meta.txt` must include at minimum:

```
date=<RFC3339 timestamp>
host=<hostname or build host description>
kernel=<uname -r>
cpu=<cpu model>
git_commit=<short sha being verified>
tool_versions=<mkosi, qemu, systemd-repart, etc.>
```

## Exit criteria → directories

| # | Criterion (from `release/milestones.toml`)  | Directory                        | Verifying command (from `HANDOFF.md`)                           |
|---|---------------------------------------------|----------------------------------|-----------------------------------------------------------------|
| 1 | fresh VM install succeeds                   | `c1-fresh-install/`              | `sudo bash tools/test-install.sh build/rush-linux.raw`          |
| 2 | installed system boots twice cleanly        | `c2-double-boot/`                | `tools/test-double-boot.sh build/rush-linux.raw`                |
| 3 | update and rollback tests pass              | `c3-update-rollback/`            | `tools/test-rollback.sh build/rush-linux.raw`                   |
| 4 | server edition has no desktop dependency    | `c4-server-no-desktop/`          | see `c4-server-no-desktop/meta.txt` for the package-list check  |

## Producer / verifier separation

Per the Authority Matrix in `docs/agent-protocol.md`:

- A **Builder agent** may *produce* an image and *run* its self-checks, but
  may not declare a criterion verified.
- A **Verifier agent** checks out the branch cold, runs the acceptance block
  verbatim, and writes `VERIFICATION.md` (template at
  `docs/templates/VERIFICATION.md`).
- Only the **human maintainer** flips `verified = true` in
  `release/milestones.toml`, with the relative path of the transcript in
  the `note =` field.

Until that happens, the four `c*/` directories below remain placeholders.
