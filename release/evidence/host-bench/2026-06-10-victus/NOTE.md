# Note (added by Dragnet-001, 2026-06-22)

This host benchmark is **not** evidence for any milestone exit criterion, and it
has two capture defects — do not treat it as milestone verification:

1. `meta.txt` field `optid_version=` recorded the binary's `--help`/usage text
   (`Usage: optid [--apply] ...`) instead of a version string — `optid` has no
   `--version` flag at the time of capture.
2. `transcript.log` begins mid-line (`atrix] surface: ...`) because the capturing
   command stripped the leading `[m` of an ANSI `[matrix]` marker (a `tee`/ANSI
   artifact).

It is retained as a historical ambient-telemetry sample only. Dragnet's `meta.txt`
template records real values and is sanity-checked by the evidence gate so this
class of defect does not recur.
