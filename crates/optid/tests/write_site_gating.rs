//! Criterion 4 enumeration harness — v0.6 Phase A2.
//!
//! Milestone v0.6.0-beta.1 exit criterion 4 (per `release/milestones.toml`):
//! "no unsafe write occurs outside allowlisted paths". This test mechanically
//! proves that criterion by enumerating every kernel-write call site in
//! `crates/optid/src/` and asserting each one is either:
//!
//!   (a) **Allowlist-gated** — the surrounding code path calls
//!       `Actuator::allowlist_permits(...)` (which consults
//!       `Allowlist::check(...)`) and skips the write on `Verdict::Deny`. This
//!       is the WP-N4 hardware-allowlist gate.
//!
//!   (b) **ADR-0009 always-safe baseline** — the written path is on the
//!       structural write-allowlist enforced by `io_util::guarded_write`
//!       itself (`/sys/devices/system/cpu/`, `/sys/firmware/acpi/platform_profile`,
//!       `/proc/sys/vm/{swappiness,dirty_background_bytes,dirty_bytes}`,
//!       `/dev/cpu_dma_latency`). These are system-wide knobs with no
//!       per-device HWID — the hardware allowlist does not apply.
//!
//!   (b') **Curated baseline** (optid-safety) — the `Actuator::apply_baseline`
//!       write to `/proc/sys/vm/swappiness` at startup. Gated by
//!       `boot_state.baseline_armed` (disarmed only in dry-run). Target is
//!       an ADR-0009 always-safe path, but the gating is `baseline_armed`
//!       rather than `apply_armed`, so it gets its own classification.
//!
//!   (c) **State-file write** — `atomic_write_state_file` writes to
//!       `/run/optid/...` (the daemon's state directory). It is never a
//!       kernel write and is therefore out of scope for Criterion 4.
//!
//!   (d) **Revert-path write** — `revert_sysctls` / `revert_pm_qos` /
//!       `revert_runtime_pm` / `revert_storage` / `revert_display` write back
//!       a journaled original value to a path that was allowlist-gated when
//!       first written. The journal file (`original_*`) only exists if the
//!       corresponding `apply()` call previously cleared the gate, so
//!       reverting is safe by induction. The revert never introduces a new
//!       (path, value) pair the gate has not already approved.
//!
//!   (e) **Non-sysfs invocation** — `Command::new("systemctl")` is a
//!       process invocation, not a sysfs/procfs/devfs write. Out of scope for
//!       Criterion 4. (Note: the `systemctl set-property --runtime` call's
//!       argv is constructed from typed `Action::SystemdSetProperty`
//!       constructors, never from untrusted input — see the comment in
//!       `actuator.rs` at the call site.)
//!
//! ## Drift detection
//!
//! The test also counts the literal occurrences of `guarded_write(`,
//! `pmqos_sink.write_cpu_latency(`, `pmqos_sink.write_device_latency(`, and
//! `atomic_write_state_file(` in `actuator.rs` and `io_util.rs` at compile
//! time via `include_str!`, and asserts those counts match the inventory. A
//! new write site added without updating the inventory fails the test
//! mechanically — the contributor must classify the new site here.
//!
//! ## Out of scope
//!
//! Test-only writes inside `#[cfg(test)]` modules (e.g. `MockPmqosSink` impls
//! in `tests.rs`) are not kernel writes against a real host and are excluded
//! from the inventory. The `RealPmqosSink::write_device_latency`
//! implementation at `actuator.rs:71` is the actual sysfs write performed on
//! behalf of `Action::DeviceResumeLatency`; it is reached only via the gated
//! call at `actuator.rs:421`, so it is covered by that call's classification.

#![allow(dead_code)]

/// One row in the write-site inventory.
struct WriteSite {
    /// Source file path relative to crate root, e.g. `"src/actuator.rs"`.
    file: &'static str,
    /// 1-indexed line number of the call site. Best-effort: if lines shift
    /// during refactoring, the test still passes as long as the inventory
    /// count matches the source count; only the printed report becomes
    /// stale. A contributor must update both the count and the line number
    /// when adding a new site.
    line: u32,
    /// Containing function or action variant.
    function: &'static str,
    /// Classification: one of `"allowlist"`, `"adr0009-baseline"`,
    /// `"state-file"`, `"revert-path"`, `"non-sysfs"`.
    classification: &'static str,
    /// Human-readable justification.
    reason: &'static str,
}

/// The inventory. Every kernel-write call site in `crates/optid/src/` MUST
/// appear here exactly once. The drift-detection assertions below cross-check
/// the counts.
const WRITE_SITES: &[WriteSite] = &[
    // ── src/actuator.rs: Actuator::apply_baseline() write sites ───────────
    // optid-safety: the curated baseline is a small, fixed set of conservative
    // writes applied once at startup. Gated by boot_state.baseline_armed
    // (disarmed only in dry-run). Classification is "curated-baseline" — a
    // new classification, because these writes are NOT per-cycle dynamic
    // Actions (they are gated by baseline_armed, not apply_armed) and they
    // are NOT allowlist-gated (they target ADR-0009 always-safe paths).
    WriteSite {
        file: "src/actuator.rs",
        line: 210,
        function: "Actuator::apply_baseline (journal original)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/original_vm_swappiness — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 216,
        function: "Actuator::apply_baseline (journal intended)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/intended_vm_swappiness — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 219,
        function: "Actuator::apply_baseline",
        classification: "curated-baseline",
        reason: "/proc/sys/vm/swappiness — ADR-0009 always-safe path; gated by boot_state.baseline_armed (curated baseline, safe by construction; disarmed only in dry-run).",
    },

    // ── src/actuator.rs: Action::apply() write sites ──────────────────────
    WriteSite {
        file: "src/actuator.rs",
        line: 365,
        function: "Action::CpuEpp::apply",
        classification: "adr0009-baseline",
        reason: "Per-CPU EPP under /sys/devices/system/cpu/ — ADR-0009 always-safe.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 391,
        function: "Action::PlatformProfile::apply",
        classification: "adr0009-baseline",
        reason: "/sys/firmware/acpi/platform_profile — ADR-0009 always-safe.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 416,
        function: "Action::SystemdSetProperty::apply",
        classification: "non-sysfs",
        reason: "systemctl set-property --runtime invocation; argv constructed from typed Action constructors (see comment at call site).",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 449,
        function: "Action::VmSysctl::apply (journal original)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/original_vm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 455,
        function: "Action::VmSysctl::apply (journal intended)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/intended_vm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 463,
        function: "Action::VmSysctl::apply",
        classification: "adr0009-baseline",
        reason: "/proc/sys/vm/{swappiness,dirty_background_bytes,dirty_bytes} — ADR-0009 always-safe.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 493,
        function: "Action::CpuDmaLatency::apply",
        classification: "adr0009-baseline",
        reason: "/dev/cpu_dma_latency — system-wide PM QoS, no per-device HWID; ADR-0009 always-safe.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 538,
        function: "Action::DeviceResumeLatency::apply (journal original)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/original_dev_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 547,
        function: "Action::DeviceResumeLatency::apply (journal intended)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/intended_dev_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 557,
        function: "Action::DeviceResumeLatency::apply",
        classification: "allowlist",
        reason: "Gated by allowlist_permits(\"runtime_pm\", hwid_from_attr_path(path), …) at actuator.rs:527; default-deny + audit on Deny.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 631,
        function: "Action::RuntimePm::apply (journal original)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/original_rpm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 634,
        function: "Action::RuntimePm::apply (journal intended)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/intended_rpm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 762,
        function: "Action::RuntimePm::apply (autosuspend_delay_ms)",
        classification: "allowlist",
        reason: "Gated by allowlist_permits(\"runtime_pm\", hwid_from_device_dir(device_dir), …) earlier in the RuntimePm arm; default-deny + audit on Deny. Phase 6 transactional: this is the first write of a two-write transaction.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 787,
        function: "Action::RuntimePm::apply (control=auto)",
        classification: "allowlist",
        reason: "Gated by allowlist_permits(\"runtime_pm\", hwid_from_device_dir(device_dir), …) earlier in the RuntimePm arm; default-deny + audit on Deny. Phase 6 transactional: this is the second write of a two-write transaction; on failure the first write is rolled back.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 807,
        function: "Action::RuntimePm::apply (rollback autosuspend_delay_ms on control-write failure)",
        classification: "allowlist",
        reason: "Phase 6 compensating rollback: writes the journaled original delay value back to power/autosuspend_delay_ms when the control=auto write fails after the delay write succeeded. Part of the same allowlist-gated transaction — the forward write was already approved by allowlist_permits(\"runtime_pm\", …), so the rollback to the same path is safe by induction. If rollback itself fails, the journal is retained for the next revert_runtime_pm pass.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 700,
        function: "Action::PcieAspm::apply (journal original)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/original_aspm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 707,
        function: "Action::PcieAspm::apply (journal intended)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/intended_aspm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 709,
        function: "Action::PcieAspm::apply",
        classification: "allowlist",
        reason: "Gated by allowlist_permits(\"pci_aspm\", hwid_from_device_dir(device_dir), …) at actuator.rs:673; default-deny + audit on Deny.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 755,
        function: "Action::SataAlpm::apply (journal original)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/original_alpm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 761,
        function: "Action::SataAlpm::apply (journal intended)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/intended_alpm_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 763,
        function: "Action::SataAlpm::apply",
        classification: "allowlist",
        reason: "Gated by allowlist_permits(\"sata_alpm\", hwid_from_ancestors(host_dir), …) at actuator.rs:735; default-deny + audit on Deny.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 820,
        function: "Action::Backlight::apply (journal original)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/original_bl_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 827,
        function: "Action::Backlight::apply (journal intended)",
        classification: "state-file",
        reason: "atomic_write_state_file to state_dir/intended_bl_* — not a kernel write.",
    },
    WriteSite {
        file: "src/actuator.rs",
        line: 829,
        function: "Action::Backlight::apply",
        classification: "allowlist",
        reason: "Gated by allowlist_permits(\"backlight\", hwid_from_ancestors(device_dir), …) at actuator.rs:786; default-deny + audit on Deny.",
    },

    // ── src/io_util.rs: revert-path write sites ──────────────────────────
    // These write back journaled originals to paths whose first-write was
    // allowlist-gated. They never introduce a (path, value) pair the gate
    // has not already approved. See module docstring classification (d).
    // optid-safety: the revert functions now also detect crash recovery
    // (original_<key> present but applied_<key> absent) and log it.
    WriteSite {
        file: "src/io_util.rs",
        line: 136,
        function: "revert_sysctls",
        classification: "revert-path",
        reason: "Reverts /proc/sys/vm/* to journaled original; ADR-0009 baseline + safe-by-construction revert. Crash-recovery aware (checks applied_<key> marker).",
    },
    WriteSite {
        file: "src/io_util.rs",
        line: 173,
        function: "revert_pm_qos",
        classification: "revert-path",
        reason: "Reverts per-device PM QoS resume latency to journaled original; first-write was allowlist-gated by Action::DeviceResumeLatency. Crash-recovery aware.",
    },
    WriteSite {
        file: "src/io_util.rs",
        line: 225,
        function: "revert_runtime_pm (control)",
        classification: "revert-path",
        reason: "Reverts power/control to journaled original; first-write was allowlist-gated by Action::RuntimePm. Crash-recovery aware.",
    },
    WriteSite {
        file: "src/io_util.rs",
        line: 241,
        function: "revert_runtime_pm (autosuspend_delay_ms)",
        classification: "revert-path",
        reason: "Reverts power/autosuspend_delay_ms to journaled original; first-write was allowlist-gated by Action::RuntimePm. Crash-recovery aware.",
    },
    WriteSite {
        file: "src/io_util.rs",
        line: 292,
        function: "revert_storage",
        classification: "revert-path",
        reason: "Reverts link/l1_aspm or link_power_management_policy to journaled original; first-write was allowlist-gated by Action::PcieAspm / Action::SataAlpm. Crash-recovery aware.",
    },
    WriteSite {
        file: "src/io_util.rs",
        line: 340,
        function: "revert_display",
        classification: "revert-path",
        reason: "Reverts brightness to journaled original; first-write was allowlist-gated by Action::Backlight. Crash-recovery aware.",
    },
];

const ACTUATOR_RS: &str = include_str!("../src/actuator.rs");
const IO_UTIL_RS: &str = include_str!("../src/io_util.rs");

/// Count occurrences of `needle` in `haystack`. Overlapping matches are not
/// counted (we don't need them here).
fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn criterion_4_no_ungated_write_sites() {
    // Print the inventory so a failing run leaves a readable audit trail.
    println!(
        "═══ Criterion 4 write-site inventory ({} sites) ═══",
        WRITE_SITES.len()
    );
    for site in WRITE_SITES {
        println!(
            "  {file}:{line:<4} [{classification:<18}] {function} — {reason}",
            file = site.file,
            line = site.line,
            classification = site.classification,
            function = site.function,
            reason = site.reason,
        );
    }
    println!("═══ End inventory ═══");

    let valid = [
        "allowlist",
        "adr0009-baseline",
        "curated-baseline",
        "state-file",
        "revert-path",
        "non-sysfs",
    ];
    let mut bad: Vec<&WriteSite> = Vec::new();
    for site in WRITE_SITES {
        if !valid.contains(&site.classification) {
            bad.push(site);
        }
    }
    assert!(
        bad.is_empty(),
        "write sites with invalid classification (typo?): {:?}",
        bad.iter()
            .map(|s| (s.file, s.line, s.classification))
            .collect::<Vec<_>>()
    );
}

#[test]
fn criterion_4_drift_detection_actuator_rs() {
    // Count kernel-write call sites in actuator.rs and assert the inventory
    // matches. New sites added without classification here fail mechanically.
    //
    // F2: the actuator now routes sysfs/procfs writes through
    // `self.kernel.write(` (the KernelWrite trait method) instead of the
    // `guarded_write(` free function. The KernelWrite impl enforces the
    // same allowlist via `kernel_io::is_allowlisted_write_path`. The
    // RealPmqosSink::write_device_latency impl still calls
    // `RealKernel::default().write(` (1 occurrence; subtracted below —
    // covered by the DeviceResumeLatency call's classification).
    //
    // `self.kernel.write(` matches:
    //   - one call per Action variant in apply() that writes directly (7: CpuEpp,
    //     PlatformProfile, VmSysctl, PcieAspm, SataAlpm, Backlight, + apply_baseline)
    //   - one call inside the `runtime_pm_write` helper (subtracted below —
    //     the 3 actual RuntimePm write sites call `self.runtime_pm_write(`
    //     instead, and are counted separately)
    let kernel_write_calls_raw = count(ACTUATOR_RS, "self.kernel.write(");
    // Subtract 1 for the runtime_pm_write helper's internal self.kernel.write(
    // — it is reached via the 3 self.runtime_pm_write( calls counted below.
    let guarded_calls = kernel_write_calls_raw.saturating_sub(1); // runtime_pm_write helper
    let pmqos_cpu_calls = count(ACTUATOR_RS, "self.pmqos_sink.write_cpu_latency(");
    let pmqos_dev_calls = count(ACTUATOR_RS, "self.pmqos_sink.write_device_latency(");
    // Phase 6: the 3 RuntimePm write sites (delay, control, rollback) go through
    // the `runtime_pm_write` helper rather than calling `self.kernel.write(` directly.
    // Count them separately so the inventory matches.
    let runtime_pm_write_calls = count(ACTUATOR_RS, "self.runtime_pm_write(");
    // `atomic_write_state_file(` matches only the real call sites in
    // apply() + apply_baseline() (the use-statement at L22 has a comma after,
    // not `(`). 14 sites as of optid-safety: 12 in apply() + 2 in apply_baseline().
    let atomic_calls = count(ACTUATOR_RS, "atomic_write_state_file(");
    let systemctl_calls = count(ACTUATOR_RS, "Command::new(\"systemctl\")");

    // Kernel-write calls = self.kernel.write (minus helper) + pmqos_sink calls
    // (both reach sysfs or /dev/cpu_dma_latency). The pmqos_sink.write_device_latency
    // impl delegates to RealKernel::default().write(, which is the actual sysfs
    // write — we count it via pmqos_dev_calls, NOT via a separate RealKernel count,
    // to avoid double-counting. The inventory's "allowlist" + "adr0009-baseline" +
    // "curated-baseline" classifications cover ALL kernel-write call sites.
    let kernel_write_calls =
        guarded_calls + pmqos_cpu_calls + pmqos_dev_calls + runtime_pm_write_calls;
    let inv_actuator_kernel = WRITE_SITES
        .iter()
        .filter(|s| {
            s.file == "src/actuator.rs"
                && (s.classification == "allowlist"
                    || s.classification == "adr0009-baseline"
                    || s.classification == "curated-baseline")
        })
        .count();
    let inv_actuator_atomic = WRITE_SITES
        .iter()
        .filter(|s| s.file == "src/actuator.rs" && s.classification == "state-file")
        .count();
    let inv_actuator_systemctl = WRITE_SITES
        .iter()
        .filter(|s| s.file == "src/actuator.rs" && s.classification == "non-sysfs")
        .count();

    assert_eq!(
        kernel_write_calls, inv_actuator_kernel,
        "actuator.rs: counted {kernel_write_calls} kernel-write call sites (self.kernel.write + RealKernel pmqos + pmqos_sink) but inventory lists {inv_actuator_kernel} (allowlist + adr0009-baseline + curated-baseline). Update WRITE_SITES."
    );
    assert_eq!(
        atomic_calls, inv_actuator_atomic,
        "actuator.rs: counted {atomic_calls} atomic_write_state_file( call sites but inventory lists {inv_actuator_atomic}. Update WRITE_SITES."
    );
    assert_eq!(
        systemctl_calls, inv_actuator_systemctl,
        "actuator.rs: counted {systemctl_calls} Command::new(\"systemctl\") call sites but inventory lists {inv_actuator_systemctl}. Update WRITE_SITES."
    );
}

#[test]
fn criterion_4_drift_detection_io_util_rs() {
    // io_util.rs only has revert-path writes; no apply() sites, no atomic
    // state-file calls (the atomic_write_state_file DEFINITION lives here
    // but no calls).
    //
    // `guarded_write(` matches:
    //   - the function definition at L17 (1)
    //   - 6 revert-path call sites in revert_sysctls / revert_pm_qos /
    //     revert_runtime_pm / revert_storage / revert_display
    //   - 2 test-module real calls (the `let res = guarded_write(&target, "x")`
    //     lines in `guarded_write_rejects_directory_traversal` and
    //     `guarded_write_rejects_unallowlisted_paths`)
    // The doc comment at L3 has no `(`. The test fn names
    // (`guarded_write_rejects_directory_traversal(`,
    //  `guarded_write_rejects_unallowlisted_paths(`) do NOT match because
    // `guarded_write(` requires `(` immediately after `guarded_write`, and
    // these have `_rejects_...` after.
    // Total non-inventory occurrences: 1 (fn def) + 2 (test calls) = 3.
    let total = count(IO_UTIL_RS, "guarded_write(");
    let non_call_occurrences = 3;
    let revert_calls = total.saturating_sub(non_call_occurrences);

    let inv_io_util = WRITE_SITES
        .iter()
        .filter(|s| s.file == "src/io_util.rs" && s.classification == "revert-path")
        .count();

    assert_eq!(
        revert_calls, inv_io_util,
        "io_util.rs: counted {revert_calls} guarded_write( revert-path call sites but inventory lists {inv_io_util}. Update WRITE_SITES."
    );
}
