//! Raw Landlock syscall wrappers for the D0 prototype.
//!
//! Uses `libc::syscall` directly because the `landlock` crate is not
//! a project dependency and D0 is an experimental prototype that
//! should not pull in new deps.

use std::io;
use std::os::raw::{c_int, c_long, c_uint, c_ulong, c_void};

/// Linux syscall numbers for Landlock (from linux/landlock.h).
/// These are stable on x86_64 and aarch64; other arches may differ.
const SYS_LANDLOCK_CREATE_RULESET: c_long = 444;
const SYS_LANDLOCK_ADD_RULE: c_long = 445;
const SYS_LANDLOCK_RESTRICT_SELF: c_long = 446;

/// Landlock rule type: path-bounded file.
const LANDLOCK_RULE_PATH_BENEATH: c_int = 1;

/// Landlock access rights for write (as of ABI v1).
const LANDLOCK_ACCESS_FS_WRITE: u64 = 0x02;
const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 0x400;
const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 0x10;
const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 0x20;
const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 0x80;
const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 0x40;
const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 0x100;
const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 0x200;
const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 0x800;
const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 0x1000;

/// All write-related Landlock access rights. If Landlock denies all of
/// these, the process cannot open new files for writing outside the
/// ruleset's allowed paths.
const ALL_WRITE_RIGHTS: u64 = LANDLOCK_ACCESS_FS_WRITE
    | LANDLOCK_ACCESS_FS_TRUNCATE
    | LANDLOCK_ACCESS_FS_REMOVE_FILE
    | LANDLOCK_ACCESS_FS_REMOVE_DIR
    | LANDLOCK_ACCESS_FS_MAKE_REG
    | LANDLOCK_ACCESS_FS_MAKE_DIR
    | LANDLOCK_ACCESS_FS_MAKE_SOCK
    | LANDLOCK_ACCESS_FS_MAKE_FIFO
    | LANDLOCK_ACCESS_FS_MAKE_BLOCK
    | LANDLOCK_ACCESS_FS_MAKE_SYM;

/// `landlock_ruleset_attr` from linux/landlock.h.
#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
    handled_access_net: u64,
}

/// `landlock_path_beneath_attr` from linux/landlock.h.
#[repr(C)]
struct LandlockPathBeneathAttr {
    allowed_access: u64,
    parent_fd: c_int,
}

/// Detect the running Landlock ABI version.
///
/// Returns `Ok(abi_version)` if Landlock is supported, or `Err` if the
/// kernel does not support Landlock. The ABI version is the maximum
/// supported version (1 = initial, 2 = truncate, 3 = network, etc.).
pub(crate) fn detect_landlock_abi() -> io::Result<u32> {
    // landlock_create_ruleset(NULL, 0, LANDLOCK_CREATE_RULESET_VERSION)
    // returns the maximum supported ABI version.
    let ret = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<c_void>(),
            0usize,
            1u32, // LANDLOCK_CREATE_RULESET_VERSION
        )
    };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ret as u32)
}

/// Install Landlock restrictions that deny ALL filesystem write access.
///
/// After this call, the process cannot open new files for writing
/// anywhere. Pre-opened file descriptors remain usable (this is the
/// key D2 invariant: "no new write opens after sealing").
///
/// The restrictions are inherited by child threads and processes
/// (forked or exec'd) and cannot be removed — Landlock is a one-way
/// ratchet.
pub(crate) fn install_landlock_restrictions() -> io::Result<()> {
    // Step 1: create a ruleset that handles all write rights.
    let attr = LandlockRulesetAttr {
        handled_access_fs: ALL_WRITE_RIGHTS,
        handled_access_net: 0, // ABI v1: no network
    };
    let ruleset_fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const LandlockRulesetAttr,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0u32, // flags
        )
    };
    if ruleset_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let ruleset_fd = ruleset_fd as c_int;

    // Step 2: restrict self with the ruleset. We add NO rules, so all
    // write access is denied (default-deny). Pre-opened descriptors
    // remain usable because Landlock only gates new opens.
    let ret = unsafe {
        libc::syscall(
            SYS_LANDLOCK_RESTRICT_SELF,
            ruleset_fd,
            0u32, // flags
        )
    };
    let restrict_err = if ret < 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };

    // Close the ruleset fd (no longer needed after restrict_self).
    unsafe {
        libc::close(ruleset_fd);
    }

    if let Some(e) = restrict_err {
        return Err(e);
    }
    Ok(())
}

/// Attempt to add a path-bounded rule to a ruleset. Used by tests that
/// want to verify the "allow specific path" path. Not used by the
/// default restrict-self-to-empty-ruleset path.
#[allow(dead_code)]
pub(crate) fn add_path_rule(ruleset_fd: c_int, path_fd: c_int) -> io::Result<()> {
    let attr = LandlockPathBeneathAttr {
        allowed_access: LANDLOCK_ACCESS_FS_WRITE,
        parent_fd: path_fd,
    };
    let ret = unsafe {
        libc::syscall(
            SYS_LANDLOCK_ADD_RULE,
            ruleset_fd,
            LANDLOCK_RULE_PATH_BENEATH as c_ulong,
            &attr as *const LandlockPathBeneathAttr,
            0u32, // flags
        )
    };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Get a rough kernel version identifier for the receipt. This is not
/// a full kernel version — just enough to distinguish major kernel
/// families (5.x vs 6.x) for the Landlock ABI matrix.
pub(crate) fn kernel_version() -> u32 {
    // Read /proc/sys/kernel/os-release
    let release = std::fs::read_to_string("/proc/sys/kernel/os-release").unwrap_or_default();
    // Parse "X.Y.Z" → 0xXXYYZZ
    let parts: Vec<&str> = release.split('.').collect();
    if parts.len() >= 3 {
        let major = parts[0].parse::<u32>().unwrap_or(0);
        let minor = parts[1].parse::<u32>().unwrap_or(0);
        let patch = parts[2]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        return (major << 16) | (minor << 8) | patch;
    }
    0
}

// Suppress unused warnings for c_uint (used in type annotations above).
#[allow(dead_code)]
const _C_UINT_USED: c_uint = 0;
