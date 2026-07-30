//! F2 production adapter for injectable kernel I/O.
//!
//! The implementation that existed before the production-surface proof lives
//! unchanged in `kernel_io_impl.rs`. This module keeps `RealKernel` as the
//! production type consumed by the daemon while allowing binary-crate tests to
//! replace that adapter on the current thread with a deterministic `KernelIo`.
//! The override is compiled only for tests; release behavior remains a direct
//! delegation to the original production implementation.

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

/// Production kernel adapter used by the daemon, sensors, recovery helpers,
/// and actuator. In non-test builds every method delegates directly to the
/// pre-existing implementation.
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
        self.dispatch(|io| io.read_to_string(path))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.dispatch(|io| io.read_dir(path))
    }

    fn exists(&self, path: &Path) -> bool {
        self.dispatch(|io| io.exists(path))
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.dispatch(|io| io.read_link(path))
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.dispatch(|io| io.canonicalize(path))
    }
}

impl KernelWrite for RealKernel {
    fn write(&self, path: &Path, value: &str) -> io::Result<()> {
        self.dispatch(|io| io.write(path, value))
    }

    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.dispatch(|io| io.write_state_file(path, value))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dispatch(|io| io.create_dir_all(path))
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.dispatch(|io| io.rename(from, to))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.dispatch(|io| io.remove_file(path))
    }

    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.dispatch(|io| io.append(path, text))
    }
}

impl Clock for RealKernel {
    fn now_unix(&self) -> u64 {
        self.dispatch(|io| io.now_unix())
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
