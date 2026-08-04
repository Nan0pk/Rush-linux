#!/usr/bin/env python3
"""Apply the bounded S4D production integration on the dedicated branch."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one exact match, found {count}: {old[:80]!r}")
    write(path, text.replace(old, new, 1))


def sub_once(path: str, pattern: str, replacement: str) -> None:
    text = read(path)
    updated, count = re.subn(pattern, replacement, text, count=1, flags=re.DOTALL)
    if count != 1:
        raise SystemExit(f"{path}: regex matched {count}: {pattern[:80]!r}")
    write(path, updated)


# The production wrapper does not need Debug, and RealKernel deliberately does
# not expose it.
replace_once(
    "crates/optid/src/capability_table.rs",
    "#[derive(Clone, Debug)]\npub(crate) struct CapabilityKernel",
    "#[derive(Clone)]\npub(crate) struct CapabilityKernel",
)

# ---------------------------------------------------------------------------
# Policy/config: migration-safe observe default, explicit enforce mode.
# ---------------------------------------------------------------------------
policy_marker = "#[derive(Debug, Clone, serde::Deserialize)]\npub(crate) struct Policy {"
policy_types = r'''#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CapabilitySealingMode {
    Observe,
    Enforce,
}

impl Default for CapabilitySealingMode {
    fn default() -> Self {
        Self::Observe
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SafetyConfig {
    #[serde(default)]
    pub(crate) capability_sealing: CapabilitySealingMode,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Policy {'''
replace_once("crates/optid/src/policy.rs", policy_marker, policy_types)

replace_once(
    "crates/optid/src/policy.rs",
    '''    #[serde(default)]
    pub(crate) thermal: crate::thermal::ThermalConfig,
}''',
    '''    #[serde(default)]
    pub(crate) thermal: crate::thermal::ThermalConfig,
    /// S4D — capability sealing is observe-only unless explicitly enforced.
    #[serde(default)]
    pub(crate) safety: SafetyConfig,
}''',
)

replace_once(
    "crates/optid/src/policy.rs",
    '''            thermal: crate::thermal::ThermalConfig::from_toml_str("mode = \\"observe\\"\\n")
                .unwrap_or_default(),
        }''',
    '''            thermal: crate::thermal::ThermalConfig::from_toml_str("mode = \\"observe\\"\\n")
                .unwrap_or_default(),
            // S4D migration-safe default: inventory only, no kernel actuation.
            safety: SafetyConfig::default(),
        }''',
)

replace_once(
    "config/optid/policy.toml",
    '''competing_policy_daemons = [
  "tlp.service",
  "power-profiles-daemon.service",
  "tuned.service",
]
''',
    '''competing_policy_daemons = [
  "tlp.service",
  "power-profiles-daemon.service",
  "tuned.service",
]

# S4D capability sealing. `observe` inventories exact targets but suppresses
# kernel writes. Set `enforce` only on kernels covered by the D0 mechanism
# proof; startup must still pass the live table/seal self-tests.
[safety]
capability_sealing = "observe"
''',
)

# ---------------------------------------------------------------------------
# Actuator: pre-open PM QoS and suppress non-systemd writes unless sealed.
# ---------------------------------------------------------------------------
replace_once(
    "crates/optid/src/actuator.rs",
    "use std::path::{Path, PathBuf};\n",
    "use std::os::unix::fs::OpenOptionsExt;\nuse std::path::{Path, PathBuf};\n",
)

pmqos_impl = r'''pub(crate) const SEALED_CPU_PM_QOS_SLOTS: usize = 8;

pub(crate) struct RealPmqosSink {
    cpu_fd: Option<fs::File>,
    cpu_spares: Vec<fs::File>,
    cpu_value: Option<i32>,
    sealed: bool,
}

impl RealPmqosSink {
    pub(crate) fn new() -> Self {
        Self {
            cpu_fd: None,
            cpu_spares: Vec::new(),
            cpu_value: None,
            sealed: false,
        }
    }

    fn open_cpu_descriptor() -> io::Result<fs::File> {
        fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open("/dev/cpu_dma_latency")
    }

    /// Open a bounded pool before Landlock. Closing the active request is the
    /// exact handback contract; a later request consumes another pre-opened
    /// descriptor instead of reopening the device after sealing.
    pub(crate) fn preopen_for_sealing() -> io::Result<Self> {
        let mut cpu_spares = Vec::new();
        for _ in 0..SEALED_CPU_PM_QOS_SLOTS {
            match Self::open_cpu_descriptor() {
                Ok(file) => cpu_spares.push(file),
                Err(error) if error.kind() == io::ErrorKind::NotFound => break,
                Err(error) => return Err(error),
            }
        }
        Ok(Self {
            cpu_fd: None,
            cpu_spares,
            cpu_value: None,
            sealed: true,
        })
    }
}

impl PmqosSink for RealPmqosSink {
    fn read_cpu_latency(&self) -> io::Result<String> {
        if self.sealed {
            Ok(self
                .cpu_value
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unconstrained".to_string()))
        } else {
            fs::read_to_string("/dev/cpu_dma_latency")
        }
    }

    fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
        use std::io::Write;
        match value {
            Some(value) => {
                if self.cpu_fd.is_none() {
                    self.cpu_fd = if self.sealed {
                        self.cpu_spares.pop()
                    } else {
                        Some(Self::open_cpu_descriptor()?)
                    };
                }
                let file = self.cpu_fd.as_mut().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "sealed CPU PM QoS descriptor pool is exhausted or unavailable",
                    )
                })?;
                file.write_all(&value.to_ne_bytes())?;
                file.flush()?;
                self.cpu_value = Some(value);
            }
            None => {
                self.cpu_fd = None;
                self.cpu_value = None;
            }
        }
        Ok(())
    }

    fn read_device_latency(&self, device_path: &Path) -> io::Result<String> {
        fs::read_to_string(device_path)
    }

    fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()> {
        crate::kernel_io::RealKernel::new().write(device_path, value)
    }
}

pub(crate) struct Actuator'''
sub_once(
    "crates/optid/src/actuator.rs",
    r"pub\(crate\) struct RealPmqosSink \{.*?\n\}\n\npub\(crate\) struct Actuator",
    pmqos_impl,
)

replace_once(
    "crates/optid/src/actuator.rs",
    '''    /// Correlation ID for the current control-loop iteration.
    pub(crate) correlation_id: String,
''',
    '''    /// Correlation ID for the current control-loop iteration.
    pub(crate) correlation_id: String,
    /// S4D production gate. `None` preserves legacy/unit-test construction;
    /// main always sets `Some(false|true)` before entering the control loop.
    pub(crate) capability_sealing_enforced: Option<bool>,
''',
)

text = read("crates/optid/src/actuator.rs")
needle = "            correlation_id: String::new(),\n"
if text.count(needle) != 2:
    raise SystemExit(f"actuator constructors: expected two correlation fields, found {text.count(needle)}")
text = text.replace(
    needle,
    needle + "            capability_sealing_enforced: None,\n",
)
write("crates/optid/src/actuator.rs", text)

replace_once(
    "crates/optid/src/actuator.rs",
    '''    pub(crate) fn set_boot_state(&mut self, boot_state: BootState) {
        self.boot_state = Some(boot_state);
    }
''',
    '''    pub(crate) fn set_boot_state(&mut self, boot_state: BootState) {
        self.boot_state = Some(boot_state);
    }

    pub(crate) fn set_capability_sealing_enforced(&mut self, enforced: bool) {
        self.capability_sealing_enforced = Some(enforced);
    }
''',
)

replace_once(
    "crates/optid/src/actuator.rs",
    '''        if apply_denied {
            outcome.targets.push(TargetOutcome::denied(
                action.stable_target_id(),
                PipelineStage::ApplyGate,
                "dynamic writes are disarmed".to_string(),
            ));
            return Ok(outcome);
        }

        let contract_gate = self.contract_gate(action)?;
''',
    '''        if apply_denied {
            outcome.targets.push(TargetOutcome::denied(
                action.stable_target_id(),
                PipelineStage::ApplyGate,
                "dynamic writes are disarmed".to_string(),
            ));
            return Ok(outcome);
        }

        if !matches!(action, Action::SystemdSetProperty { .. })
            && self.capability_sealing_enforced == Some(false)
        {
            outcome.gates.push(GateEvaluation::denied(
                GateStage::CapabilityValidation,
                GateReasonCode::CapabilityDenied,
                "S4D capability sealing is observe-only or startup sealing failed",
            ));
            outcome.targets.push(TargetOutcome::denied(
                action.stable_target_id(),
                PipelineStage::CapabilityValidation,
                "kernel write requires an enforced pre-opened capability table".to_string(),
            ));
            return Ok(outcome);
        }

        let contract_gate = self.contract_gate(action)?;
''',
)

actuator = read("crates/optid/src/actuator.rs")
actuator, read_count = re.subn(
    r"self\.pmqos_sink\s*\.read_device_latency\(path\)",
    "self.kernel.read_to_string(path)",
    actuator,
)
actuator, write_count = re.subn(
    r"self\.pmqos_sink\s*\.write_device_latency\(path, &value_string\)",
    "self.kernel.write(path, &value_string)",
    actuator,
)
if read_count < 3 or write_count != 1:
    raise SystemExit(
        f"device PM QoS migration mismatch: reads={read_count} writes={write_count}"
    )
write("crates/optid/src/actuator.rs", actuator)

restore = read("crates/optid/src/reconciler/restore.rs")
restore, read_count = re.subn(
    r"actuator\s*\.pmqos_sink\s*\.read_device_latency\(path\)\?",
    "actuator.kernel.read_to_string(path)?",
    restore,
)
restore, write_count = re.subn(
    r"actuator\.pmqos_sink\.write_device_latency\(path, value\)",
    "actuator.kernel.write(path, value)",
    restore,
)
if read_count != 1 or write_count != 1:
    raise SystemExit(f"restore PM QoS migration mismatch: reads={read_count} writes={write_count}")
write("crates/optid/src/reconciler/restore.rs", restore)

# ---------------------------------------------------------------------------
# Startup: seal before threads, use shared descriptor table, cold rebuild 75.
# ---------------------------------------------------------------------------
replace_once(
    "crates/optid/src/main.rs",
    "mod capability;\n",
    "mod capability;\nmod capability_table;\n",
)
replace_once(
    "crates/optid/src/main.rs",
    "use actuator::Actuator;\n",
    "use actuator::{Actuator, PmqosSink, RealPmqosSink};\n",
)
replace_once(
    "crates/optid/src/main.rs",
    "use contracts::Contracts;\n",
    '''use capability_table::{
    topology_fingerprint, CapabilityKernel, CapabilityTable, TopologyDebouncer,
    TopologyDecision, EXIT_TOPOLOGY_REBUILD,
};
use contracts::Contracts;
''',
)
replace_once(
    "crates/optid/src/main.rs",
    "use kernel_io::Clock;\n",
    "use kernel_io::{Clock, KernelIo, RealKernel};\n",
)
replace_once(
    "crates/optid/src/main.rs",
    "use policy::Policy;\n",
    "use policy::{CapabilitySealingMode, Policy};\n",
)

replace_once(
    "crates/optid/src/main.rs",
    '''    if let Err(err) = run(args) {
        eprintln!("optid: {err}");
        std::process::exit(1);
    }
}
''',
    '''    match run(args) {
        Ok(RunExit::Clean) => {}
        Ok(RunExit::TopologyRebuild) => std::process::exit(EXIT_TOPOLOGY_REBUILD),
        Err(err) => {
            eprintln!("optid: {err}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunExit {
    Clean,
    TopologyRebuild,
}
''',
)
replace_once(
    "crates/optid/src/main.rs",
    "fn run(args: Args) -> io::Result<()> {\n",
    "fn run(args: Args) -> io::Result<RunExit> {\n",
)

replace_once(
    "crates/optid/src/main.rs",
    '''    spawn_dbus_servers(
        args.state_dir.clone(),
        &policy_for_conflicts,
        conflict_report.is_blocking(),
    );

''',
    "",
)

startup = r'''    let discovery_kernel = RealKernel::new();
    let startup_snapshot = Snapshot::collect_with_thermal(
        &discovery_kernel,
        &discovery_kernel,
        &policy_for_conflicts.thermal,
        None,
    );
    let startup_topology = topology_fingerprint(&discovery_kernel, &startup_snapshot);
    let mut topology_debouncer = TopologyDebouncer::new(startup_topology);

    let mut actuator_kernel: Box<dyn KernelIo> = Box::new(RealKernel::new());
    let mut cycle_kernel: Box<dyn KernelIo> = Box::new(RealKernel::new());
    let mut pmqos_sink: Box<dyn PmqosSink> = Box::new(RealPmqosSink::new());
    let mut capability_sealing_enforced = false;

    if apply_armed
        && policy_for_conflicts.safety.capability_sealing == CapabilitySealingMode::Enforce
    {
        match (
            CapabilityTable::from_snapshot(&discovery_kernel, &startup_snapshot),
            RealPmqosSink::preopen_for_sealing(),
        ) {
            (Ok(table), Ok(sealed_pmqos)) => {
                let table = Arc::new(table);
                let state_roots = vec![args.state_dir.clone(), PathBuf::from("/var/lib/optid")];
                match table.seal(&state_roots) {
                    Ok(report) => {
                        let sealed_kernel = CapabilityKernel::new(Arc::clone(&table));
                        actuator_kernel = Box::new(sealed_kernel.clone());
                        cycle_kernel = Box::new(sealed_kernel);
                        pmqos_sink = Box::new(sealed_pmqos);
                        capability_sealing_enforced = true;
                        let message = format!(
                            "optid: S4D seal enforced — capabilities={} Landlock ABI={} rights=0x{:x} new_write_open_denied={} state_write_allowed={}\n",
                            report.capability_count,
                            report.landlock_abi,
                            report.handled_rights,
                            report.new_hardware_write_open_denied,
                            report.state_write_allowed,
                        );
                        eprint!("{message}");
                        append_log(&args.state_dir.join("decisions.log"), &message)?;
                    }
                    Err(error) => {
                        let message = format!(
                            "optid: S4D enforce requested but sealing failed; kernel writes remain observe-only: {error}\n"
                        );
                        eprint!("{message}");
                        let _ = append_log(&args.state_dir.join("decisions.log"), &message);
                    }
                }
            }
            (Err(error), _) => {
                let message = format!(
                    "optid: S4D capability-table construction failed; kernel writes remain observe-only: {error}\n"
                );
                eprint!("{message}");
                append_log(&args.state_dir.join("decisions.log"), &message)?;
            }
            (_, Err(error)) => {
                let message = format!(
                    "optid: S4D CPU PM QoS pre-open failed; kernel writes remain observe-only: {error}\n"
                );
                eprint!("{message}");
                append_log(&args.state_dir.join("decisions.log"), &message)?;
            }
        }
    } else {
        let reason = if !apply_armed {
            "apply is not armed"
        } else {
            "[safety].capability_sealing=observe"
        };
        let message = format!(
            "optid: S4D capability sealing observe-only ({reason}); non-systemd kernel writes suppressed\n"
        );
        eprint!("{message}");
        append_log(&args.state_dir.join("decisions.log"), &message)?;
    }

    let mut actuator = Actuator::new_with_kernel(args.state_dir.clone(), actuator_kernel);
    actuator.pmqos_sink = pmqos_sink;'''
replace_once(
    "crates/optid/src/main.rs",
    "    let mut actuator = Actuator::new(args.state_dir.clone());",
    startup,
)

replace_once(
    "crates/optid/src/main.rs",
    '''    actuator.set_boot_state(boot_state.clone());

    let cycle_kernel = kernel_io::RealKernel::new();
''',
    '''    actuator.set_boot_state(boot_state.clone());
    actuator.set_capability_sealing_enforced(capability_sealing_enforced);

''',
)

replace_once(
    "crates/optid/src/main.rs",
    '''    if args.foreground == args::ForegroundMode::Auto {
''',
    '''    spawn_dbus_servers(
        args.state_dir.clone(),
        &policy_for_conflicts,
        conflict_report.is_blocking(),
    );

    if args.foreground == args::ForegroundMode::Auto {
''',
)

replace_once(
    "crates/optid/src/main.rs",
    '''        let policy = Policy::load(&args.config_path);
        let kernel = cycle_kernel.clone();
        let mut snapshot = Snapshot::collect_with_thermal(
            &kernel,
            &kernel,
            &policy.thermal,
            previous_thermal_budget.as_ref(),
        );
        previous_thermal_budget = Some(snapshot.thermal_budget.clone());
''',
    '''        let policy = Policy::load(&args.config_path);
        let mut snapshot = Snapshot::collect_with_thermal(
            cycle_kernel.as_ref(),
            cycle_kernel.as_ref(),
            &policy.thermal,
            previous_thermal_budget.as_ref(),
        );
        previous_thermal_budget = Some(snapshot.thermal_budget.clone());

        match topology_debouncer.observe(topology_fingerprint(
            cycle_kernel.as_ref(),
            &snapshot,
        )) {
            TopologyDecision::Stable => {}
            TopologyDecision::Pending { observations } => {
                append_log(
                    &args.state_dir.join("decisions.log"),
                    &format!(
                        "optid: S4D topology change pending observations={observations}; new targets remain observe-only\n"
                    ),
                )?;
            }
            TopologyDecision::Rebuild => {
                append_log(
                    &args.state_dir.join("decisions.log"),
                    "optid: S4D stable topology change; handing back owned targets before cold rebuild\n",
                )?;
                let handbacks = reconciler.restore_all_owned(&mut actuator)?;
                for outcome in handbacks {
                    append_log(
                        &args.state_dir.join("decisions.log"),
                        &format!(
                            "optid: S4D topology handback target={} outcome={:?}\n",
                            outcome.target_id, outcome.reason
                        ),
                    )?;
                }
                append_log(
                    &args.state_dir.join("decisions.log"),
                    "optid: S4D handback complete; requesting supervisor capability-table rebuild status=75\n",
                )?;
                return Ok(RunExit::TopologyRebuild);
            }
        }
''',
)

main = read("crates/optid/src/main.rs")
main = main.replace("&cycle_kernel", "cycle_kernel.as_ref()")
write("crates/optid/src/main.rs", main)

replace_once(
    "crates/optid/src/main.rs",
    '''    let _ = lock_file;
    Ok(())
}

fn spawn_dbus_servers''',
    '''    let _ = lock_file;
    Ok(RunExit::Clean)
}

fn spawn_dbus_servers''',
)

# ---------------------------------------------------------------------------
# Supervisor contract: status 75 rebuilds through S3D recovery.
# ---------------------------------------------------------------------------
for unit in [
    "packaging/systemd/optid-apply.service",
    "mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service",
]:
    replace_once(
        unit,
        "Restart=on-failure\nRestartSec=2\n",
        "Restart=on-failure\nRestartForceExitStatus=75\nRestartSec=2\n",
    )

print("S4D integration patch applied")
