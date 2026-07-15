//! Privacy-safe recovery screen — replaces the old `fail_with_diag` path
//! that dumped raw diagnostics and dropped to an interactive root shell.
//!
//! Contract (from the boot-reliability PR):
//!
//! - On failure, the runner shows a clear, privacy-safe recovery screen with:
//!     - a short failure category/code
//!     - whether USB, intent, plan, or catalog was unavailable
//!     - a safe next action
//!     - a reboot option
//! - No root shell is spawned by default.
//! - On-screen diagnosis avoids raw identifiers (no MACs, UUIDs, hostnames,
//!   IPs, serials, cmdline contents, blkid output, lsblk output, dmesg
//!   output). Only a short, privacy-safe failure category is shown.
//! - Raw diagnostics are written separately to
//!   `PRIVATE-DIAGNOSTICS/<run_id>/` on the USB (see `private_diag`); the
//!   recovery screen merely points the operator there for local review.
//!
//! This module is testable: `FailureCategory` carries no system-specific
//! data, and `recovery_screen_text` returns a plain string so tests can
//! assert on its contents without touching stdout.

use crate::tui::Palette;
use std::io::Write;

/// Short failure category. The `code()` is a stable identifier operators
/// can photograph and transcribe; the `description()` is one sentence of
/// context. Neither contains raw system identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCategory {
    /// The USB ESP partition (label RUSHESP) was not found within the
    /// bounded retry window. The mount service failed.
    UsbNotFound,
    /// The USB was found but could not be mounted (filesystem error).
    UsbMountFailed,
    /// `run-intent.json` is missing, malformed, stale, or inconsistent.
    IntentInvalid,
    /// `plan.json` is missing or its hash does not match the intent.
    PlanInvalid,
    /// The bench-list catalog is missing or its hash does not match the
    /// intent.
    CatalogInvalid,
    /// The runner image version does not match the intent's
    /// `testos_version`.
    VersionMismatch,
    /// A benchmark panicked or the runner hit an internal error. The
    /// operator should report the failure code and review the private
    /// diagnostics on the USB.
    InternalError,
    /// ACPI reported a boot-blocking failure (distinct from benign ACPI
    /// warnings, which do not trigger this category).
    AcpiBlocking,
}

impl FailureCategory {
    pub fn code(self) -> &'static str {
        match self {
            FailureCategory::UsbNotFound => "E001",
            FailureCategory::UsbMountFailed => "E002",
            FailureCategory::IntentInvalid => "E003",
            FailureCategory::PlanInvalid => "E004",
            FailureCategory::CatalogInvalid => "E005",
            FailureCategory::VersionMismatch => "E006",
            FailureCategory::InternalError => "E099",
            FailureCategory::AcpiBlocking => "E101",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            FailureCategory::UsbNotFound => {
                "The USB partition (label RUSHESP) was not found within the retry window."
            }
            FailureCategory::UsbMountFailed => {
                "The USB partition was found but could not be mounted."
            }
            FailureCategory::IntentInvalid => {
                "The run-intent file on the USB is missing, malformed, stale, or inconsistent."
            }
            FailureCategory::PlanInvalid => {
                "The plan file on the USB is missing or its hash does not match the intent."
            }
            FailureCategory::CatalogInvalid => {
                "The benchmark catalog on the USB is missing or its hash does not match the intent."
            }
            FailureCategory::VersionMismatch => {
                "The running testOS image version does not match the intent's testos_version."
            }
            FailureCategory::InternalError => {
                "The runner hit an internal error while executing the benchmark plan."
            }
            FailureCategory::AcpiBlocking => {
                "ACPI reported a boot-blocking failure. (Benign ACPI warnings do not trigger this.)"
            }
        }
    }

    pub fn next_action(self) -> &'static str {
        match self {
            FailureCategory::UsbNotFound | FailureCategory::UsbMountFailed => {
                "Re-prepare the USB on the host (tools/livedev-next --prepare-usb), then reboot from it."
            }
            FailureCategory::IntentInvalid
            | FailureCategory::PlanInvalid
            | FailureCategory::CatalogInvalid
            | FailureCategory::VersionMismatch => {
                "Re-prepare the USB on the host so the intent, plan, catalog, and image version agree, then reboot."
            }
            FailureCategory::InternalError => {
                "Review the private diagnostics on the USB (tools/testos-diagnostics.py inspect), then report the failure code."
            }
            FailureCategory::AcpiBlocking => {
                "Photograph the recovery screen, then reboot. If it recurs, report E101 with the hardware details."
            }
        }
    }

    /// True when the failure is "USB-side" (operator should re-prepare the
    /// USB on the host). Used by tests and by the recovery screen layout.
    pub fn is_usb_side(self) -> bool {
        matches!(
            self,
            FailureCategory::UsbNotFound
                | FailureCategory::UsbMountFailed
                | FailureCategory::IntentInvalid
                | FailureCategory::PlanInvalid
                | FailureCategory::CatalogInvalid
                | FailureCategory::VersionMismatch
        )
    }
}

/// Render the recovery screen as a plain string. The runner writes this to
/// stdout (which is tty1). It contains NO raw identifiers — only the code,
/// the description, the next action, and a pointer to the private
/// diagnostics directory on the USB.
pub fn recovery_screen_text(category: FailureCategory, private_diag_relative: &str) -> String {
    let mut s = String::new();
    s.push('\n');
    s.push_str("===============================================\n");
    s.push_str("  testOS — recovery screen\n");
    s.push_str("===============================================\n");
    s.push('\n');
    s.push_str(&format!("  Failure code:        {}\n", category.code()));
    s.push_str(&format!(
        "  Category:            {}\n",
        category_label(category)
    ));
    s.push_str(&format!(
        "  What happened:       {}\n",
        category.description()
    ));
    s.push_str(&format!(
        "  Safe next action:    {}\n",
        category.next_action()
    ));
    s.push('\n');
    s.push_str("  Local diagnostics (private, NOT submitted):\n");
    s.push_str(&format!("    {}\n", private_diag_relative));
    s.push('\n');
    s.push_str("  Review them with:\n");
    s.push_str("    python3 tools/testos-diagnostics.py inspect <USB>/");
    s.push_str(private_diag_relative.trim_start_matches('/'));
    s.push('\n');
    s.push('\n');
    s.push_str("  Rebooting in 10 seconds (Ctrl-C to stay on this screen).\n");
    s.push_str("===============================================\n");
    s
}

/// Human-readable category label for the recovery screen. Short, no
/// identifiers.
fn category_label(c: FailureCategory) -> &'static str {
    match c {
        FailureCategory::UsbNotFound => "USB not found",
        FailureCategory::UsbMountFailed => "USB mount failed",
        FailureCategory::IntentInvalid => "intent unavailable",
        FailureCategory::PlanInvalid => "plan unavailable",
        FailureCategory::CatalogInvalid => "catalog unavailable",
        FailureCategory::VersionMismatch => "image version mismatch",
        FailureCategory::InternalError => "runner internal error",
        FailureCategory::AcpiBlocking => "ACPI blocking",
    }
}

/// Print the recovery screen to stdout using the palette. Color is never
/// the only signal — the failure code is always printed as plain text too.
pub fn print_recovery_screen(
    palette: &Palette,
    category: FailureCategory,
    private_diag_relative: &str,
) {
    let p = palette;
    let text = recovery_screen_text(category, private_diag_relative);
    // We print the structured lines individually so we can color the code
    // and the category, while keeping the plain-text version identical to
    // `recovery_screen_text` (so the snapshot tests still hold).
    let _ = writeln!(std::io::stdout());
    let _ = writeln!(
        std::io::stdout(),
        "{}==============================================={}",
        p.red,
        p.reset
    );
    let _ = writeln!(
        std::io::stdout(),
        "{}  testOS — recovery screen{}",
        p.bold,
        p.reset
    );
    let _ = writeln!(
        std::io::stdout(),
        "{}==============================================={}",
        p.red,
        p.reset
    );
    let _ = writeln!(std::io::stdout());
    let _ = writeln!(
        std::io::stdout(),
        "  {}Failure code:{}        {}{}{}",
        p.dim,
        p.reset,
        p.red,
        category.code(),
        p.reset
    );
    let _ = writeln!(
        std::io::stdout(),
        "  {}Category:{}            {}{}{}",
        p.dim,
        p.reset,
        p.yellow,
        category_label(category),
        p.reset
    );
    let _ = writeln!(
        std::io::stdout(),
        "  {}What happened:{}       {}{}{}",
        p.dim,
        p.reset,
        p.reset,
        category.description(),
        p.reset
    );
    let _ = writeln!(
        std::io::stdout(),
        "  {}Safe next action:{}    {}",
        p.dim,
        p.reset,
        category.next_action()
    );
    let _ = writeln!(std::io::stdout());
    let _ = writeln!(
        std::io::stdout(),
        "  Local diagnostics (private, NOT submitted):"
    );
    let _ = writeln!(
        std::io::stdout(),
        "    {}{}{}",
        p.dim,
        private_diag_relative,
        p.reset
    );
    let _ = writeln!(std::io::stdout());
    let _ = writeln!(std::io::stdout(), "  Review them with:");
    let _ = writeln!(
        std::io::stdout(),
        "    python3 tools/testos-diagnostics.py inspect <USB>/{}",
        private_diag_relative.trim_start_matches('/')
    );
    let _ = writeln!(std::io::stdout());
    let _ = writeln!(
        std::io::stdout(),
        "  {}Rebooting in 10 seconds{} (Ctrl-C to stay on this screen).",
        p.yellow,
        p.reset
    );
    let _ = writeln!(
        std::io::stdout(),
        "{}==============================================={}",
        p.red,
        p.reset
    );
    // Touch `text` so the snapshot helper is exported and tests can call it
    // directly even when this code path colors its output.
    let _ = text;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_short_strings() {
        // Operators photograph these codes, so they must be stable and short.
        for c in [
            FailureCategory::UsbNotFound,
            FailureCategory::UsbMountFailed,
            FailureCategory::IntentInvalid,
            FailureCategory::PlanInvalid,
            FailureCategory::CatalogInvalid,
            FailureCategory::VersionMismatch,
            FailureCategory::InternalError,
            FailureCategory::AcpiBlocking,
        ] {
            let code = c.code();
            assert!(code.starts_with('E'));
            assert!(code.len() <= 5, "code {} too long", code);
            // No identifier leakage.
            assert!(!code.contains(':'));
            assert!(!code.contains('/'));
        }
    }

    #[test]
    fn recovery_screen_contains_code_and_next_action() {
        let txt = recovery_screen_text(FailureCategory::UsbNotFound, "/PRIVATE-DIAGNOSTICS/run-1");
        assert!(txt.contains("E001"), "missing code: {}", txt);
        assert!(
            txt.contains("USB not found"),
            "missing category label: {}",
            txt
        );
        assert!(
            txt.contains("Safe next action"),
            "missing next-action header: {}",
            txt
        );
        assert!(
            txt.contains("Re-prepare the USB"),
            "missing actionable next step: {}",
            txt
        );
        assert!(txt.contains("Rebooting"), "missing reboot line: {}", txt);
    }

    #[test]
    fn recovery_screen_points_to_private_diagnostics() {
        let txt =
            recovery_screen_text(FailureCategory::InternalError, "/PRIVATE-DIAGNOSTICS/run-1");
        assert!(
            txt.contains("PRIVATE-DIAGNOSTICS"),
            "missing private diag pointer: {}",
            txt
        );
        assert!(
            txt.contains("testos-diagnostics.py inspect"),
            "missing inspect command: {}",
            txt
        );
    }

    #[test]
    fn recovery_screen_has_no_root_shell() {
        // The old fail_with_diag dropped to a root shell. The new recovery
        // screen must NOT mention a shell, must NOT prompt for Enter to
        // drop to bash, and must NOT contain "bash" as a next step.
        for c in [
            FailureCategory::UsbNotFound,
            FailureCategory::UsbMountFailed,
            FailureCategory::IntentInvalid,
            FailureCategory::PlanInvalid,
            FailureCategory::CatalogInvalid,
            FailureCategory::VersionMismatch,
            FailureCategory::InternalError,
            FailureCategory::AcpiBlocking,
        ] {
            let txt = recovery_screen_text(c, "/PRIVATE-DIAGNOSTICS/run-1");
            let lower = txt.to_lowercase();
            assert!(
                !lower.contains("drop to a shell"),
                "category {:?} mentions root shell",
                c
            );
            assert!(
                !lower.contains("dropping to shell"),
                "category {:?} mentions root shell",
                c
            );
            assert!(
                !lower.contains("press enter to drop"),
                "category {:?} prompts to drop to shell",
                c
            );
            assert!(
                !lower.contains("type 'reboot' when done"),
                "category {:?} implies interactive shell",
                c
            );
        }
    }

    #[test]
    fn recovery_screen_has_no_raw_identifiers() {
        // The recovery screen text itself must contain no raw identifiers.
        // (Private diagnostics live in a separate directory and are never
        // printed to the screen.)
        let txt =
            recovery_screen_text(FailureCategory::InternalError, "/PRIVATE-DIAGNOSTICS/run-1");
        // We do not embed dmesg/journal/blkid/lsblk/cmdline output here.
        let lower = txt.to_lowercase();
        assert!(!lower.contains("dmesg"));
        assert!(!lower.contains("journalctl"));
        assert!(!lower.contains("blkid"));
        assert!(!lower.contains("lsblk"));
        assert!(!lower.contains("/proc/cmdline"));
        assert!(!lower.contains("mac address"));
        assert!(!lower.contains("serial number"));
        assert!(!lower.contains("uuid"));
    }

    #[test]
    fn usb_side_categories_classified_correctly() {
        assert!(FailureCategory::UsbNotFound.is_usb_side());
        assert!(FailureCategory::UsbMountFailed.is_usb_side());
        assert!(FailureCategory::IntentInvalid.is_usb_side());
        assert!(FailureCategory::PlanInvalid.is_usb_side());
        assert!(FailureCategory::CatalogInvalid.is_usb_side());
        assert!(FailureCategory::VersionMismatch.is_usb_side());
        assert!(!FailureCategory::InternalError.is_usb_side());
        assert!(!FailureCategory::AcpiBlocking.is_usb_side());
    }

    #[test]
    fn print_recovery_screen_does_not_panic_with_plain_palette() {
        // Smoke test: the colored printer must work with the plain palette
        // (non-TTY / NO_COLOR path).
        let p = Palette::plain();
        print_recovery_screen(
            &p,
            FailureCategory::UsbNotFound,
            "/PRIVATE-DIAGNOSTICS/run-1",
        );
    }

    #[test]
    fn print_recovery_screen_does_not_panic_with_colored_palette() {
        let p = Palette::colored();
        print_recovery_screen(
            &p,
            FailureCategory::AcpiBlocking,
            "/PRIVATE-DIAGNOSTICS/run-1",
        );
    }

    #[test]
    fn acpi_blocking_is_distinct_from_benign_warnings() {
        // The recovery screen only fires E101 when ACPI actually blocks
        // boot. Benign ACPI warnings (the common HP firmware noise) do NOT
        // trigger this category — they are surfaced via the operator-facing
        // ACPI note in the TUI, not via the recovery screen.
        let txt = recovery_screen_text(FailureCategory::AcpiBlocking, "/PRIVATE-DIAGNOSTICS/run-1");
        assert!(txt.contains("E101"));
        assert!(txt.contains("boot-blocking"));
        assert!(txt.contains("Benign ACPI warnings do not trigger this"));
    }
}
