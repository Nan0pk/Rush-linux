//! Experimental proof for capability sealing and supervisor-managed cold restart.
//!
//! This binary is never enabled in shipped `optid`. It validates the security
//! and lifecycle assumptions required before runtime writes can move to a sealed
//! typed capability table:
//!
//! - ABI-aware Landlock write restrictions;
//! - verified no-new-privileges;
//! - continued use of descriptors opened before sealing;
//! - denial of new write opens after sealing;
//! - inheritance across threads and fork/exec;
//! - safe handling of a genuinely removed object; and
//! - a dedicated topology-rebuild exit followed by supervisor-ordered recovery.

#![cfg(feature = "experimental-capability-sealing")]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "../capability_seal_test/landlock_syscall.rs"]
mod landlock_syscall;
#[path = "../capability_seal_test/seal_test.rs"]
mod seal_test;

const EXIT_FAILURE: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_TOPOLOGY_REBUILD: i32 = 75;
const RECOVERY_MARKER_HEADER: &str = "optid-capability-recovery-v1";

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Probe,
    ExecChildWriteProbe(PathBuf),
    TopologyRebuild,
    RecoveryStep(PathBuf),
    SupervisorCycle { marker: PathBuf, counter: PathBuf },
}

fn main() {
    let mode = match parse_mode(std::env::args_os().skip(1).collect()) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("optid-capability-seal-test: {message}");
            print_usage();
            process::exit(EXIT_USAGE);
        }
    };

    process::exit(run_mode(mode));
}

fn parse_mode(args: Vec<OsString>) -> Result<Mode, String> {
    match args.as_slice() {
        [] => Ok(Mode::Probe),
        [flag] if flag == "--probe" => Ok(Mode::Probe),
        [flag] if flag == "--topology-rebuild" => Ok(Mode::TopologyRebuild),
        [flag, path] if flag == "--exec-child-write-probe" => {
            Ok(Mode::ExecChildWriteProbe(PathBuf::from(path)))
        }
        [flag, marker] if flag == "--recovery-step" => {
            Ok(Mode::RecoveryStep(PathBuf::from(marker)))
        }
        [flag, marker, counter] if flag == "--supervisor-cycle" => Ok(Mode::SupervisorCycle {
            marker: PathBuf::from(marker),
            counter: PathBuf::from(counter),
        }),
        _ => Err("invalid arguments".to_string()),
    }
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  optid-capability-seal-test [--probe]");
    eprintln!("  optid-capability-seal-test --topology-rebuild");
    eprintln!("  optid-capability-seal-test --recovery-step <marker>");
    eprintln!("  optid-capability-seal-test --supervisor-cycle <marker> <counter>");
}

fn run_mode(mode: Mode) -> i32 {
    match mode {
        Mode::Probe => run_capability_probe(),
        Mode::ExecChildWriteProbe(path) => run_exec_child_probe(&path),
        Mode::TopologyRebuild => {
            eprintln!("topology change accepted; requesting supervisor-managed cold restart");
            EXIT_TOPOLOGY_REBUILD
        }
        Mode::RecoveryStep(marker) => match write_recovery_marker(&marker) {
            Ok(()) => {
                eprintln!("recovery step completed: {}", marker.display());
                0
            }
            Err(error) => {
                eprintln!("recovery step failed: {error}");
                EXIT_FAILURE
            }
        },
        Mode::SupervisorCycle { marker, counter } => run_supervisor_cycle(&marker, &counter),
    }
}

fn run_exec_child_probe(path: &Path) -> i32 {
    let no_new_privs = match landlock_syscall::no_new_privs_is_set() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("exec child could not query no-new-privileges: {error}");
            return EXIT_FAILURE;
        }
    };
    let write_denied = match seal_test::new_write_open_is_denied(path) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("exec child received unexpected write-open error: {error}");
            return EXIT_FAILURE;
        }
    };

    if no_new_privs && write_denied {
        eprintln!("exec child inherited no-new-privileges and Landlock write denial");
        0
    } else {
        eprintln!(
            "exec child inheritance failed: no_new_privs={no_new_privs} write_denied={write_denied}"
        );
        EXIT_FAILURE
    }
}

fn run_capability_probe() -> i32 {
    eprintln!("optid capability sealing and cold-restart proof");
    eprintln!("  experimental only; no real hardware writes are performed");

    let abi = match landlock_syscall::detect_landlock_abi() {
        Ok(abi) => abi,
        Err(error) => {
            eprintln!("  FAIL: Landlock is unavailable: {error}");
            eprintln!("  no unsealed fallback is permitted");
            return EXIT_FAILURE;
        }
    };
    let expected_rights = match landlock_syscall::handled_write_rights(abi) {
        Ok(rights) => rights,
        Err(error) => {
            eprintln!("  FAIL: could not construct ABI-aware rights: {error}");
            return EXIT_FAILURE;
        }
    };
    eprintln!("  kernel={} Landlock ABI={abi}", landlock_syscall::kernel_release());
    eprintln!("  handled write rights=0x{expected_rights:x}");

    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("  FAIL: could not resolve current executable: {error}");
            return EXIT_FAILURE;
        }
    };
    let temp_dir = match create_synthetic_targets() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("  FAIL: could not create synthetic targets: {error}");
            return EXIT_FAILURE;
        }
    };
    eprintln!("  synthetic targets={}", temp_dir.display());

    let pre_opened = match seal_test::open_targets_before_seal(&temp_dir) {
        Ok(handles) => handles,
        Err(error) => {
            eprintln!("  FAIL: could not build pre-opened capability table: {error}");
            let _ = fs::remove_dir_all(&temp_dir);
            return EXIT_FAILURE;
        }
    };
    eprintln!("  pre-opened descriptors={}", pre_opened.len());

    let installed_rights = match landlock_syscall::install_landlock_restrictions(abi) {
        Ok(rights) => rights,
        Err(error) => {
            eprintln!("  FAIL: could not install Landlock restrictions: {error}");
            let _ = fs::remove_dir_all(&temp_dir);
            return EXIT_FAILURE;
        }
    };

    let mut results = Vec::new();
    results.push(seal_test::SealTestResult {
        name: "abi_aware_rights",
        passed: installed_rights == expected_rights,
        message: format!(
            "detected ABI {abi}; installed rights 0x{installed_rights:x}; expected 0x{expected_rights:x}"
        ),
    });
    let no_new_privs = landlock_syscall::no_new_privs_is_set().unwrap_or(false);
    results.push(seal_test::SealTestResult {
        name: "no_new_privileges",
        passed: no_new_privs,
        message: format!("PR_GET_NO_NEW_PRIVS={}", i32::from(no_new_privs)),
    });
    results.extend(seal_test::run_seal_checks(
        &temp_dir,
        &pre_opened,
        &current_exe,
    ));

    let mut all_passed = true;
    for result in &results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        eprintln!("  [{status}] {}: {}", result.name, result.message);
        all_passed &= result.passed;
    }

    // Sealing is intentionally irreversible, so this process cannot remove the
    // synthetic directory after the proof. The test-only service uses PrivateTmp
    // and systemd removes that namespace when the process exits.
    eprintln!("  post-seal cleanup delegated to the private temporary namespace");

    if all_passed {
        eprintln!("  capability-sealing proof passed ({} checks)", results.len());
        0
    } else {
        eprintln!("  capability-sealing proof failed");
        EXIT_FAILURE
    }
}

fn run_supervisor_cycle(marker: &Path, counter: &Path) -> i32 {
    if let Err(error) = verify_recovery_marker(marker) {
        eprintln!("recovery ordering failed before capability discovery: {error}");
        return EXIT_FAILURE;
    }
    eprintln!(
        "recovery marker verified before capability discovery: {}",
        marker.display()
    );

    let cycle = match increment_cycle_counter(counter) {
        Ok(cycle) => cycle,
        Err(error) => {
            eprintln!("could not update supervisor cycle counter: {error}");
            return EXIT_FAILURE;
        }
    };

    let probe_status = run_capability_probe();
    if probe_status != 0 {
        return probe_status;
    }

    if cycle == 1 {
        eprintln!("first sealed cycle complete; requesting topology rebuild with status 75");
        EXIT_TOPOLOGY_REBUILD
    } else {
        eprintln!("fresh sealed process started after recovery; supervisor cycle complete");
        0
    }
}

fn write_recovery_marker(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .as_nanos();
    fs::write(
        path,
        format!(
            "{RECOVERY_MARKER_HEADER}\nrecovery_complete=1\npid={}\nunix_nanos={now}\n",
            process::id()
        ),
    )
}

fn verify_recovery_marker(path: &Path) -> io::Result<()> {
    let marker = fs::read_to_string(path)?;
    let mut lines = marker.lines();
    if lines.next() != Some(RECOVERY_MARKER_HEADER)
        || !lines.any(|line| line == "recovery_complete=1")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid recovery marker at {}", path.display()),
        ));
    }
    Ok(())
}

fn increment_cycle_counter(path: &Path) -> io::Result<u32> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let previous = match fs::read_to_string(path) {
        Ok(value) => value.trim().parse::<u32>().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid cycle counter: {error}"),
            )
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    let next = previous
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cycle counter overflow"))?;
    fs::write(path, format!("{next}\n"))?;
    Ok(next)
}

fn create_synthetic_targets() -> io::Result<PathBuf> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "optid-capability-seal-test-{}-{nonce}",
        process::id()
    ));
    fs::create_dir_all(dir.join("power"))?;
    fs::write(dir.join("control"), "on\n")?;
    fs::write(dir.join("brightness"), "100\n")?;
    fs::write(dir.join("power/autosuspend_delay_ms"), "2000\n")?;
    fs::write(dir.join("removed-object"), "present-before-seal\n")?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_topology_rebuild_mode() {
        assert_eq!(
            parse_mode(vec![OsString::from("--topology-rebuild")]).expect("parse mode"),
            Mode::TopologyRebuild
        );
    }

    #[test]
    fn topology_rebuild_mode_returns_status_75() {
        assert_eq!(run_mode(Mode::TopologyRebuild), EXIT_TOPOLOGY_REBUILD);
    }

    #[test]
    fn recovery_marker_round_trip() {
        let path = std::env::temp_dir().join(format!(
            "optid-capability-recovery-test-{}",
            process::id()
        ));
        let _ = fs::remove_file(&path);
        write_recovery_marker(&path).expect("write marker");
        verify_recovery_marker(&path).expect("verify marker");
        fs::remove_file(path).expect("remove marker");
    }

    #[test]
    fn invalid_recovery_marker_is_rejected() {
        let path = std::env::temp_dir().join(format!(
            "optid-capability-recovery-invalid-{}",
            process::id()
        ));
        fs::write(&path, "not-a-recovery-marker\n").expect("write invalid marker");
        assert_eq!(
            verify_recovery_marker(&path)
                .expect_err("invalid marker must fail")
                .kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_file(path).expect("remove marker");
    }
}
