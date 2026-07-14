//! Lockless PSI (Pressure Stall Information) total extraction.
//!
//! Bypasses the `avg10/60/300` exponential moving averages and reads only
//! the monotonic `total=` microsecond counter from `/proc/pressure/{cpu,io}`.
//! Uses pre-computed byte offsets for zero-parse reads.
//!
//! ## Why `total=` instead of `avg10=`
//!
//! The kernel's PSI subsystem computes `avg10` using a decaying EMA sampled
//! every 2 seconds. For a benchmark window of N seconds (where N << 10),
//! the average is dominated by the pre-benchmark idle state. A 5-second
//! benchmark inside a 10-second decay window contributes at most ~39% of
//! its true value.
//!
//! The `total=` counter is a monotonic μs counter of absolute stall time
//! since boot. A lockless delta (`total_end - total_start`) divided by the
//! benchmark window gives the exact stall percentage during that window,
//! unaffected by any averaging kernel.

use std::fs::File;
use std::io;
use std::os::unix::io::RawFd;

/// Byte offset of the numeric value after "total=" in the PSI file's
/// first ("some") line. Discovered at initialization.
#[derive(Debug, Clone, Copy)]
struct PsiFileHandle {
    fd: RawFd,
    /// Byte offset where the numeric total value starts (after "total=").
    total_offset: i64,
}

/// PSI resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsiResource {
    Cpu,
    Io,
}

impl PsiResource {
    fn path(&self) -> &'static str {
        match self {
            PsiResource::Cpu => "/proc/pressure/cpu",
            PsiResource::Io => "/proc/pressure/io",
        }
    }
}

/// Raw PSI total counter sample.
#[derive(Debug, Clone, Copy)]
pub struct PsiSample {
    /// Monotonic microsecond counter from kernel PSI subsystem.
    /// This is the absolute stall time since boot.
    pub total_us: u64,
    /// `CLOCK_MONOTONIC` timestamp of when this sample was taken.
    pub instant: std::time::Instant,
}

/// A reader for a single PSI resource.
pub struct PsiReader {
    resource: PsiResource,
    handle: Option<PsiFileHandle>,
}

impl PsiReader {
    /// Open a PSI reader for the given resource.
    ///
    /// At initialization, reads the file once to discover the byte offset
    /// of the `total=` value. All subsequent reads use `pread()` at that
    /// offset, avoiding full-file parsing.
    pub fn open(resource: PsiResource) -> io::Result<Self> {
        let handle = Self::discover_offset(resource)?;
        Ok(PsiReader {
            resource,
            handle: Some(handle),
        })
    }

    /// Discover the byte offset of "total=" in the PSI file.
    fn discover_offset(resource: PsiResource) -> io::Result<PsiFileHandle> {
        use std::os::unix::io::FromRawFd;

        let path = resource.path();
        // Open with O_RDONLY | O_CLOEXEC
        let file = File::open(path)?;
        let fd = std::os::unix::io::AsRawFd::as_raw_fd(&file);

        // Read enough to capture the "some" line (typically ~80 bytes)
        let mut buf = [0u8; 128];
        let n = unsafe {
            libc::pread(fd, buf.as_mut_ptr() as *mut libc::c_void, 128, 0)
        };
        if n <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("PSI file {path} is empty or unreadable"),
            ));
        }

        // Find "total=" in the first line (the "some" line)
        let haystack = &buf[..n as usize];
        let total_marker = b"total=";
        let offset = find_subsequence(haystack, total_marker).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("'total=' not found in {path}"),
            )
        })?;

        let total_offset = (offset + total_marker.len()) as i64;

        // We need to keep the file open for subsequent pread() calls.
        // We leak the File intentionally — the RawFd is stored in the handle.
        // The File will be closed when PsiReader is dropped (we store it).
        std::mem::forget(file);

        Ok(PsiFileHandle { fd, total_offset })
    }

    /// Read the raw PSI total microsecond counter.
    ///
    /// This is the hot path — uses a single `pread()` at the pre-computed
    /// offset. No string parsing, no full-file reads.
    #[inline]
    pub fn read_total(&self) -> io::Result<PsiSample> {
        let handle = self.handle.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "PSI reader not initialized")
        })?;

        // The total value is at most 20 digits (u64 max = 18446744073709551615)
        // plus a newline. Read 21 bytes.
        let mut val_buf = [0u8; 21];
        let n = unsafe {
            libc::pread(
                handle.fd,
                val_buf.as_mut_ptr() as *mut libc::c_void,
                21,
                handle.total_offset,
            )
        };
        if n <= 0 {
            return Err(io::Error::last_os_error());
        }

        // Parse the ASCII digits directly — no allocation, no String.
        let val_str = std::str::from_utf8(&val_buf[..n as usize])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Trim any trailing whitespace/newline
        let val_str = val_str.trim();

        let total_us: u64 = val_str.parse().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse PSI total '{val_str}': {e}"),
            )
        })?;

        Ok(PsiSample {
            total_us,
            instant: std::time::Instant::now(),
        })
    }

    /// Compute the exact stall percentage during a measurement window.
    ///
    /// Returns the percentage of wall-clock time (0.0-100.0) that tasks
    /// were stalled on this resource during the interval between `start`
    /// and `end`.
    ///
    /// This is **not** an EMA approximation — it is the exact delta of
    /// the monotonic total counter divided by the elapsed wall time.
    pub fn stall_percentage(start: &PsiSample, end: &PsiSample) -> f64 {
        let elapsed_us = end.instant.duration_since(start.instant).as_micros() as f64;
        if elapsed_us <= 0.0 {
            return 0.0;
        }
        let stall_us = end.total_us.saturating_sub(start.total_us) as f64;
        (stall_us / elapsed_us) * 100.0
    }
}

impl Drop for PsiReader {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                libc::close(handle.fd);
            }
        }
    }
}

/// Find a subsequence in a byte slice. Returns the starting index.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_subsequence() {
        assert_eq!(
            find_subsequence(b"some avg10=0.00 avg60=0.00 avg300=0.00 total=12345678", b"total="),
            Some(44)
        );
        assert_eq!(find_subsequence(b"hello", b"world"), None);
    }

    #[test]
    fn test_stall_percentage_calculation() {
        let start = PsiSample {
            total_us: 1000000,
            instant: std::time::Instant::now(),
        };
        // Simulate 100ms later with 50ms of stall
        let end = PsiSample {
            total_us: 1050000,
            instant: start.instant + std::time::Duration::from_millis(100),
        };
        let pct = PsiReader::stall_percentage(&start, &end);
        assert!((pct - 50.0).abs() < 1.0);
    }
}
