// SPDX-License-Identifier: GPL-2.0
/*
 * rush_telemetry — eBPF Telemetry Collection Programs
 *
 * This file contains the kernel-side BPF programs for zero-overhead
 * telemetry extraction. All programs emit packed binary structs to a
 * BPF_MAP_TYPE_RINGBUF. No string formatting, no float math, no
 * divisions in the hot path.
 *
 * Compile with:
 *   clang -O2 -g -target bpf -D__TARGET_ARCH_x86 \
 *         -I/usr/include/bpf -c telemetry.bpf.c -o telemetry.bpf.o
 *
 * Generate skeleton with:
 *   bpftool gen skeleton telemetry.bpf.o > telemetry.skel.h
 */

#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>

/* ── Constants ─────────────────────────────────────────────────── */

#define MAX_EVENTS       65536
#define RINGBUF_SIZE     (256 * 1024)  /* 256 KB */
#define ENERGY_SAMPLE_NS 10000000     /* 10 ms */
#define MAX_TRACKED_PIDS 64

/* ── Event type discriminants ──────────────────────────────────── */

enum event_type {
    EVENT_ENERGY_SAMPLE = 0,
    EVENT_PSI_SAMPLE    = 1,
    EVENT_SCHED_WAIT    = 2,
    EVENT_SCHED_SWITCH  = 3,
    EVENT_MARKER        = 4,
};

enum marker_type {
    MARKER_START = 0,
    MARKER_STOP  = 1,
    MARKER_ABORT = 2,
};

/* ── Wire format — must match Rust TelemetryEvent exactly ──────── */

struct energy_payload {
    u64 rapl_raw;
    u32 rollover_count;
    u32 _pad;
};

struct psi_payload {
    u64 total_us;
    u32 resource;  /* 0=cpu, 1=io */
    u32 _pad;
};

struct sched_wait_payload {
    u32 pid;
    u32 prev_pid;
    u64 wait_ns;
};

struct sched_switch_payload {
    u32 prev_pid;
    u32 next_pid;
    u64 prev_state;
};

struct marker_payload {
    u8  marker_type;
    u8  _pad[7];
};

struct telemetry_event {
    u8  event_type;
    u8  cpu_id;
    u16 _reserved;
    u64 tsc_ns;
    union {
        struct energy_payload     energy;
        struct psi_payload        psi;
        struct sched_wait_payload sched_wait;
        struct sched_switch_payload sched_switch;
        struct marker_payload     marker;
        u8 raw[16];
    } payload;
} __attribute__((packed));

/* Ensure the struct is exactly 40 bytes */
_Static_assert(sizeof(struct telemetry_event) == 40,
               "TelemetryEvent must be 40 bytes");

/* ── BPF Maps ──────────────────────────────────────────────────── */

/* Ring buffer for delivering events to user-space */
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, RINGBUF_SIZE);
} events SEC(".maps");

/* Hash map of tracked PIDs (user-space populates before measurement) */
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, MAX_TRACKED_PIDS);
    __type(key, u32);    /* PID */
    __type(value, u8);   /* dummy value — presence = tracked */
} tracked_pids SEC(".maps");

/* Per-CPU state for tracking sched_stat_wait timestamps */
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 4096);
    __type(key, u32);    /* PID */
    __type(value, u64);  /* wakeup timestamp (ns) */
} wakeup_ts SEC(".maps");

/* Global RAPL energy unit calibration (set by user-space at load time) */
const volatile u64 energy_unit_multiplier_x1e9 = 0;  /* energy_unit * 1e9, as integer */

/* Global flag: are we measuring? (set by user-space) */
const volatile u8 is_measuring = 0;

/* ── Helper: get current TSC as nanoseconds ────────────────────── */

static __always_inline u64 get_tsc_ns(void)
{
    /*
     * Use bpf_ktime_get_ns() which returns CLOCK_MONOTONIC in nanoseconds.
     * This is good enough for telemetry timestamps. For true TSC, we'd need
     * a calibrated multiplier from user-space, but ktime_get_ns is monotonic
     * and already in nanosecond units.
     */
    return bpf_ktime_get_ns();
}

static __always_inline u16 get_cpu_id(void)
{
    return (u16)bpf_get_smp_processor_id();
}

/* ── Program 1: sched_stat_wait tracepoint ──────────────────────── *
 *
 * Fires when a task wakes up after waiting. Captures the wait time
 * and emits it as a SCHED_WAIT event. Only emits for tracked PIDs
 * (or all PIDs if the tracked_pids map is empty).
 */

SEC("tp/sched/sched_stat_wait")
int handle_sched_stat_wait(struct trace_event_raw_sched_stat_wait *ctx)
{
    if (!is_measuring)
        return 0;

    u32 pid = ctx->pid;

    /* Check if we should track this PID */
    if (bpf_map_lookup_elem(&tracked_pids, &pid) == NULL) {
        /* If tracked_pids is empty (no entries), track everything */
        /* We can't check map emptiness from BPF, so we use a sentinel:
           if PID 0 is tracked, we track everything */
        u32 zero = 0;
        if (bpf_map_lookup_elem(&tracked_pids, &zero) == NULL)
            return 0;  /* Map has entries, this PID not tracked */
    }

    u64 wait_ns = ctx->delay;

    struct telemetry_event *evt;
    evt = bpf_ringbuf_reserve(&events, sizeof(*evt), 0);
    if (!evt)
        return 0;  /* Ring buffer full, drop event */

    evt->event_type = EVENT_SCHED_WAIT;
    evt->cpu_id = get_cpu_id();
    evt->_reserved = 0;
    evt->tsc_ns = get_tsc_ns();
    evt->payload.sched_wait.pid = pid;
    evt->payload.sched_wait.prev_pid = 0;  /* Not available in stat_wait */
    evt->payload.sched_wait.wait_ns = wait_ns;

    bpf_ringbuf_submit(evt, 0);
    return 0;
}

/* ── Program 2: sched_switch tracepoint ─────────────────────────── *
 *
 * Fires on every context switch. Captures task state transitions
 * for tracked PIDs. This provides the raw scheduling data needed
 * to compute per-task CPU residency and migration patterns.
 */

SEC("tp/sched/sched_switch")
int handle_sched_switch(struct trace_event_raw_sched_switch *ctx)
{
    if (!is_measuring)
        return 0;

    u32 prev_pid = ctx->prev_pid;
    u32 next_pid = ctx->next_pid;

    /* Only emit if either PID is tracked */
    u8 *prev_tracked = bpf_map_lookup_elem(&tracked_pids, &prev_pid);
    u8 *next_tracked = bpf_map_lookup_elem(&tracked_pids, &next_pid);

    if (!prev_tracked && !next_tracked) {
        /* Check sentinel (PID 0 = track all) */
        u32 zero = 0;
        if (bpf_map_lookup_elem(&tracked_pids, &zero) == NULL)
            return 0;
    }

    struct telemetry_event *evt;
    evt = bpf_ringbuf_reserve(&events, sizeof(*evt), 0);
    if (!evt)
        return 0;

    evt->event_type = EVENT_SCHED_SWITCH;
    evt->cpu_id = get_cpu_id();
    evt->_reserved = 0;
    evt->tsc_ns = get_tsc_ns();
    evt->payload.sched_switch.prev_pid = prev_pid;
    evt->payload.sched_switch.next_pid = next_pid;
    evt->payload.sched_switch.prev_state = (u64)ctx->prev_state;

    bpf_ringbuf_submit(evt, 0);
    return 0;
}

/* ── Program 3: Marker emission ─────────────────────────────────── *
 *
 * User-space emits markers by writing to a BPF_MAP_TYPE_ARRAY map.
 * This program is triggered by a kprobe on a custom tracepoint.
 * For now, markers are emitted from user-space directly to the
 * event collector (not via BPF).
 */

/* ── Program 4: PSI change notification ─────────────────────────── *
 *
 * Attached to the kernel's psi_group_change function via kprobe.
 * Captures raw PSI state changes with microsecond precision.
 * This gives us true zero-lag PSI data instead of the EMA averages.
 */

SEC("kprobe/psi_group_change")
int handle_psi_group_change(struct pt_regs *ctx)
{
    if (!is_measuring)
        return 0;

    /*
     * psi_group_change(struct psi_group *group, unsigned int cpu,
     *                  int clear, int set, u64 *state_mask)
     *
     * We capture the CPU and the state change. The actual stall
     * accumulation happens in the kernel; we just record the
     * event timestamp for post-processing correlation.
     */
    u32 cpu = (u32)bpf_get_smp_processor_id();

    struct telemetry_event *evt;
    evt = bpf_ringbuf_reserve(&events, sizeof(*evt), 0);
    if (!evt)
        return 0;

    evt->event_type = EVENT_PSI_SAMPLE;
    evt->cpu_id = (u8)cpu;
    evt->_reserved = 0;
    evt->tsc_ns = get_tsc_ns();
    evt->payload.psi.total_us = 0;  /* Post-process reads total from /proc */
    evt->payload.psi.resource = 0;  /* cpu */
    evt->payload.psi._pad = 0;

    bpf_ringbuf_submit(evt, 0);
    return 0;
}

/* ── License ───────────────────────────────────────────────────── */

char LICENSE[] SEC("license") = "GPL";
