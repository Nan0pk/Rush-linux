//! Runtime checks for the experimental capability-sealing proof.
//!
//! These checks execute after Landlock is installed. They validate the exact
//! behaviors needed by the fail-passive architecture: pre-opened descriptors
//! remain usable, new write opens fail, descendants cannot escape by thread or
//! exec, and a descriptor to an object removed before sealing is handled without
//! reopening a path or panicking.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A descriptor opened before Landlock restriction.
pub(crate) struct PreOpenedHandle {
    pub path: PathBuf,
    pub fd: RawFd,
    pub removed_before_seal: bool,
}

impl Drop for PreOpenedHandle {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe {
                libc::close(self.fd);
            }
            self.fd = -1;
        }
    }
}

/// The result of one runtime check.
pub(crate) struct SealTestResult {
    pub name: &'static str,
    pub passed: bool,
    pub message: String,
}

impl SealTestResult {
    fn pass(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            passed: true,
            message: message.into(),
        }
    }

    fn fail(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            passed: false,
            message: message.into(),
        }
    }
}

/// Open all synthetic targets before sealing. The removed-object target is
/// unlinked while its descriptor remains open, which creates a real removed
/// object rather than merely accepting either outcome on a live file.
pub(crate) fn open_targets_before_seal(temp_dir: &Path) -> io::Result<Vec<PreOpenedHandle>> {
    let mut handles = Vec::new();

    for relative in [
        PathBuf::from("control"),
        PathBuf::from("brightness"),
        PathBuf::from("power/autosuspend_delay_ms"),
    ] {
        let fd = open_for_write(&temp_dir.join(&relative))?;
        handles.push(PreOpenedHandle {
            path: relative,
            fd,
            removed_before_seal: false,
        });
    }

    let removed = PathBuf::from("removed-object");
    let removed_path = temp_dir.join(&removed);
    let removed_fd = open_for_write(&removed_path)?;
    fs::remove_file(&removed_path)?;
    handles.push(PreOpenedHandle {
        path: removed,
        fd: removed_fd,
        removed_before_seal: true,
    });

    Ok(handles)
}

fn open_for_write(path: &Path) -> io::Result<RawFd> {
    let c_path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

/// Return `Ok(true)` only when a new write open is rejected with the expected
/// access-control error. This is also used by the post-exec child mode.
pub(crate) fn new_write_open_is_denied(path: &Path) -> io::Result<bool> {
    let c_path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC) };
    if fd >= 0 {
        unsafe {
            libc::close(fd);
        }
        return Ok(false);
    }

    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EACCES | libc::EPERM) => Ok(true),
        _ => Err(error),
    }
}

/// Run every post-seal runtime check.
pub(crate) fn run_seal_checks(
    temp_dir: &Path,
    pre_opened: &[PreOpenedHandle],
    current_exe: &Path,
) -> Vec<SealTestResult> {
    vec![
        check_pre_opened_writes_succeed(pre_opened),
        check_new_write_opens_denied(temp_dir),
        check_new_read_opens_succeed(temp_dir),
        check_child_thread_inherits(temp_dir),
        check_child_process_exec_inherits(temp_dir, current_exe),
        check_removed_object_handling(temp_dir, pre_opened),
    ]
}

fn check_pre_opened_writes_succeed(pre_opened: &[PreOpenedHandle]) -> SealTestResult {
    for handle in pre_opened {
        if let Err(error) = write_probe(handle.fd, b"sealed-write\n") {
            return SealTestResult::fail(
                "pre_opened_writes_succeed",
                format!(
                    "write to pre-opened '{}' failed: {error}",
                    handle.path.display()
                ),
            );
        }
    }

    SealTestResult::pass(
        "pre_opened_writes_succeed",
        format!(
            "all {} pre-opened descriptors remained writable after sealing",
            pre_opened.len()
        ),
    )
}

fn check_new_write_opens_denied(temp_dir: &Path) -> SealTestResult {
    let target = temp_dir.join("control");
    match new_write_open_is_denied(&target) {
        Ok(true) => SealTestResult::pass(
            "new_write_opens_denied",
            "new write open denied with EACCES/EPERM",
        ),
        Ok(false) => SealTestResult::fail(
            "new_write_opens_denied",
            "new write open succeeded after sealing",
        ),
        Err(error) => SealTestResult::fail(
            "new_write_opens_denied",
            format!("new write open failed with an unexpected error: {error}"),
        ),
    }
}

fn check_new_read_opens_succeed(temp_dir: &Path) -> SealTestResult {
    let target = temp_dir.join("control");
    match fs::read_to_string(target) {
        Ok(_) => SealTestResult::pass(
            "new_read_opens_succeed",
            "new read open remained available because only write rights are handled",
        ),
        Err(error) => SealTestResult::fail(
            "new_read_opens_succeed",
            format!("new read open failed: {error}"),
        ),
    }
}

fn check_child_thread_inherits(temp_dir: &Path) -> SealTestResult {
    let target = temp_dir.join("brightness");
    let handle = std::thread::spawn(move || new_write_open_is_denied(&target));
    match handle.join() {
        Ok(Ok(true)) => SealTestResult::pass(
            "child_thread_inherits",
            "child thread inherited sealing and could not open a write descriptor",
        ),
        Ok(Ok(false)) => SealTestResult::fail(
            "child_thread_inherits",
            "child thread escaped sealing and opened a write descriptor",
        ),
        Ok(Err(error)) => SealTestResult::fail(
            "child_thread_inherits",
            format!("child thread received an unexpected error: {error}"),
        ),
        Err(_) => SealTestResult::fail("child_thread_inherits", "child thread panicked"),
    }
}

fn check_child_process_exec_inherits(temp_dir: &Path, current_exe: &Path) -> SealTestResult {
    let target = temp_dir.join("brightness");
    match Command::new(current_exe)
        .arg("--exec-child-write-probe")
        .arg(&target)
        .status()
    {
        Ok(status) if status.success() => SealTestResult::pass(
            "child_process_exec_inherits",
            "fork/exec child retained no-new-privileges and could not open a write descriptor",
        ),
        Ok(status) => SealTestResult::fail(
            "child_process_exec_inherits",
            format!("fork/exec child reported escape or invalid inheritance: {status}"),
        ),
        Err(error) => SealTestResult::fail(
            "child_process_exec_inherits",
            format!("could not execute child probe: {error}"),
        ),
    }
}

fn check_removed_object_handling(
    temp_dir: &Path,
    pre_opened: &[PreOpenedHandle],
) -> SealTestResult {
    let Some(handle) = pre_opened.iter().find(|handle| handle.removed_before_seal) else {
        return SealTestResult::fail(
            "removed_object_handling",
            "no descriptor was marked as removed before sealing",
        );
    };

    let removed_path = temp_dir.join(&handle.path);
    if removed_path.exists() {
        return SealTestResult::fail(
            "removed_object_handling",
            format!("removed object still exists at {}", removed_path.display()),
        );
    }

    match write_probe(handle.fd, b"removed-object-probe\n") {
        Ok(()) => SealTestResult::pass(
            "removed_object_handling",
            "object was genuinely unlinked; its pre-opened descriptor remained safely usable",
        ),
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::EIO | libc::ENODEV | libc::EBADF | libc::EINVAL)
            ) =>
        {
            SealTestResult::pass(
                "removed_object_handling",
                format!("removed descriptor returned a handled kernel error: {error}"),
            )
        }
        Err(error) => SealTestResult::fail(
            "removed_object_handling",
            format!("removed descriptor returned an unexpected error: {error}"),
        ),
    }
}

fn write_probe(fd: RawFd, data: &[u8]) -> io::Result<()> {
    let ret = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    if ret as usize != data.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!("short write: expected {} bytes, wrote {ret}", data.len()),
        ));
    }
    Ok(())
}
