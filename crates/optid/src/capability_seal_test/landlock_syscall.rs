//! Raw Landlock syscall wrappers shared by the D0 proof and S4D runtime seal.
//!
//! The implementation detects the running ABI, handles only rights supported
//! by that ABI, sets `no_new_privs`, and installs an irreversible filesystem
//! restriction. D0 uses an empty allowlist to prove complete write denial.
//! S4D additionally grants write access only beneath explicit daemon-state
//! roots while hardware writes continue through descriptors opened pre-seal.

use std::io;
use std::os::raw::{c_int, c_long, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

// These syscall numbers are shared by the Linux architectures Rush currently
// targets (x86_64 and aarch64).
const SYS_LANDLOCK_CREATE_RULESET: c_long = 444;
const SYS_LANDLOCK_ADD_RULE: c_long = 445;
const SYS_LANDLOCK_RESTRICT_SELF: c_long = 446;
const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;
const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

// Filesystem access bits from include/uapi/linux/landlock.h.
const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;

const LANDLOCK_ABI_REFER: u32 = 2;
const LANDLOCK_ABI_TRUNCATE: u32 = 3;

const ABI1_WRITE_RIGHTS: u64 = LANDLOCK_ACCESS_FS_WRITE_FILE
    | LANDLOCK_ACCESS_FS_REMOVE_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_FILE
    | LANDLOCK_ACCESS_FS_MAKE_CHAR
    | LANDLOCK_ACCESS_FS_MAKE_DIR
    | LANDLOCK_ACCESS_FS_MAKE_REG
    | LANDLOCK_ACCESS_FS_MAKE_SOCK
    | LANDLOCK_ACCESS_FS_MAKE_FIFO
    | LANDLOCK_ACCESS_FS_MAKE_BLOCK
    | LANDLOCK_ACCESS_FS_MAKE_SYM;

/// Prefix of `struct landlock_ruleset_attr` used by every Landlock ABI.
/// Passing the eight-byte prefix avoids depending on fields added later.
#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
}

/// `struct landlock_path_beneath_attr` from linux/landlock.h.
#[repr(C)]
struct LandlockPathBeneathAttr {
    allowed_access: u64,
    parent_fd: c_int,
    reserved: u32,
}

struct OwnedFd(c_int);

impl Drop for OwnedFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

impl OwnedFd {
    fn open_path(path: &Path) -> io::Result<Self> {
        let bytes = path.as_os_str().as_bytes();
        if bytes.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Landlock path contains NUL: {}", path.display()),
            ));
        }
        let mut nul = Vec::with_capacity(bytes.len() + 1);
        nul.extend_from_slice(bytes);
        nul.push(0);
        let fd = unsafe { libc::open(nul.as_ptr().cast(), libc::O_PATH | libc::O_CLOEXEC) };
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(fd))
        }
    }
}

/// Detect the maximum Landlock ABI supported by the running kernel.
pub(crate) fn detect_landlock_abi() -> io::Result<u32> {
    let ret = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<c_void>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret as u32)
    }
}

/// Return exactly the write-related rights supported by `abi`.
pub(crate) fn handled_write_rights(abi: u32) -> io::Result<u64> {
    if abi == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Landlock ABI 0 cannot enforce filesystem restrictions",
        ));
    }

    let mut rights = ABI1_WRITE_RIGHTS;
    if abi >= LANDLOCK_ABI_REFER {
        rights |= LANDLOCK_ACCESS_FS_REFER;
    }
    if abi >= LANDLOCK_ABI_TRUNCATE {
        rights |= LANDLOCK_ACCESS_FS_TRUNCATE;
    }
    Ok(rights)
}

/// Set Linux's irreversible no-new-privileges bit for this thread.
pub(crate) fn set_no_new_privs() -> io::Result<()> {
    let ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Query whether no-new-privileges is active for this thread.
pub(crate) fn no_new_privs_is_set() -> io::Result<bool> {
    let ret = unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret == 1)
    }
}

fn create_ruleset(handled_access_fs: u64) -> io::Result<OwnedFd> {
    let attr = LandlockRulesetAttr { handled_access_fs };
    let fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const LandlockRulesetAttr,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0u32,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(OwnedFd(fd as c_int))
    }
}

fn add_write_root(ruleset: &OwnedFd, root: &Path, rights: u64) -> io::Result<()> {
    let parent = OwnedFd::open_path(root)?;
    let attr = LandlockPathBeneathAttr {
        allowed_access: rights,
        parent_fd: parent.0,
        reserved: 0,
    };
    let ret = unsafe {
        libc::syscall(
            SYS_LANDLOCK_ADD_RULE,
            ruleset.0,
            LANDLOCK_RULE_PATH_BENEATH,
            &attr as *const LandlockPathBeneathAttr,
            0u32,
        )
    };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Install an empty ruleset. This is retained for the D0 negative proof: all
/// new write opens and path mutations are denied after the call.
pub(crate) fn install_landlock_restrictions(abi: u32) -> io::Result<u64> {
    install_landlock_restrictions_with_write_roots(abi, &[])
}

// The same source is compiled both by the D0 proof binary and as S4D's nested
// runtime Landlock module. Keep the proof-only wrapper visible in the latter
// build through a typed compile-time reference rather than a lint suppression.
const _: fn(u32) -> io::Result<u64> = install_landlock_restrictions;

/// Install a write-deny ruleset with explicit writable daemon-state roots.
///
/// Existing descriptors keep the rights acquired when opened. New hardware
/// write opens are denied because no hardware path is granted. Each state root
/// is canonicalized and de-duplicated before a `PATH_BENEATH` rule is added.
pub(crate) fn install_landlock_restrictions_with_write_roots(
    abi: u32,
    write_roots: &[PathBuf],
) -> io::Result<u64> {
    let handled_access_fs = handled_write_rights(abi)?;
    let ruleset = create_ruleset(handled_access_fs)?;

    let mut canonical_roots = Vec::new();
    for root in write_roots {
        let canonical = std::fs::canonicalize(root)?;
        if !canonical_roots.contains(&canonical) {
            add_write_root(&ruleset, &canonical, handled_access_fs)?;
            canonical_roots.push(canonical);
        }
    }

    set_no_new_privs()?;
    if !no_new_privs_is_set()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "PR_SET_NO_NEW_PRIVS returned success but the bit is not set",
        ));
    }

    let ret = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset.0, 0u32) };
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(handled_access_fs)
    }
}

/// Exact running kernel release for a cold-verification receipt.
pub(crate) fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

// See the proof-wrapper reference above: the runtime module does not print the
// kernel release, but compiling the shared implementation must remain clean.
const _: fn() -> String = kernel_release;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_one_excludes_newer_rights() {
        let rights = handled_write_rights(1).expect("ABI 1 rights");
        assert_eq!(rights & LANDLOCK_ACCESS_FS_REFER, 0);
        assert_eq!(rights & LANDLOCK_ACCESS_FS_TRUNCATE, 0);
        assert_ne!(rights & LANDLOCK_ACCESS_FS_WRITE_FILE, 0);
        assert_ne!(rights & LANDLOCK_ACCESS_FS_REMOVE_FILE, 0);
    }

    #[test]
    fn abi_two_adds_refer_only() {
        let rights = handled_write_rights(2).expect("ABI 2 rights");
        assert_ne!(rights & LANDLOCK_ACCESS_FS_REFER, 0);
        assert_eq!(rights & LANDLOCK_ACCESS_FS_TRUNCATE, 0);
    }

    #[test]
    fn abi_three_adds_truncate() {
        let rights = handled_write_rights(3).expect("ABI 3 rights");
        assert_ne!(rights & LANDLOCK_ACCESS_FS_REFER, 0);
        assert_ne!(rights & LANDLOCK_ACCESS_FS_TRUNCATE, 0);
    }

    #[test]
    fn abi_zero_is_rejected() {
        assert_eq!(
            handled_write_rights(0).expect_err("ABI 0 must fail").kind(),
            io::ErrorKind::Unsupported
        );
    }
}
