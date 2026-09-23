//! D2 — NVMe Identify Controller parsing and Autonomous Power State
//! Transition (APST) table construction.
//!
//! This is one narrowly-scoped, standalone slice of the storage depth
//! control package (`D2` in `docs/plans/optid-package-status.toml`; see
//! `OPTID-COMPLETION-PLAN.md`, "D2 — Complete storage depth control,
//! including NVMe APST"). Design source:
//! `docs/research/0008-nvme-apst-pcie-aspm-sata-alpm.md`.
//!
//! ## What this module does
//!
//! - Parses the fixed-format Identify Controller data structure (NVMe Base
//!   Specification, "Identify Controller data structure") far enough to
//!   recover its Power State Descriptor (PSD) array and the "APST supported"
//!   attribute (`APSTA`).
//! - Builds the Set Features data buffer that Feature ID `0x0C`
//!   (Autonomous Power State Transition) defines: for every power state, an
//!   Idle Transition Power State (ITPS) and Idle Time Prior to Transition
//!   (ITPT) pair, packed the way the spec's APST Entry structure requires.
//!
//! The table-construction algorithm's *control flow* mirrors the publicly
//! documented behavior of the Linux kernel's `nvme_configure_apst()`
//! (`drivers/nvme/host/core.c`, cited in research 0008 §1.1): walk power
//! states from deepest to shallowest, and let each state that is safe to
//! target (non-operational, and within an exit-latency ceiling the caller
//! supplies) become the transition target offered to every shallower state,
//! until a still-shallower safe target is found. That control flow is an
//! independent, from-scratch reimplementation written from the spec and
//! from a plain-English description of the kernel's behavior — no kernel
//! source is copied here. The idle-time-threshold arithmetic (see
//! `ApstTable::build`) is this module's own choice, not a transcription of
//! research 0008 §1.1's simpler `EXLAT × 2` example — the two formulas
//! differ, and reconciling them, if that turns out to matter, is a later,
//! separate decision.
//!
//! ## What this module deliberately does NOT do
//!
//! - It does not issue an NVMe ioctl, open a device node, or touch any real
//!   hardware. Every function here is a pure transformation over byte
//!   buffers or already-parsed structs. That is deliberate: the D2 packet's
//!   spec gap ("owner chooses NVMe interface") recommends "a Rust ioctl
//!   implementation with recorded fixtures; avoid parsing human text" — this
//!   module is the byte-buffer half of that recommendation. It consumes raw
//!   Identify-command output exactly as a real ioctl or a recorded fixture
//!   would produce it, never parsed `nvme-cli` text. Which interface issues
//!   that ioctl is a separate, later decision this module does not make.
//! - It is not wired into the reconciler, the sealed capability table
//!   (`capability.rs`), or `actuator.rs`'s write path. Nothing in production
//!   calls it yet; only its own tests do. Firmware-revision gating, the
//!   HDD/CNVi/active-IO guards, rollback-on-transition, and hardware
//!   promotion evidence are later, separate slices of D2 — see the D2 entry
//!   in `docs/plans/optid-package-status.toml` for exactly what remains.
//! - It does not decide whether a given exit latency is "safe" against a
//!   responsiveness contract; `max_exit_latency_us` is a plain input the
//!   caller supplies. Threading a real contract floor into that input is
//!   later wiring, not this module's job.

/// Length in bytes of the Identify Controller data structure the NVMe
/// Identify command returns (NVMe Base Specification, "Identify Controller
/// data structure").
pub(crate) const IDENTIFY_CONTROLLER_LEN: usize = 4096;

/// Byte offset of `NPSS` (Number of Power States Support), a 0's-based
/// count: `NPSS + 1` power states are defined, `PS0..=PS{NPSS}`.
const NPSS_OFFSET: usize = 263;

/// Byte offset of `APSTA` (Autonomous Power State Transition Attributes).
/// Bit 0 is "APST supported".
const APSTA_OFFSET: usize = 265;

/// Byte offset where the Power State Descriptor array begins.
const PSD_ARRAY_OFFSET: usize = 2048;

/// Size in bytes of one Power State Descriptor entry.
const PSD_ENTRY_LEN: usize = 32;

/// The largest number of power states the NPSS field can express
/// (`NPSS` is a 0's-based value; NVMe controllers implement at most 32).
const MAX_POWER_STATE_COUNT: usize = 32;

/// Feature ID for Autonomous Power State Transition (Set/Get Features).
pub(crate) const APST_FEATURE_ID: u8 = 0x0C;

/// Byte length of one APST table entry (one 64-bit little-endian word per
/// power state).
const APST_ENTRY_LEN: usize = 8;

/// Byte length of the full Set Features data buffer for feature `0x0C`:
/// one entry per possible power state.
pub(crate) const APST_TABLE_LEN: usize = MAX_POWER_STATE_COUNT * APST_ENTRY_LEN;

/// The `NOPS` (Non-Operational State) bit within a PSD's flags byte.
const PSD_FLAG_NON_OPERATIONAL: u8 = 0b0000_0010;

/// The `MPS` (Max Power Scale) bit within a PSD's flags byte: 0 = the `MP`
/// field is in centiwatts (0.01 W); 1 = the `MP` field is in 0.0001 W units.
const PSD_FLAG_MAX_POWER_SCALE_FINE: u8 = 0b0000_0001;

/// Largest value the 24-bit ITPT (Idle Time Prior to Transition, in
/// milliseconds) field of an APST table entry can hold.
const MAX_ITPT_MS: u64 = (1 << 24) - 1;

/// Why an Identify Controller buffer could not be parsed.
///
/// Both variants describe a buffer that is malformed relative to the NVMe
/// specification's fixed layout, not a live-hardware failure; nothing here
/// touches a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentifyParseError {
    /// The buffer is shorter than the spec-mandated 4096-byte Identify
    /// Controller data structure, so the PSD array (which starts at byte
    /// 2048) cannot be read safely.
    Truncated { expected: usize, actual: usize },
    /// `NPSS` claims more power states than the NVMe power-state space
    /// (0..=31) can hold.
    PowerStateCountOutOfRange { npss: u8 },
}

/// One Power State Descriptor (PSD), decoded from the Identify Controller
/// data structure. Fields not needed by any current or near-term consumer
/// (`IDLP`, `ACTP`, and their scale bits) are intentionally left unparsed;
/// add them when something reads them, per the project's simplicity rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PowerStateDescriptor {
    /// Index into the PSD array (`0` is `PS0`, the maximum-performance
    /// state).
    pub(crate) index: u8,
    /// `NOPS` — true if the controller cannot process I/O commands while in
    /// this state and must be woken first. Only a non-operational state is
    /// a valid APST transition target (research 0008 §1.1).
    pub(crate) non_operational: bool,
    /// Maximum power this state draws, normalized to microwatts regardless
    /// of the PSD's own scale bit.
    pub(crate) max_power_microwatts: u32,
    /// `ENLAT` — time to enter this state from the preceding state,
    /// microseconds.
    pub(crate) entry_latency_us: u32,
    /// `EXLAT` — time to fully exit this state back to `PS0`, microseconds.
    /// This is the value the responsiveness-contract exit-latency gate
    /// uses (research 0008 §1.1).
    pub(crate) exit_latency_us: u32,
    /// `RRT` — Relative Read Throughput (0 = best; larger is worse).
    pub(crate) relative_read_throughput: u8,
    /// `RRL` — Relative Read Latency (0 = best; larger is worse).
    pub(crate) relative_read_latency: u8,
    /// `RWT` — Relative Write Throughput (0 = best; larger is worse).
    pub(crate) relative_write_throughput: u8,
    /// `RWL` — Relative Write Latency (0 = best; larger is worse).
    pub(crate) relative_write_latency: u8,
}

/// The parts of an Identify Controller response this module cares about:
/// whether the controller declares APST support, and its power states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControllerPowerInfo {
    /// `APSTA` bit 0 — true if the controller supports the Autonomous Power
    /// State Transition feature (`0x0C`) at all. A caller must check this
    /// before submitting a table built by [`ApstTable::build`]: this module
    /// builds a table unconditionally from whatever PSDs it is given, since
    /// deciding whether to *use* that table is a later, separate step.
    pub(crate) apst_supported: bool,
    /// `PS0..=PS{NPSS}`, in ascending index order, one entry per state the
    /// controller actually declares (never more than 32).
    pub(crate) power_states: Vec<PowerStateDescriptor>,
}

/// Parses an Identify Controller buffer (the raw 4096-byte response an
/// Identify command with `CNS = 01h` returns, or an equivalent recorded
/// fixture) into [`ControllerPowerInfo`].
pub(crate) fn parse_identify_controller(
    buf: &[u8],
) -> Result<ControllerPowerInfo, IdentifyParseError> {
    if buf.len() < IDENTIFY_CONTROLLER_LEN {
        return Err(IdentifyParseError::Truncated {
            expected: IDENTIFY_CONTROLLER_LEN,
            actual: buf.len(),
        });
    }

    let npss = buf[NPSS_OFFSET];
    let state_count = usize::from(npss) + 1;
    if state_count > MAX_POWER_STATE_COUNT {
        return Err(IdentifyParseError::PowerStateCountOutOfRange { npss });
    }

    let apst_supported = buf[APSTA_OFFSET] & 0x01 != 0;

    let mut power_states = Vec::with_capacity(state_count);
    for state in 0..state_count {
        let start = PSD_ARRAY_OFFSET + state * PSD_ENTRY_LEN;
        let entry = &buf[start..start + PSD_ENTRY_LEN];
        power_states.push(parse_power_state_descriptor(entry, state as u8));
    }

    Ok(ControllerPowerInfo {
        apst_supported,
        power_states,
    })
}

/// Decodes one 32-byte Power State Descriptor entry. `bytes` must be
/// exactly [`PSD_ENTRY_LEN`] long; callers within this module always slice
/// it that way.
fn parse_power_state_descriptor(bytes: &[u8], index: u8) -> PowerStateDescriptor {
    debug_assert_eq!(bytes.len(), PSD_ENTRY_LEN);

    let max_power_raw = u16::from_le_bytes([bytes[0], bytes[1]]);
    let flags = bytes[3];
    let entry_latency_us = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let exit_latency_us = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let relative_read_throughput = bytes[12] & 0b0001_1111;
    let relative_read_latency = bytes[13] & 0b0001_1111;
    let relative_write_throughput = bytes[14] & 0b0001_1111;
    let relative_write_latency = bytes[15] & 0b0001_1111;

    // MP is in centiwatts (0.01 W) unless MPS says 0.0001 W units.
    let max_power_microwatts = if flags & PSD_FLAG_MAX_POWER_SCALE_FINE != 0 {
        u32::from(max_power_raw) * 100
    } else {
        u32::from(max_power_raw) * 10_000
    };

    PowerStateDescriptor {
        index,
        non_operational: flags & PSD_FLAG_NON_OPERATIONAL != 0,
        max_power_microwatts,
        entry_latency_us,
        exit_latency_us,
        relative_read_throughput,
        relative_read_latency,
        relative_write_throughput,
        relative_write_latency,
    }
}

/// One decoded APST table entry: the state to autonomously transition into,
/// and how long the controller must be idle first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ApstEntry {
    /// `ITPS` — Idle Transition Power State: the (non-operational) state to
    /// transition into.
    pub(crate) target_state: u8,
    /// `ITPT` — Idle Time Prior to Transition, milliseconds.
    pub(crate) idle_time_before_transition_ms: u32,
}

/// The 32-entry Autonomous Power State Transition table (Set Features,
/// Feature ID `0x0C`, data buffer).
///
/// A zero-valued entry means "no transition configured for this state" —
/// the same sentinel `nvme_configure_apst()` uses a zero-initialized
/// `target` for. `ITPS = 0` naming `PS0` (the maximum-power state) as a
/// transition target with `ITPT = 0` would be nonsensical, so this is an
/// unambiguous "unset" marker, not a real target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ApstTable {
    entries: [u64; MAX_POWER_STATE_COUNT],
}

impl ApstTable {
    /// Builds the APST table for `power_states` (as parsed from an Identify
    /// Controller buffer, in ascending index order).
    ///
    /// `max_exit_latency_us` is a ceiling below which a state's exit
    /// latency must fall for that state to be offered as a transition
    /// target to any shallower state. This module does not decide what
    /// that ceiling should be — pass `u32::MAX` for "no ceiling" — a later,
    /// separate step is responsible for threading in a real responsiveness
    /// contract floor.
    ///
    /// Algorithm: walk `power_states` from the deepest (highest index) to
    /// the shallowest (`PS0`). Before evaluating each state, if a
    /// still-shallower qualifying non-operational state has already been
    /// found (from an earlier, deeper iteration), record it as this
    /// state's entry — so every state points at the *shallowest* qualifying
    /// non-operational state deeper than it has been discovered so far,
    /// letting transitions cascade to deeper states as idle time grows.
    /// Then check whether the current state itself qualifies to become the
    /// target offered to shallower states: it must be non-operational, and
    /// its exit latency must not exceed `max_exit_latency_us`. A qualifying
    /// state's idle-time threshold, in milliseconds, is
    /// `(exit_latency_us + entry_latency_us)` — a microsecond value —
    /// divided by 20 and rounded up. Because 1 ms is 1000 us, dividing by
    /// 20 rather than 1000 sets the millisecond threshold to fifty times
    /// the state's own round-trip latency (equivalently, that latency is
    /// about two percent of the chosen idle threshold), capped at the
    /// 24-bit ITPT field's maximum.
    pub(crate) fn build(power_states: &[PowerStateDescriptor], max_exit_latency_us: u32) -> Self {
        let mut entries = [0u64; MAX_POWER_STATE_COUNT];
        let mut target: u64 = 0;

        for ps in power_states.iter().rev() {
            let slot = usize::from(ps.index);
            if slot >= MAX_POWER_STATE_COUNT {
                // A malformed index (should not happen for a table built
                // from `parse_identify_controller`'s own output) is simply
                // ignored rather than panicking; this function only
                // constructs data, it never actuates.
                continue;
            }
            if target != 0 {
                entries[slot] = target;
            }

            if !ps.non_operational {
                continue;
            }
            if ps.exit_latency_us > max_exit_latency_us {
                continue;
            }

            let total_latency_us = u64::from(ps.exit_latency_us) + u64::from(ps.entry_latency_us);
            let transition_ms = total_latency_us.div_ceil(20).min(MAX_ITPT_MS);
            target = (u64::from(ps.index) << 3) | (transition_ms << 8);
        }

        Self { entries }
    }

    /// True if at least one entry is set — i.e. the table is worth
    /// submitting with `APSTE = 1`. An all-zero table (no non-operational
    /// state qualified) means APST would have nothing to do.
    pub(crate) fn feature_enabled(&self) -> bool {
        self.entries.iter().any(|&entry| entry != 0)
    }

    /// The decoded entry for `state`, or `None` if that state has no
    /// transition configured (either `state` is out of range, or its raw
    /// entry is the zero sentinel).
    pub(crate) fn entry_for(&self, state: u8) -> Option<ApstEntry> {
        let raw = *self.entries.get(usize::from(state))?;
        decode_entry(raw)
    }

    /// Encodes the table as the 256-byte little-endian Set Features data
    /// buffer Feature ID `0x0C` defines: one 64-bit word per power state,
    /// `ITPS` in bits 7:3 and `ITPT` in bits 31:8.
    pub(crate) fn to_set_features_payload(self) -> [u8; APST_TABLE_LEN] {
        let mut payload = [0u8; APST_TABLE_LEN];
        for (index, raw) in self.entries.iter().enumerate() {
            let start = index * APST_ENTRY_LEN;
            payload[start..start + APST_ENTRY_LEN].copy_from_slice(&raw.to_le_bytes());
        }
        payload
    }
}

fn decode_entry(raw: u64) -> Option<ApstEntry> {
    if raw == 0 {
        return None;
    }
    let target_state = ((raw >> 3) & 0b0001_1111) as u8;
    let idle_time_before_transition_ms = ((raw >> 8) & MAX_ITPT_MS) as u32;
    Some(ApstEntry {
        target_state,
        idle_time_before_transition_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a synthetic (not captured from real hardware) Identify
    /// Controller buffer with the given power states and APST-supported
    /// flag. `states` gives `(non_operational, entry_latency_us,
    /// exit_latency_us)` for `PS0..`.
    fn synthetic_identify_controller(states: &[(bool, u32, u32)], apst_supported: bool) -> Vec<u8> {
        assert!(!states.is_empty());
        assert!(states.len() <= MAX_POWER_STATE_COUNT);
        let mut buf = vec![0u8; IDENTIFY_CONTROLLER_LEN];
        buf[NPSS_OFFSET] = (states.len() - 1) as u8;
        buf[APSTA_OFFSET] = if apst_supported { 0x01 } else { 0x00 };
        for (index, &(non_operational, entry_latency_us, exit_latency_us)) in
            states.iter().enumerate()
        {
            let start = PSD_ARRAY_OFFSET + index * PSD_ENTRY_LEN;
            // MP = 1000 centiwatts (10 W); MPS = 0 (centiwatt units).
            buf[start..start + 2].copy_from_slice(&1000u16.to_le_bytes());
            buf[start + 3] = if non_operational {
                PSD_FLAG_NON_OPERATIONAL
            } else {
                0
            };
            buf[start + 4..start + 8].copy_from_slice(&entry_latency_us.to_le_bytes());
            buf[start + 8..start + 12].copy_from_slice(&exit_latency_us.to_le_bytes());
        }
        buf
    }

    #[test]
    fn parses_multi_psd_buffer_with_apst_supported() {
        // PS0-PS2 operational (0 latency, as real controllers report for
        // the always-on state), PS3 and PS4 non-operational.
        let buf = synthetic_identify_controller(
            &[
                (false, 0, 0),
                (false, 0, 0),
                (false, 0, 0),
                (true, 2_000, 1_000),
                (true, 20_000, 10_000),
            ],
            true,
        );

        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        assert!(info.apst_supported);
        assert_eq!(info.power_states.len(), 5);
        assert_eq!(info.power_states[0].index, 0);
        assert!(!info.power_states[0].non_operational);
        assert_eq!(info.power_states[4].index, 4);
        assert!(info.power_states[4].non_operational);
        assert_eq!(info.power_states[4].entry_latency_us, 20_000);
        assert_eq!(info.power_states[4].exit_latency_us, 10_000);
        assert_eq!(info.power_states[3].exit_latency_us, 1_000);
    }

    #[test]
    fn builds_apst_table_pointing_shallower_states_at_shallowest_qualifying_target() {
        let buf = synthetic_identify_controller(
            &[
                (false, 0, 0),
                (false, 0, 0),
                (false, 0, 0),
                (true, 2_000, 1_000),
                (true, 20_000, 10_000),
            ],
            true,
        );
        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        let table = ApstTable::build(&info.power_states, u32::MAX);

        assert!(table.feature_enabled());

        // PS4 is the deepest state: nothing deeper to point at, so it gets
        // no entry of its own (it is a candidate target for shallower
        // states, not a source).
        assert_eq!(table.entry_for(4), None);

        // PS3 is shallower than PS4 and non-operational, so once idle it
        // cascades on into PS4. The idle-time threshold uses the *target*
        // state's (PS4's) own round-trip latency, not PS3's: entry_lat
        // 20_000 + exit_lat 10_000 = 30_000us, (30_000 + 19) / 20 = 1_500ms.
        let ps3_entry = table.entry_for(3).expect("PS3 has a cascade entry");
        assert_eq!(ps3_entry.target_state, 4);
        assert_eq!(ps3_entry.idle_time_before_transition_ms, 1_500);

        // PS0-PS2 are operational and shallower than PS3, which is itself
        // the shallowest qualifying non-operational state, so they all
        // transition to PS3 first: total latency 1_000 + 2_000 = 3_000us,
        // (3_000 + 19) / 20 = 150 ms.
        for state in 0..3u8 {
            let entry = table
                .entry_for(state)
                .unwrap_or_else(|| panic!("PS{state} has an entry"));
            assert_eq!(entry.target_state, 3, "PS{state} should target PS3");
            assert_eq!(entry.idle_time_before_transition_ms, 150);
        }
    }

    #[test]
    fn exit_latency_ceiling_excludes_a_deep_state() {
        // Same shape as above, but the caller's ceiling excludes PS4
        // (10_000us > 5_000us ceiling); only PS3 may be used.
        let buf = synthetic_identify_controller(
            &[(false, 0, 0), (true, 2_000, 1_000), (true, 20_000, 10_000)],
            true,
        );
        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        let table = ApstTable::build(&info.power_states, 5_000);

        assert_eq!(table.entry_for(2), None, "PS2 exceeds the ceiling");
        let ps0_entry = table.entry_for(0).expect("PS0 still targets PS1");
        assert_eq!(ps0_entry.target_state, 1);
    }

    #[test]
    fn all_operational_states_produce_a_disabled_table() {
        // A controller with no non-operational states at all (uncommon,
        // but not malformed) has nothing for APST to do.
        let buf = synthetic_identify_controller(&[(false, 0, 0), (false, 0, 0)], true);
        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        let table = ApstTable::build(&info.power_states, u32::MAX);

        assert!(!table.feature_enabled());
        assert_eq!(table.entry_for(0), None);
        assert_eq!(table.entry_for(1), None);
    }

    #[test]
    fn single_power_state_has_no_transition_target() {
        let buf = synthetic_identify_controller(&[(false, 0, 0)], true);
        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        let table = ApstTable::build(&info.power_states, u32::MAX);
        assert!(!table.feature_enabled());
    }

    #[test]
    fn parses_buffer_with_apst_unsupported() {
        let buf = synthetic_identify_controller(&[(false, 0, 0), (true, 2_000, 1_000)], false);
        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        assert!(!info.apst_supported);
        // A caller must still be able to build the table itself (it is a
        // pure data transform); whether to *submit* it is a separate,
        // later decision that belongs to the code that checks
        // `apst_supported`.
        assert_eq!(info.power_states.len(), 2);
    }

    #[test]
    fn rejects_truncated_buffer() {
        let buf = vec![0u8; 100];
        let error = parse_identify_controller(&buf).unwrap_err();
        assert_eq!(
            error,
            IdentifyParseError::Truncated {
                expected: IDENTIFY_CONTROLLER_LEN,
                actual: 100,
            }
        );
    }

    #[test]
    fn rejects_impossible_power_state_count() {
        let mut buf = vec![0u8; IDENTIFY_CONTROLLER_LEN];
        buf[NPSS_OFFSET] = 250; // 251 states claimed; max is 32.
        let error = parse_identify_controller(&buf).unwrap_err();
        assert_eq!(
            error,
            IdentifyParseError::PowerStateCountOutOfRange { npss: 250 }
        );
    }

    #[test]
    fn set_features_payload_round_trips_through_decode() {
        let buf = synthetic_identify_controller(&[(false, 0, 0), (true, 2_000, 1_000)], true);
        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        let table = ApstTable::build(&info.power_states, u32::MAX);
        let payload = table.to_set_features_payload();

        assert_eq!(payload.len(), APST_TABLE_LEN);

        // Decode PS0's 8-byte entry straight out of the payload bytes and
        // check it agrees with the table's own accessor.
        let raw = u64::from_le_bytes(payload[0..8].try_into().unwrap());
        let decoded = decode_entry(raw);
        assert_eq!(decoded, table.entry_for(0));
    }

    #[test]
    fn apst_feature_id_matches_spec() {
        // Locks the Feature ID a later Set/Get Features call must use;
        // nothing else in this pure-parsing slice issues that call yet.
        assert_eq!(APST_FEATURE_ID, 0x0C);
    }

    #[test]
    fn max_power_scale_bit_changes_units() {
        let mut buf = synthetic_identify_controller(&[(false, 0, 0)], true);
        let start = PSD_ARRAY_OFFSET;
        // MP = 5 in fine (0.0001 W) units -> 500 microwatts... actually
        // 5 * 0.0001 W = 0.0005 W = 500 microwatts.
        buf[start..start + 2].copy_from_slice(&5u16.to_le_bytes());
        buf[start + 3] = PSD_FLAG_MAX_POWER_SCALE_FINE;
        let info = parse_identify_controller(&buf).expect("valid buffer parses");
        assert_eq!(info.power_states[0].max_power_microwatts, 500);
    }
}
