//! Shared systemd "unset property" handling.
//!
//! `systemctl show` prints the literal string `[not set]` for a property with
//! no value, and a restore is written back as a bare `PROPERTY=` assignment,
//! which resets the value but leaves a drop-in line behind that names the
//! property without giving it one. Treating either of those as a real value
//! is the recurring defect here: it produced a restore that could never
//! verify (`explicit = true` read back against a record saying
//! `explicit = false`), a write systemd rejected outright
//! (`Failed to parse CPUWeight= value '[not set]': Invalid argument`), and a
//! transaction that stayed pending forever, opening the global S5D circuit on
//! the next daemon start.
//!
//! This file is shared, not duplicated, by `#[path]`-inclusion into two
//! separate binary compilations that cannot depend on each other:
//! `reconciler/mod.rs` (the `optid` daemon) and `recovery.rs` (the
//! standalone, policy-free `optid-recover` binary, itself `#[path]`-included
//! from `src/bin/optid-recover.rs`). A fix landed in only one of the two
//! copies is exactly how this defect survived past its first fix — see
//! `docs/inbox/2026-08-22-enforce-run/README.md` and the commit that added
//! this file. Change this file once and both binaries pick it up.

/// systemd's human-readable stand-in for "this property has no value".
const SYSTEMD_UNSET: &str = "[not set]";

/// Is this `systemctl show` output the absence of a value rather than a value?
pub(crate) fn is_unset_placeholder(value: &str) -> bool {
    let value = value.trim();
    value.is_empty() || value == SYSTEMD_UNSET
}

/// Does this drop-in line give `property` an actual value?
///
/// Restoring a property that was never explicitly set is done with an empty
/// assignment (`systemctl set-property --runtime user.slice CPUWeight=`),
/// which resets the value but leaves the drop-in file in place carrying the
/// bare `CPUWeight=` line. Treating that as an explicit setting made the
/// restore unverifiable: recovery wrote the original, read back
/// `explicit = true` against a record saying `explicit = false`, and reported
/// "recovery readback did not match captured original" — permanently, since a
/// retry does the same thing. On any machine where these weights start out
/// unset (all of them), one unclean exit left records that could never be
/// recovered, and the daemon then refused to start at all with
/// `StaleGeneration`.
///
/// An empty assignment is the absence of a value, so it is not explicit.
pub(crate) fn assigns_a_value(line: &str, property: &str) -> bool {
    line.trim_start()
        .strip_prefix(property)
        .and_then(|suffix| suffix.strip_prefix('='))
        .is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systemd_unset_placeholder_is_not_a_value() {
        assert!(is_unset_placeholder("[not set]"));
        assert!(is_unset_placeholder("  [not set]  "));
        assert!(is_unset_placeholder(""));
        assert!(is_unset_placeholder("   "));
    }

    #[test]
    fn a_real_systemd_value_is_a_value() {
        assert!(!is_unset_placeholder("150"));
        assert!(!is_unset_placeholder("infinity"));
        assert!(!is_unset_placeholder("[not set] 150"));
    }

    #[test]
    fn an_empty_assignment_is_not_an_explicit_value() {
        assert!(!assigns_a_value("CPUWeight=", "CPUWeight"));
        assert!(!assigns_a_value("  CPUWeight=  ", "CPUWeight"));
        assert!(!assigns_a_value("CPUWeight=\t", "CPUWeight"));
    }

    #[test]
    fn a_real_assignment_is_explicit() {
        assert!(assigns_a_value("CPUWeight=150", "CPUWeight"));
        assert!(assigns_a_value("  CPUWeight=100", "CPUWeight"));
    }

    #[test]
    fn a_different_property_does_not_match() {
        assert!(!assigns_a_value("IOWeight=150", "CPUWeight"));
        // A property whose name merely starts the same must not match either.
        assert!(!assigns_a_value("CPUWeightFoo=150", "CPUWeight"));
        assert!(!assigns_a_value("[Slice]", "CPUWeight"));
    }
}
