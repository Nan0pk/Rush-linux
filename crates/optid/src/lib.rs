//! Optid library — F2: injectable kernel I/O seam for integration tests.
//!
//! This library target exposes the kernel I/O traits and types so that
//! integration tests can construct `MemoryKernel` and `FaultKernel` instances
//! to exercise deterministic fault injection through the same I/O boundary
//! that production uses.
//!
//! The binary target (`main.rs`) remains the sole production entry point.
//! This library is intended for workspace-internal use only.

// Include the kernel_io module which defines all the public types.
mod kernel_io;

/// S1D frozen per-lever rollback, stabilization, and semantic envelopes.
pub mod lever_contract;

// Re-export the kernel I/O traits and types for integration tests.
pub use kernel_io::{
    Clock, EventSource, FaultKernel, KernelIo, KernelRead, KernelWrite, RealKernel,
};

// Re-export the allowlist check for test fixtures that need to populate
// allowlisted paths in MemoryKernel.
pub use kernel_io::is_allowlisted_write_path;

// Re-export MemoryKernel only when the test-utils feature is enabled or during tests.
#[cfg(any(test, feature = "test-utils"))]
pub use kernel_io::MemoryKernel;
