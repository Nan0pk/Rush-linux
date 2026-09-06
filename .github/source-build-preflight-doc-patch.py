from pathlib import Path

path = Path("docs/plans/source-build-experiment.md")
text = path.read_text()
old = '''The existing builder now supports `--plan`, `--snapshot YYYYMMDD` and repeatable
`--package-dir DIR`. The base/profile selection stays the same; Cargo now
requires the committed lockfile and the image emits a JSON package manifest. Snapshot and
local package choices use mkosi's existing interfaces; no signature checks are
disabled. The plan returns before compilation, staging or `--clean` deletion.
'''
new = '''The existing builder now supports `--plan`, `--snapshot YYYYMMDD` and repeatable
`--package-dir DIR`. The base/profile selection stays the same; Cargo now
requires the committed lockfile and the image emits a JSON package manifest. Snapshot and
local package choices use mkosi's existing interfaces; no signature checks are
disabled. The plan returns before compilation, staging or `--clean` deletion.
For a real build, the wrapper prints the Cargo and mkosi versions and runs
`mkosi summary` with the exact resolved build arguments before compilation or
staging, so the preflight configuration can be retained with the experiment.
'''
if text.count(old) != 1:
    raise SystemExit(f"expected one source-build preparation paragraph, found {text.count(old)}")
path.write_text(text.replace(old, new))
