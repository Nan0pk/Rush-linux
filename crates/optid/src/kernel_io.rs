//! F2 production adapter for injectable kernel I/O.
//!
//! The mechanical implementations live in `kernel_io_impl.rs`. This module
//! keeps `RealKernel` as the production type consumed by the daemon while
//! allowing binary-crate tests to replace that adapter on the current thread
//! with a deterministic `KernelIo`. The override is compiled only for tests;
//! release behavior remains direct delegation to the production implementation.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "kernel_io_impl.rs"]
mod implementation;

pub use implementation::{
    is_allowlisted_write_path, Clock, EventSource, FaultKernel, KernelIo, KernelRead, KernelWrite,
};

#[cfg(any(test, feature = "test-utils"))]
pub use implementation::MemoryKernel;

// The facade is private in the binary target and public in the library target.
// These compile-time references keep every exported F2 boundary, including the
// future E1 event seam, visible to both builds without dead-code suppressions.
const _: fn(&Path) -> io::Result<()> = is_allowlisted_write_path;
fn wait_event_source(source: &dyn EventSource, duration: Duration) -> bool {
    source.wait(duration)
}
const _: fn(&dyn EventSource, Duration) -> bool = wait_event_source;
const _: fn(Box<dyn KernelIo>) -> FaultKernel = FaultKernel::new;
const _: for<'a> fn(&'a FaultKernel, PathBuf, io::ErrorKind) -> &'a FaultKernel =
    FaultKernel::fail_next_write;
const _: for<'a> fn(&'a FaultKernel, PathBuf, usize) -> &'a FaultKernel =
    FaultKernel::fail_next_write_short;
const _: for<'a> fn(&'a FaultKernel, PathBuf, io::ErrorKind) -> &'a FaultKernel =
    FaultKernel::fail_next_read;
const _: for<'a> fn(&'a FaultKernel, PathBuf) -> &'a FaultKernel = FaultKernel::hide_path;
const _: for<'a> fn(&'a FaultKernel, PathBuf, String) -> &'a FaultKernel =
    FaultKernel::malform_content;
const _: for<'a> fn(&'a FaultKernel, PathBuf, PathBuf, io::ErrorKind) -> &'a FaultKernel =
    FaultKernel::fail_next_rename;
const _: for<'a> fn(&'a FaultKernel, PathBuf, io::ErrorKind) -> &'a FaultKernel =
    FaultKernel::fail_next_remove;
const _: for<'a> fn(&'a FaultKernel, PathBuf, io::ErrorKind) -> &'a FaultKernel =
    FaultKernel::fail_next_create_dir;

#[cfg(any(test, feature = "test-utils"))]
const _: fn() -> MemoryKernel = MemoryKernel::new;
#[cfg(any(test, feature = "test-utils"))]
const _: fn(&MemoryKernel, u64) = MemoryKernel::advance_clock;
#[cfg(any(test, feature = "test-utils"))]
const _: fn(&MemoryKernel, &Path, &str) = MemoryKernel::write_raw;
#[cfg(any(test, feature = "test-utils"))]
const _: fn(&MemoryKernel, &Path, &Path) = MemoryKernel::write_link;
#[cfg(any(test, feature = "test-utils"))]
const _: fn(&MemoryKernel, &Path, &Path) = MemoryKernel::add_dir;
#[cfg(any(test, feature = "test-utils"))]
const _: fn(&MemoryKernel, &Path, &Path) = MemoryKernel::add_dir_entry;

/// Production kernel adapter used by the daemon, sensors, recovery helpers,
/// and actuator. In non-test builds every method delegates directly to the
/// standard-library implementation.
#[derive(Clone, Default)]
pub struct RealKernel {
    inner: implementation::RealKernel,
}

impl RealKernel {
    pub fn new() -> Self {
        Self {
            inner: implementation::RealKernel::new(),
        }
    }

    fn dispatch<R>(&self, call: impl Fn(&dyn KernelIo) -> R) -> R {
        #[cfg(test)]
        {
            let overridden = REAL_KERNEL_OVERRIDE.with(|slot| {
                let slot = slot.borrow();
                slot.as_deref().map(&call)
            });
            if let Some(result) = overridden {
                return result;
            }
        }

        call(&self.inner)
    }
}

impl KernelRead for RealKernel {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.dispatch(|kernel| kernel.read_to_string(path))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.dispatch(|kernel| kernel.read_dir(path))
    }

    fn exists(&self, path: &Path) -> bool {
        self.dispatch(|kernel| kernel.exists(path))
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.dispatch(|kernel| kernel.read_link(path))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.dispatch(|kernel| kernel.canonicalize(path))
    }
}

impl KernelWrite for RealKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        self.dispatch(|kernel| kernel.write(path, value))
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.dispatch(|kernel| kernel.write_state_file(path, value))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dispatch(|kernel| kernel.create_dir_all(path))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.dispatch(|kernel| kernel.rename(from, to))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.dispatch(|kernel| kernel.remove_file(path))
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.dispatch(|kernel| kernel.append(path, text))
    }
}

impl Clock for RealKernel {
    fn now_unix(&self) -> u64 {
        self.dispatch(|kernel| kernel.now_unix())
    }
}

impl EventSource for RealKernel {
    fn wait(&self, duration: Duration) -> bool {
        self.inner.wait(duration)
    }
}

#[cfg(test)]
thread_local! {
    static REAL_KERNEL_OVERRIDE: std::cell::RefCell<Option<Box<dyn KernelIo>>> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
#[allow(dead_code)] // Consumed by binary-crate tests, not lib tests.
pub(crate) fn real_kernel_override_is_active() -> bool {
    REAL_KERNEL_OVERRIDE.with(|slot| slot.borrow().is_some())
}

#[cfg(test)]
struct OverrideGuard(Option<Box<dyn KernelIo>>);

#[cfg(test)]
impl Drop for OverrideGuard {
    fn drop(&mut self) {
        REAL_KERNEL_OVERRIDE.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

/// Run a binary-crate test with all `RealKernel` construction on the current
/// thread routed through `kernel`. The previous override is restored even if
/// the test unwinds.
#[cfg(test)]
pub(crate) fn with_real_kernel_override<R>(
    kernel: Box<dyn KernelIo>,
    run: impl FnOnce() -> R,
) -> R {
    let previous = REAL_KERNEL_OVERRIDE.with(|slot| slot.replace(Some(kernel)));
    let _guard = OverrideGuard(previous);
    run()
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn f2_real_kernel_override_is_visible_to_library_target() {
        let memory = MemoryKernel::new();
        let path = Path::new("/virtual/f2-adapter");
        memory.write_raw(path, "injected");

        let observed =
            with_real_kernel_override(Box::new(memory), || RealKernel::new().read_to_string(path))
                .expect("the RealKernel facade must delegate through the test override");

        assert_eq!(observed, "injected");
    }
}
