//! Shared telemetry event types.
//!
//! These structs define the binary wire format for events emitted from the
//! eBPF programs via the ring buffer. They are `#[repr(C, packed)]` to
//! ensure identical layout in kernel and user space.
//!
//! ## Constraint Compliance
//!
//! - No strings, no floats, no divisions in the collection path
//! - All values are raw integers (TSC nanoseconds, RAPL ticks, μs counters)
//! - Scaling and interpretation happens exclusively in post-processing

use serde::{Deserialize, Serialize};

/// Event type discriminant — matches the BPF-side enum.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventType {
    /// Raw RAPL energy counter sample.
    EnergySample = 0,
    /// PSI total microsecond counter sample.
    PsiSample = 1,
    /// Task wakeup latency (from sched_stat_wait).
    SchedWait = 2,
    /// Context switch (from sched_switch).
    SchedSwitch = 3,
    /// Measurement window marker (START / STOP / ABORT).
    Marker = 4,
}

/// Marker types for measurement window boundaries.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkerType {
    Start = 0,
    Stop = 1,
    Abort = 2,
}

/// The fixed-size telemetry event struct.
///
/// This struct is **packed** with no alignment padding. Its layout must
/// exactly match the C-side `struct telemetry_event` in the BPF program.
/// Total size: 40 bytes.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct TelemetryEvent {
    /// Event type discriminant.
    pub event_type: u8,
    /// Logical CPU ID that generated this event.
    pub cpu_id: u8,
    /// Reserved / flags.
    pub _reserved: u16,
    /// TSC timestamp converted to nanoseconds via calibrated multiplier.
    pub tsc_ns: u64,
    /// Event payload — interpreted based on `event_type`.
    pub payload: EventPayload,
}

/// Union of all possible event payloads.
///
/// Only the field corresponding to `event_type` is valid.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub union EventPayload {
    pub energy: EnergyPayload,
    pub psi: PsiPayload,
    pub sched_wait: SchedWaitPayload,
    pub sched_switch: SchedSwitchPayload,
    pub marker: MarkerPayload,
    /// Raw bytes for zero-copy serialization.
    pub raw: [u8; 16],
}

/// Energy sample payload — raw RAPL counter value.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct EnergyPayload {
    /// Raw `MSR_PKG_ENERGY_STATUS` value (32-bit counter, zero-extended to 64).
    pub rapl_raw: u64,
    /// Cumulative rollover count (to handle 32-bit wrap).
    pub rollover_count: u32,
    pub _pad: u32,
}

/// PSI total counter payload.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct PsiPayload {
    /// Raw PSI total microseconds (monotonic since boot).
    pub total_us: u64,
    /// PSI resource type: 0=cpu, 1=io.
    pub resource: u32,
    pub _pad: u32,
}

/// Task wait latency payload (from sched_stat_wait tracepoint).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SchedWaitPayload {
    /// PID of the waking task.
    pub pid: u32,
    /// PID of the previously running task (preemptor).
    pub prev_pid: u32,
    /// Wait time in nanoseconds.
    pub wait_ns: u64,
}

/// Context switch payload (from sched_switch tracepoint).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SchedSwitchPayload {
    /// PID of the task being switched out.
    pub prev_pid: u32,
    /// PID of the task being switched in.
    pub next_pid: u32,
    /// State of the previous task (TASK_RUNNING, TASK_INTERRUPTIBLE, etc.).
    pub prev_state: u64,
}

/// Measurement window marker payload.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MarkerPayload {
    /// Marker type (START / STOP / ABORT).
    pub marker_type: u8,
    pub _pad: [u8; 7],
}

// Ensure the struct is exactly 40 bytes
const _: () = assert!(std::mem::size_of::<TelemetryEvent>() == 40);

/// Validate that our Rust types match the expected BPF C layout.
///
/// This function is called at initialization to catch any struct
/// layout mismatches between the BPF program and user-space.
pub fn validate_layout() -> Result<(), String> {
    let size = std::mem::size_of::<TelemetryEvent>();
    if size != 40 {
        return Err(format!(
            "TelemetryEvent size mismatch: expected 40, got {size}. \
             Check #[repr(C, packed)] layout matches BPF C struct."
        ));
    }

    // Verify field offsets
    let base = std::ptr::null::<TelemetryEvent>();
    unsafe {
        let event_type_offset = std::ptr::addr_of!((*base).event_type) as usize;
        let cpu_id_offset = std::ptr::addr_of!((*base).cpu_id) as usize;
        let tsc_ns_offset = std::ptr::addr_of!((*base).tsc_ns) as usize;
        let payload_offset = std::ptr::addr_of!((*base).payload) as usize;

        assert_eq!(event_type_offset, 0, "event_type offset");
        assert_eq!(cpu_id_offset, 1, "cpu_id offset");
        assert_eq!(tsc_ns_offset, 4, "tsc_ns offset");
        assert_eq!(payload_offset, 12, "payload offset");
    }

    Ok(())
}
