#!/usr/bin/env python3
"""Apply final fail-closed corrections after assembling the staged S5D source."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one correction target, found {count}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


replace_once(
    "crates/optid/src/main.rs",
    "use envelope::{ActionOutcome, ControlCycleEnvelope, CycleIdGenerator};",
    "use envelope::{ActionOutcome, ControlCycleEnvelope, CycleIdGenerator, WriteOutcome};",
)

replace_once(
    "crates/optid/src/circuit_breaker.rs",
    "use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};",
    "use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};",
)
replace_once(
    "crates/optid/src/circuit_breaker.rs",
    "    Runtime,\n    ProcessWide,\n",
    "    Runtime,\n",
)
replace_once(
    "crates/optid/src/circuit_breaker.rs",
    '            Self::Runtime => "runtime",\n            Self::ProcessWide => "process_wide",\n',
    '            Self::Runtime => "runtime",\n',
)
replace_once(
    "crates/optid/src/circuit_breaker.rs",
    """                let permissions_ok = fs::metadata(&path)
                    .map(|metadata| metadata.permissions().mode() & 0o077 == 0)
                    .unwrap_or(false);""",
    """                let permissions_ok = fs::metadata(&path)
                    .map(|metadata| {
                        let private = metadata.permissions().mode() & 0o077 == 0;
                        let root_owned = path != Path::new(PERSISTENT_CIRCUIT_FILE)
                            || metadata.uid() == 0;
                        private && root_owned
                    })
                    .unwrap_or(false);""",
)
replace_once(
    "crates/optid/src/circuit_breaker.rs",
    """        self.state.last_seen_at = self.state.last_seen_at.max(now);
        let key = record_key(scope, class);""",
    """        self.state.last_seen_at = self.state.last_seen_at.max(now);
        if permit == CircuitPermit::Canary {
            for key in self.matching_record_keys(scope) {
                if let Some(existing) = self.state.records.get_mut(&key) {
                    existing.open = true;
                    existing.opened_at = now;
                    existing.cooldown_until = now.saturating_add(self.config.cooldown_secs);
                    existing.recovery_verified = false;
                    existing.canary_in_flight = false;
                }
            }
        }
        let key = record_key(scope, class);""",
)
replace_once(
    "crates/optid/src/circuit_breaker.rs",
    """        assert!(transition.opened);
        assert_eq!(
            breaker.decide(&scope, 401).expect("decide").permit,
            CircuitPermit::Suppressed
        );
    }

    #[test]
    fn s5d_firmware_change_uses_independent_scope()""",
    """        assert!(transition.opened);
        assert_eq!(
            breaker.decide(&scope, 401).expect("decide").permit,
            CircuitPermit::Suppressed
        );
        breaker
            .mark_recovery_success(800)
            .expect("recovery after canary failure");
        assert_eq!(
            breaker.decide(&scope, 800).expect("second canary").permit,
            CircuitPermit::Canary
        );
    }

    #[test]
    fn s5d_firmware_change_uses_independent_scope()""",
)

print("S5D final fail-closed corrections applied")
