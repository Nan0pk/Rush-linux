# Runtime observation field-read repair

The runtime-state observability package (`O1`) remains `candidate`. This repair
addresses three findings from the independent [cold verification](2026-09-06-runtime-observability-verification.md), without claiming package completion.

The standalone `optid-observe` reporter now opens the CPU PM QoS interface
directly. An inaccessible ancestor reports `permission_denied`; a missing file
reports `unsupported`. The old existence check erased the distinction before
the read. Per-device resume-latency reads likewise preserve permission and
malformed-input errors in the device status. An absent optional per-device
attribute remains unavailable without degrading otherwise readable runtime PM.

Backlight reporting no longer substitutes requested brightness for a missing
actual-brightness reading. It prints `actual=unavailable status=unsupported`
when that interface is absent, while retaining the requested brightness.

Three new module tests failed against the previous implementation and passed
after the changes. The production integration test compares stable kernel read
errors before and after running the real reporter. It covers the inaccessible
debugfs ancestor on this host and absence on hosts without that interface.
The module suite has 16 passing tests and the production suite has five.

The reporter remains read-only and separate from daemon/safety proof paths.
No physical performance benefit, hardware promotion or passing cold receipt is
claimed. The historical verification report describes the tested base commit;
this repair does not rewrite that independent verdict.

Next agent work: preserve and expose directory-discovery errors, including
previous samples when a source becomes inaccessible. The cold report identifies
the affected collectors. The intentionally unwired `optctl status` consumer
also remains explicit in the ledger. Neither gap is resolved by these field
repairs, and downstream dependencies remain locked.
