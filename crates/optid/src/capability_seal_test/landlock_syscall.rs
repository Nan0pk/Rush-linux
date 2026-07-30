//! Raw Landlock syscall wrappers for the experimental capability-sealing proof.
//!
//! The prototype intentionally uses the platform ABI directly. It detects the
//! running Landlock version, handles only rights supported by that version, sets
//! `no_new_privs`, and then installs an empty ruleset for write-related rights.
//! Existing descriptors remain usable; new write opens and path mutations are
//! denied.

use std::io;
use std::os::raw::{c_int, c_long, c_void};

// These syscall numbers are shared by the Linux architectures Rush currently
// targets for this prototype (x86_64 and aarch64).
const SYS_LANDLOCK_CREATE_RULESET: c_long = 444;
const SYS_LANDLOCK_RESTRICT_SELF: c_long = 446;
const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;

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
/// Passing the eight-byte prefix avoids depending on fields added by later ABIs.
#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
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
///
/// `REFER` was added in ABI 2 and `TRUNCATE` in ABI 3. Supplying a newer right
/// to an older kernel makes ruleset creation fail with `EINVAL`, so this mapping
/// is part of the security contract rather than a compatibility convenience.
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

/// Install an empty Landlock ruleset for all write-related rights supported by
/// the detected ABI.
///
/// With no allow rules, new write opens and path mutations are denied. File
/// descriptors opened before this call keep the rights captured at open time.
pub(crate) fn install_landlock_restrictions(abi: u32) -> io::Result<u64> {
    let handled_access_fs = handled_write_rights(abi)?;

    set_no_new_privs()?;
    if !no_new_privs_is_set()? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "PR_SET_NO_NEW_PRIVS returned success but the bit is not set",
        ));
    }

    let attr = LandlockRulesetAttr { handled_access_fs };
    let ruleset_fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const LandlockRulesetAttr,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0u32,
        )
    };
    if ruleset_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let ruleset_fd = ruleset_fd as c_int;

    let ret = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset_fd, 0u32) };
    let result = if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(handled_access_fs)
    };

    unsafe {
        libc::close(ruleset_fd);
    }
    result
}

/// Exact running kernel release for a cold-verification receipt.
pub(crate) fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

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
