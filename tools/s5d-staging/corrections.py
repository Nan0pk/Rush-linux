#!/usr/bin/env python3
"""Apply final fail-closed corrections after assembling the staged S5D source."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def replace_once(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one correction target, found {count}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


def regex_replace_once(path: str, pattern: str, replacement: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.DOTALL)
    if count != 1:
        raise SystemExit(f"{path}: expected one regex correction target, found {count}")
    target.write_text(updated, encoding="utf-8")


# Keep circuit-clear commands outside the normal daemon Args contract. This
# preserves every existing Args literal and makes the maintenance operation a
# one-shot pre-parser rather than another control-loop configuration field.
replace_once(
    "crates/optid/src/args.rs",
    """    pub(crate) version: bool,
    pub(crate) clear_all_circuits: bool,
    pub(crate) clear_circuit_domain: Option<String>,
    pub(crate) interval_sec: u64,""",
    """    pub(crate) version: bool,
    pub(crate) interval_sec: u64,""",
)
replace_once(
    "crates/optid/src/args.rs",
    """            help: false,
            version: false,
            clear_all_circuits: false,
            clear_circuit_domain: None,
            interval_sec: DEFAULT_INTERVAL_SEC,""",
    """            help: false,
            version: false,
            interval_sec: DEFAULT_INTERVAL_SEC,""",
)
replace_once(
    "crates/optid/src/args.rs",
    """                \"-V\" | \"--version\" => args.version = true,
                \"--clear-all-circuits\" => args.clear_all_circuits = true,
                \"--clear-circuit-domain\" => {
                    let value = it.next().ok_or_else(|| {
                        \"--clear-circuit-domain requires a domain name\".to_string()
                    })?;
                    args.clear_circuit_domain = Some(value);
                }
                \"--allowlist\" => args.allowlist = true,""",
    """                \"-V\" | \"--version\" => args.version = true,
                \"--allowlist\" => args.allowlist = true,""",
)
replace_once(
    "crates/optid/src/args.rs",
    """        if args.clear_all_circuits && args.clear_circuit_domain.is_some() {
            return Err(
                \"--clear-all-circuits and --clear-circuit-domain are mutually exclusive\"
                    .to_string(),
            );
        }
        if (args.clear_all_circuits || args.clear_circuit_domain.is_some()) && args.apply {
            return Err(\"circuit clearing cannot be combined with --apply\".to_string());
        }

""",
    "",
)
regex_replace_once(
    "crates/optid/src/args.rs",
    r"\n    #\[test\]\n    fn s5d_clear_commands_are_one_shot_and_mutually_exclusive\(\) \{.*?\n    \}\n\n    #\[test\]\n    fn version_flag_is_recognized\(\) \{",
    "\n    #[test]\n    fn version_flag_is_recognized() {",
)

replace_once(
    "crates/optid/src/main.rs",
    "use envelope::{ActionOutcome, ControlCycleEnvelope, CycleIdGenerator};",
    "use envelope::{ActionOutcome, ControlCycleEnvelope, CycleIdGenerator, WriteOutcome};",
)
replace_once(
    "crates/optid/src/main.rs",
    """use circuit_breaker::{
    circuit_runtime_failure_outcome, circuit_suppressed_outcome, CircuitBreaker, CircuitPermit,
    CircuitScope,
};""",
    """use circuit_breaker::{
    circuit_runtime_failure_outcome, circuit_suppressed_outcome, extract_circuit_clear_request,
    CircuitBreaker, CircuitClearRequest, CircuitPermit, CircuitScope,
};""",
)

regex_replace_once(
    "crates/optid/src/main.rs",
    r"fn main\(\) \{.*?\n\}\n\n#\[derive\(Debug, Clone, Copy, PartialEq, Eq\)\]",
    """fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let (clear_request, filtered_args) = match extract_circuit_clear_request(raw_args) {
        Ok(value) => value,
        Err(error) => {
            eprintln!(\"optid: {error}\");
            print_usage();
            std::process::exit(2);
        }
    };
    let parsed = if clear_request.is_some() {
        Args::parse(filtered_args)
    } else {
        parse_from_env()
    };
    let args = match parsed {
        Ok(args) => args,
        Err(error) => {
            eprintln!(\"optid: {error}\");
            print_usage();
            std::process::exit(2);
        }
    };

    if args.help {
        print_usage();
        return;
    }
    if args.version {
        print_version();
        return;
    }
    if let Some(request) = clear_request {
        if let Err(error) = clear_circuits(&args, request) {
            eprintln!(\"optid: {error}\");
            std::process::exit(1);
        }
        return;
    }
    match run(args) {
        Ok(RunExit::Clean) => {}
        Ok(RunExit::TopologyRebuild) => std::process::exit(EXIT_TOPOLOGY_REBUILD),
        Err(error) => {
            eprintln!(\"optid: {error}\");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]""",
)

regex_replace_once(
    "crates/optid/src/main.rs",
    r"fn clear_circuits\(args: &Args\) -> io::Result<\(\)> \{.*?\n\}\n\nfn run\(args: Args\) -> io::Result<RunExit> \{",
    """fn clear_circuits(args: &Args, request: CircuitClearRequest) -> io::Result<()> {
    if args.apply {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            \"circuit clearing cannot be combined with --apply\",
        ));
    }
    let policy = Policy::load(&args.config_path);
    let path = CircuitBreaker::state_path_for(&args.state_dir);
    let mut breaker = CircuitBreaker::load(
        path,
        policy.safety.circuit_failure_threshold,
        policy.safety.circuit_cooldown_sec,
    );
    let effective_uid = unsafe { libc::geteuid() };
    let removed = match request {
        CircuitClearRequest::All => breaker.clear_all(effective_uid)?,
        CircuitClearRequest::Domain(domain) => {
            if !policy::Domain::all()
                .iter()
                .any(|known| known.as_str() == domain)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(\"unknown circuit domain: {domain}\"),
                ));
            }
            breaker.clear_domain(&domain, effective_uid)?
        }
    };
    println!(\"optid: cleared {removed} S5D circuit record(s)\");
    Ok(())
}

fn run(args: Args) -> io::Result<RunExit> {""",
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

# End the mutable record borrow before persistence. The formatted transition
# uses the captured count rather than borrowing the map entry after persist().
circuit_path = ROOT / "crates/optid/src/circuit_breaker.rs"
circuit_text = circuit_path.read_text(encoding="utf-8")
start = circuit_text.index("    fn record_failure(")
end = circuit_text.index("    fn record_success(", start)
record_failure = circuit_text[start:end]
marker = "        self.persist()?;"
if record_failure.count(marker) != 1:
    raise SystemExit("circuit_breaker.rs: expected one record_failure persist")
before, after = record_failure.split(marker, 1)
before += "        let consecutive_failures = record.consecutive_failures;\n"
after = after.replace("record.consecutive_failures", "consecutive_failures")
record_failure = before + marker + after
circuit_path.write_text(
    circuit_text[:start] + record_failure + circuit_text[end:],
    encoding="utf-8",
)

replace_once(
    "crates/optid/src/circuit_breaker.rs",
    """fn stable_fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]""",
    """fn stable_fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CircuitClearRequest {
    Domain(String),
    All,
}

pub(crate) fn extract_circuit_clear_request<I>(
    args: I,
) -> Result<(Option<CircuitClearRequest>, Vec<String>), String>
where
    I: IntoIterator<Item = String>,
{
    let mut request = None;
    let mut remaining = Vec::new();
    let mut iter = args.into_iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            \"--clear-all-circuits\" => {
                if request.is_some() {
                    return Err(
                        \"--clear-all-circuits and --clear-circuit-domain are mutually exclusive\"
                            .to_string(),
                    );
                }
                request = Some(CircuitClearRequest::All);
            }
            \"--clear-circuit-domain\" => {
                let domain = iter.next().ok_or_else(|| {
                    \"--clear-circuit-domain requires a domain name\".to_string()
                })?;
                if request.is_some() {
                    return Err(
                        \"--clear-all-circuits and --clear-circuit-domain are mutually exclusive\"
                            .to_string(),
                    );
                }
                request = Some(CircuitClearRequest::Domain(domain));
            }
            _ => remaining.push(argument),
        }
    }
    Ok((request, remaining))
}

#[cfg(test)]""",
)

replace_once(
    "crates/optid/src/circuit_breaker.rs",
    """    fn verified_outcome() -> ActionOutcome {
        let action = Action::vm_sysctl(""",
    """    #[test]
    fn s5d_clear_commands_are_one_shot_and_mutually_exclusive() {
        let (request, remaining) = extract_circuit_clear_request([
            \"--state-dir\".to_string(),
            \"/tmp/optid\".to_string(),
            \"--clear-circuit-domain\".to_string(),
            \"runtime_pm\".to_string(),
        ])
        .expect(\"extract domain clear\");
        assert_eq!(
            request,
            Some(CircuitClearRequest::Domain(\"runtime_pm\".to_string()))
        );
        assert_eq!(remaining, [\"--state-dir\", \"/tmp/optid\"]);

        let (request, remaining) = extract_circuit_clear_request([
            \"--clear-all-circuits\".to_string(),
        ])
        .expect(\"extract global clear\");
        assert_eq!(request, Some(CircuitClearRequest::All));
        assert!(remaining.is_empty());

        let conflict = extract_circuit_clear_request([
            \"--clear-all-circuits\".to_string(),
            \"--clear-circuit-domain\".to_string(),
            \"runtime_pm\".to_string(),
        ])
        .expect_err(\"clear forms must conflict\");
        assert!(conflict.contains(\"mutually exclusive\"));

        let missing = extract_circuit_clear_request([
            \"--clear-circuit-domain\".to_string(),
        ])
        .expect_err(\"domain value is required\");
        assert!(missing.contains(\"requires a domain name\"));
    }

    fn verified_outcome() -> ActionOutcome {
        let action = Action::vm_sysctl(""",
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
