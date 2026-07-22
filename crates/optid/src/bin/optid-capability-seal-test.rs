//! D0 — Experimental capability-sealing prototype.
//!
//! This binary is the D0 package from OPTID-COMPLETION-PLAN.md. It
//! validates the Landlock behavior that the D2 fail-passive architecture
//! depends on:
//!
//! 1. Writes through file descriptors opened **before** Landlock
//!    restriction still succeed.
//! 2. New write opens **after** Landlock restriction are denied.
//! 3. Child threads/processes inherit the restrictions and cannot
//!    escape them.
//! 4. A sysfs object that disappears (hot-unplug) returns a handled
//!    error, not a crash.
//! 5. The process can exit with a dedicated topology-rebuild status
//!    so a supervisor (systemd) can cold-restart it.
//!
//! ## What this is NOT
//!
//! - It is **not** connected to production actuation. It never writes
//!   to real hardware. It uses synthetic temp-dir targets and, when
//!   available, real read-safe sysfs paths.
//! - It is **not** enabled in shipped optid. The binary only compiles
//!   when the `experimental-capability-sealing` Cargo feature is set.
//! - It **does not** select an unsealed fallback. If Landlock is
//!   unavailable, the binary exits with a non-zero status and a clear
//!   message — it never silently runs unsealed.
//!
//! ## Usage
//!
//! ```sh
//! cargo build --features experimental-capability-sealing --bin optid-capability-seal-test
//! ./target/debug/optid-capability-seal-test
//! ```
//!
//! ## Exit codes
//!
//! - `0` — all checks passed; Landlock sealing works on this kernel.
//! - `1` — Landlock is unavailable or a check failed. See stderr.
//! - `75` — topology-rebuild requested. A supervisor should cold-
//!   restart the process. (EX_TEMPFAIL from sysexits.h.)
//!
//! Per AGENTS.md §9, this prototype requires independent cold
//! verification because it validates a security boundary.

#![cfg(feature = "experimental-capability-sealing")]

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process;

#[path = "../capability_seal_test/landlock_syscall.rs"]
mod landlock_syscall;
#[path = "../capability_seal_test/seal_test.rs"]
mod seal_test;

const EXIT_TOPOLOGY_REBUILD: i32 = 75;

fn main() {
    eprintln!("optid-capability-seal-test: D0 experimental prototype");
    eprintln!("  This binary validates Landlock capability sealing for the D2 architecture.");
    eprintln!("  It does NOT write to real hardware. It uses synthetic temp-dir targets.");

    // ── Step 1: detect Landlock ABI ──────────────────────────────
    let abi = match landlock_syscall::detect_landlock_abi() {
        Ok(abi_version) => {
            eprintln!("  Landlock ABI detected: v{abi_version}");
            abi_version
        }
        Err(e) => {
            eprintln!("  FAIL: Landlock not available: {e}");
            eprintln!(
                "  D0 requires Landlock. The D2 architecture does not select an unsealed fallback."
            );
            process::exit(1);
        }
    };

    // ── Step 2: create synthetic targets ─────────────────────────
    let temp_dir = match create_synthetic_targets() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("  FAIL: could not create synthetic targets: {e}");
            process::exit(1);
        }
    };
    eprintln!("  Synthetic targets in: {}", temp_dir.display());

    // ── Step 3: open descriptors BEFORE Landlock ─────────────────
    let pre_opened = match seal_test::open_targets_before_seal(&temp_dir) {
        Ok(handles) => {
            eprintln!("  Pre-opened {} write descriptors", handles.len());
            handles
        }
        Err(e) => {
            eprintln!("  FAIL: could not pre-open descriptors: {e}");
            let _ = fs::remove_dir_all(&temp_dir);
            process::exit(1);
        }
    };

    // ── Step 4: install Landlock ─────────────────────────────────
    match landlock_syscall::install_landlock_restrictions() {
        Ok(()) => eprintln!("  Landlock restrictions installed"),
        Err(e) => {
            eprintln!("  FAIL: could not install Landlock: {e}");
            let _ = fs::remove_dir_all(&temp_dir);
            process::exit(1);
        }
    }

    // ── Step 5: run the sealing checks ───────────────────────────
    let results = seal_test::run_seal_checks(&temp_dir, &pre_opened, abi);

    // ── Step 6: report results ───────────────────────────────────
    let mut all_passed = true;
    for result in &results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        eprintln!("  [{status}] {}: {}", result.name, result.message);
        if !result.passed {
            all_passed = false;
        }
    }

    let _ = fs::remove_dir_all(&temp_dir);

    if all_passed {
        eprintln!("  All D0 checks passed. Landlock sealing works on this kernel.");
        eprintln!(
            "  Receipt: kernel=0x{:x} abi=v{abi} checks={}",
            landlock_syscall::kernel_version(),
            results.len()
        );
        process::exit(0);
    } else {
        eprintln!("  D0 checks failed. See above for details.");
        eprintln!(
            "  This blocks S4D (sealed typed capability table) but not F1–F4 or read-only work."
        );
        process::exit(1);
    }
}

/// Create a temp directory with synthetic "sysfs-like" targets.
fn create_synthetic_targets() -> io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("optid-d0-seal-test-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("control"), "on\n")?;
    fs::write(dir.join("brightness"), "100\n")?;
    fs::create_dir_all(dir.join("power"))?;
    fs::write(dir.join("power").join("autosuspend_delay_ms"), "2000\n")?;
    Ok(dir)
}

#[allow(dead_code)]
const _EXIT_TOPOLOGY_REBUILD: i32 = EXIT_TOPOLOGY_REBUILD;
