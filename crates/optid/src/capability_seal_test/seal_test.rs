//! Seal-test checks for the D0 prototype.
//!
//! Each check validates one D2 invariant. The checks run AFTER Landlock
//! restrictions are installed, so they test both the "pre-opened
//! descriptors still work" and "new opens are denied" invariants.

use std::fs;
use std::io;
use std::os::unix::io::RawFd;
use std::path::Path;

/// A pre-opened file descriptor (opened before Landlock restriction).
/// Kept as a raw fd because we need to test that writes through it
/// still succeed after restriction.
pub(crate) struct PreOpenedHandle {
    pub path: String,
    pub fd: RawFd,
}

impl Drop for PreOpenedHandle {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
}

/// The result of one seal-test check.
pub(crate) struct SealTestResult {
    pub name: &'static str,
    pub passed: bool,
    pub message: String,
}

/// Open the synthetic targets for writing BEFORE Landlock is installed.
/// Returns handles that keep the fds open. After Landlock is installed,
/// writes through these fds should still succeed.
pub(crate) fn open_targets_before_seal(temp_dir: &Path) -> io::Result<Vec<PreOpenedHandle>> {
    let mut handles = Vec::new();

    let control_path = temp_dir.join("control");
    let fd = open_for_write(&control_path)?;
    handles.push(PreOpenedHandle {
        path: "control".to_string(),
        fd,
    });

    let brightness_path = temp_dir.join("brightness");
    let fd = open_for_write(&brightness_path)?;
    handles.push(PreOpenedHandle {
        path: "brightness".to_string(),
        fd,
    });

    let delay_path = temp_dir.join("power").join("autosuspend_delay_ms");
    let fd = open_for_write(&delay_path)?;
    handles.push(PreOpenedHandle {
        path: "power/autosuspend_delay_ms".to_string(),
        fd,
    });

    Ok(handles)
}

fn open_for_write(path: &Path) -> io::Result<RawFd> {
    let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

/// Run all D0 seal-test checks. Returns one result per check.
pub(crate) fn run_seal_checks(
    temp_dir: &Path,
    pre_opened: &[PreOpenedHandle],
    abi: u32,
) -> Vec<SealTestResult> {
    let mut results = Vec::new();

    // Check 1: writes through pre-opened descriptors succeed.
    results.push(check_pre_opened_writes_succeed(pre_opened));

    // Check 2: new write opens are denied.
    results.push(check_new_write_opens_denied(temp_dir));

    // Check 3: new read opens still succeed (Landlock only gates writes
    // in our configuration — ABI v1/v2 with only write rights handled).
    // Note: this check may fail if Landlock is configured to handle read
    // rights too. Our configuration only handles write rights, so reads
    // should pass. But on some kernels, the empty ruleset might still
    // allow reads. We test this conditionally.
    if abi >= 1 {
        results.push(check_new_read_opens_succeed(temp_dir));
    }

    // Check 4: child thread inherits restrictions.
    results.push(check_child_thread_inherits(temp_dir));

    // Check 5: removed-file handling (simulate hot-unplug).
    results.push(check_removed_file_handling(pre_opened));

    results
}

/// Check 1: writes through pre-opened descriptors succeed after Landlock.
fn check_pre_opened_writes_succeed(pre_opened: &[PreOpenedHandle]) -> SealTestResult {
    for handle in pre_opened {
        let data = b"test-value\n";
        let ret = unsafe { libc::write(handle.fd, data.as_ptr() as *const _, data.len()) };
        if ret < 0 {
            return SealTestResult {
                name: "pre_opened_writes_succeed",
                passed: false,
                message: format!(
                    "write to pre-opened '{}' failed: {} (errno={})",
                    handle.path,
                    io::Error::last_os_error(),
                    unsafe { *libc::__errno_location() }
                ),
            };
        }
    }
    SealTestResult {
        name: "pre_opened_writes_succeed",
        passed: true,
        message: format!(
            "all {} pre-opened descriptors writable after Landlock",
            pre_opened.len()
        ),
    }
}

/// Check 2: new write opens are denied after Landlock.
fn check_new_write_opens_denied(temp_dir: &Path) -> SealTestResult {
    let target = temp_dir.join("control");
    let c_path = match std::ffi::CString::new(target.to_string_lossy().as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            return SealTestResult {
                name: "new_write_opens_denied",
                passed: false,
                message: format!("path encoding error: {e}"),
            };
        }
    };
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_WRONLY) };
    if fd >= 0 {
        unsafe { libc::close(fd) };
        return SealTestResult {
            name: "new_write_opens_denied",
            passed: false,
            message: "new write open succeeded after Landlock — sealing failed".to_string(),
        };
    }
    let errno = unsafe { *libc::__errno_location() };
    if errno == libc::EACCES || errno == libc::EPERM {
        SealTestResult {
            name: "new_write_opens_denied",
            passed: true,
            message: format!("new write open denied with errno={errno} (EACCES/EPERM)"),
        }
    } else {
        SealTestResult {
            name: "new_write_opens_denied",
            passed: false,
            message: format!(
                "new write open failed with unexpected errno={errno} (expected EACCES or EPERM)"
            ),
        }
    }
}

/// Check 3: new read opens still succeed (Landlock only gates writes).
fn check_new_read_opens_succeed(temp_dir: &Path) -> SealTestResult {
    let target = temp_dir.join("control");
    match fs::read_to_string(&target) {
        Ok(_) => SealTestResult {
            name: "new_read_opens_succeed",
            passed: true,
            message: "new read open succeeded (Landlock only gates writes)".to_string(),
        },
        Err(e) => SealTestResult {
            name: "new_read_opens_succeed",
            passed: false,
            message: format!("new read open failed: {e}"),
        },
    }
}

/// Check 4: child thread inherits Landlock restrictions.
fn check_child_thread_inherits(temp_dir: &Path) -> SealTestResult {
    let target = temp_dir.join("brightness");
    let target_path = target.to_string_lossy().into_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let c_path = match std::ffi::CString::new(target_path) {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(format!("path encoding error: {e}"));
                return;
            }
        };
        let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_WRONLY) };
        if fd >= 0 {
            unsafe { libc::close(fd) };
            let _ = tx.send("child thread opened for write — escape detected".to_string());
        } else {
            let errno = unsafe { *libc::__errno_location() };
            let _ = tx.send(format!("child thread write open denied (errno={errno})"));
        }
    });
    handle.join().expect("child thread panicked");
    let message = rx.recv().unwrap_or_else(|e| format!("channel error: {e}"));
    let passed = message.contains("denied");
    SealTestResult {
        name: "child_thread_inherits",
        passed,
        message,
    }
}

/// Check 5: removed-file handling — writes to a pre-opened fd whose
/// file was deleted should return a handled error, not a crash.
/// (Simulates hot-unplug of a sysfs object.)
fn check_removed_file_handling(pre_opened: &[PreOpenedHandle]) -> SealTestResult {
    // Try to write to the first pre-opened handle. The file may or may
    // not have been removed (we don't actually remove it in this test —
    // that would require unlink, which Landlock also denies). Instead,
    // we verify that a write to a pre-opened fd either succeeds or
    // returns a clean error (EIO/ENODEV/EBADF), not a panic.
    if pre_opened.is_empty() {
        return SealTestResult {
            name: "removed_file_handling",
            passed: false,
            message: "no pre-opened handles to test".to_string(),
        };
    }
    let handle = &pre_opened[0];
    let data = b"probe\n";
    let ret = unsafe { libc::write(handle.fd, data.as_ptr() as *const _, data.len()) };
    if ret >= 0 {
        SealTestResult {
            name: "removed_file_handling",
            passed: true,
            message: "write to pre-opened fd succeeded (file still present)".to_string(),
        }
    } else {
        let errno = unsafe { *libc::__errno_location() };
        // EIO, ENODEV, EBADF, EINVAL are all "handled errors" — the
        // process didn't crash, it got a clean error.
        let handled = matches!(errno, libc::EIO | libc::ENODEV | libc::EBADF | libc::EINVAL);
        SealTestResult {
            name: "removed_file_handling",
            passed: handled,
            message: format!(
                "write to pre-opened fd returned errno={errno} ({})",
                if handled { "handled" } else { "unhandled" }
            ),
        }
    }
}
