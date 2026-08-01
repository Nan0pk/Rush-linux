//! Cross-module and production-surface tests for `optid`.

#![cfg(test)]

// Preserve the existing cross-module suite unchanged. The additional F2 test
// below lives at the binary-crate surface so it can enter through `run(args)`.
#[path = "tests_impl.rs"]
mod existing;

mod f2_production_surface {
    use std::fs;
    use std::io;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::args::{Args, ForegroundMode};
    use crate::kernel_io::{with_real_kernel_override, FaultKernel, MemoryKernel};

    #[test]
    fn f2_production_daemon_run_consumes_injected_kernel_io() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock must be after the Unix epoch")
            .as_nanos();
        let state_dir = std::env::temp_dir().join(format!(
            "optid_f2_production_surface_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&state_dir).expect("create isolated daemon state directory");

        let config_path = state_dir.join("policy.toml");
        fs::write(&config_path, "").expect("write minimal policy fixture");

        let decisions_log = state_dir.join("decisions.log");
        let fault = FaultKernel::new(Box::new(MemoryKernel::new()));
        fault.fail_next_write(decisions_log.clone(), io::ErrorKind::PermissionDenied);

        let args = Args {
            apply: false,
            once: true,
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: state_dir.clone(),
            config_path,
            allowlist: false,
            foreground: ForegroundMode::Off,
        };

        let result = with_real_kernel_override(Box::new(fault), || crate::run(args));
        let error = result.expect_err(
            "the daemon entry point must observe the fault injected through RealKernel",
        );

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            error.to_string().contains("injected append fault"),
            "the error must originate from FaultKernel, got: {error}"
        );
        assert!(
            !decisions_log.exists(),
            "bypassing the injected seam would have created decisions.log"
        );

        fs::remove_dir_all(&state_dir).expect("remove isolated daemon state directory");
    }
}

mod f3_production_surface {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::args::{Args, ForegroundMode};
    use crate::envelope::{ControlCycleEnvelope, ENVELOPE_SCHEMA_VERSION};
    use crate::kernel_io::{
        with_real_kernel_override, Clock, EventSource, KernelRead, KernelWrite, MemoryKernel,
    };
    use crate::policy::DomainMode;

    #[derive(Clone)]
    struct SharedKernel(Arc<MemoryKernel>);

    impl KernelRead for SharedKernel {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.0.read_to_string(path)
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
            self.0.read_dir(path)
        }

        fn exists(&self, path: &Path) -> bool {
            self.0.exists(path)
        }

        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            self.0.read_link(path)
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            self.0.canonicalize(path)
        }
    }

    impl KernelWrite for SharedKernel {
        fn write(&self, path: &Path, value: &str) -> io::Result<()> {
            self.0.write(path, value)
        }

        fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
            self.0.write_state_file(path, value)
        }

        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            self.0.create_dir_all(path)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.0.rename(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.0.remove_file(path)
        }

        fn append(&self, path: &Path, text: &str) -> io::Result<()> {
            self.0.append(path, text)
        }
    }

    impl Clock for SharedKernel {
        fn now_unix(&self) -> u64 {
            self.0.now_unix()
        }
    }

    impl EventSource for SharedKernel {
        fn wait(&self, duration: Duration) -> bool {
            let _ = duration;
            false
        }
    }

    #[test]
    fn f3_production_daemon_run_writes_one_correlated_versioned_cycle() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock must be after the Unix epoch")
            .as_nanos();
        let state_dir = std::env::temp_dir().join(format!(
            "optid_f3_production_surface_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&state_dir).expect("create isolated daemon state directory");

        let config_path = state_dir.join("policy.toml");
        let policy = format!(
            "{}\n[domains.cpu_epp]\nmode = \"off\"\n[domains.platform_profile]\nmode = \"observe\"\n[domains.vm_sysctl]\nmode = \"actuate\"\n",
            include_str!("../../../config/optid/policy.toml")
        );
        fs::write(&config_path, policy).expect("write F3 policy fixture");

        let memory = Arc::new(MemoryKernel::new());
        memory.advance_clock(1_700_000_000);
        let shared = SharedKernel(Arc::clone(&memory));
        let args = Args {
            apply: false,
            once: true,
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: state_dir.clone(),
            config_path,
            allowlist: false,
            foreground: ForegroundMode::Off,
        };

        with_real_kernel_override(Box::new(shared), || crate::run(args))
            .expect("real daemon run must complete through injected kernel I/O");

        let status = memory
            .read_to_string(&state_dir.join("status"))
            .expect("daemon must write human status through injected I/O");
        let status_json = memory
            .read_to_string(&state_dir.join("status.json"))
            .expect("daemon must write machine status through injected I/O");
        let history = memory
            .read_to_string(&state_dir.join("control-cycles.jsonl"))
            .expect("daemon must append machine cycle history through injected I/O");
        let decisions = memory
            .read_to_string(&state_dir.join("decisions.log"))
            .expect("daemon must append correlated text decisions through injected I/O");

        let cycle: ControlCycleEnvelope = serde_json::from_str(&status_json)
            .expect("status.json must match the versioned schema");
        assert_eq!(cycle.schema_version, ENVELOPE_SCHEMA_VERSION);
        assert_eq!(cycle.cycle_timestamp, 1_700_000_000);
        assert!(!cycle.observation.values.is_empty());
        assert!(!cycle.decision.workload_class.is_empty());
        assert!(!cycle.decision.selected_mode.is_empty());
        assert!(!cycle.decision.contract.workload_class.is_empty());

        let cpu_epp = cycle
            .domains
            .iter()
            .find(|domain| domain.domain == "cpu_epp")
            .expect("off domain must remain visible");
        let platform_profile = cycle
            .domains
            .iter()
            .find(|domain| domain.domain == "platform_profile")
            .expect("observe domain must remain visible");
        let vm_sysctl = cycle
            .domains
            .iter()
            .find(|domain| domain.domain == "vm_sysctl")
            .expect("actuate domain must remain visible");
        assert_eq!(cpu_epp.selected_mode, DomainMode::Off);
        assert_eq!(platform_profile.selected_mode, DomainMode::Observe);
        assert_eq!(vm_sysctl.selected_mode, DomainMode::Actuate);
        assert!(cpu_epp.action_outcomes.is_empty());
        let observed_action = platform_profile
            .action_outcomes
            .first()
            .expect("observe mode must retain the would-be action");
        let domain_gate = observed_action
            .gates
            .iter()
            .find(|gate| gate.stage == crate::envelope::GateStage::DomainMode)
            .expect("observe action must include the domain gate");
        assert_eq!(
            domain_gate.reason,
            crate::envelope::GateReasonCode::DomainObserve
        );
        assert!(observed_action
            .targets
            .iter()
            .all(|target| !target.write_attempted));

        let history_lines: Vec<&str> = history.lines().collect();
        assert_eq!(
            history_lines.len(),
            1,
            "one --once run must append one cycle"
        );
        let historical: ControlCycleEnvelope =
            serde_json::from_str(history_lines[0]).expect("JSONL entry must match the same schema");
        assert_eq!(historical, cycle);

        let correlation = &cycle.correlation_id;
        assert!(status.contains(correlation));
        assert!(decisions.contains(correlation));
        assert_eq!(
            correlation, "optid-000000006553f100-0000000000000000",
            "fake clock and boot sequence must make the first cycle deterministic"
        );

        fs::remove_dir_all(&state_dir).expect("remove isolated daemon state directory");
    }
}

mod f3_outcome_matrix {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use crate::action::Action;
    use crate::actuator::Actuator;
    use crate::allowlist::Allowlist;
    use crate::envelope::{
        ActionOutcome, ErrorKindCode, GateDisposition, GateReasonCode, GateStage,
        ObservationEnvelope, ObservationReasonCode, OutcomeReasonCode, OwnershipState,
        PipelineStage, ReadbackOutcome, RestoreState, SupportState, WriteOutcome,
    };
    use crate::io_util::get_path_hash;
    use crate::kernel_io::{
        Clock, EventSource, FaultKernel, KernelRead, KernelWrite, MemoryKernel,
    };
    use crate::load_state::{BootState, LoadState};
    use crate::policy::DomainMode;
    use crate::sensors::Snapshot;

    #[derive(Clone)]
    struct SharedMemory(Arc<MemoryKernel>);

    impl KernelRead for SharedMemory {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.0.read_to_string(path)
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
            self.0.read_dir(path)
        }

        fn exists(&self, path: &Path) -> bool {
            self.0.exists(path)
        }

        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            self.0.read_link(path)
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            self.0.canonicalize(path)
        }
    }

    impl KernelWrite for SharedMemory {
        fn write(&self, path: &Path, value: &str) -> io::Result<()> {
            self.0.write(path, value)
        }

        fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
            self.0.write_state_file(path, value)
        }

        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            self.0.create_dir_all(path)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.0.rename(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.0.remove_file(path)
        }

        fn append(&self, path: &Path, text: &str) -> io::Result<()> {
            self.0.append(path, text)
        }
    }

    impl Clock for SharedMemory {
        fn now_unix(&self) -> u64 {
            self.0.now_unix()
        }
    }

    impl EventSource for SharedMemory {
        fn wait(&self, duration: Duration) -> bool {
            let _ = duration;
            false
        }
    }

    fn boot_state(apply_armed: bool) -> BootState {
        BootState {
            policy_load_state: LoadState::Ok,
            allowlist_load_state: LoadState::Ok,
            apply_armed,
            baseline_armed: apply_armed,
            allowlist_gate_enabled: false,
        }
    }

    fn vm_action(value: &str) -> Action {
        Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            value.to_string(),
            "F3 outcome matrix".to_string(),
        )
    }

    fn assert_gate(
        outcome: &ActionOutcome,
        stage: GateStage,
        disposition: GateDisposition,
        reason: GateReasonCode,
    ) {
        let gate = outcome
            .gates
            .iter()
            .find(|gate| gate.stage == stage)
            .unwrap_or_else(|| panic!("missing gate {stage:?} in {outcome:?}"));
        assert_eq!(gate.disposition, disposition);
        assert_eq!(gate.reason, reason);
    }

    #[test]
    fn f3_gate_matrix_distinguishes_dry_run_observe_off_and_boot_disarm() {
        let action = vm_action("60");

        let dry_run = ActionOutcome::suppressed(&action, DomainMode::Actuate, false);
        assert_eq!(dry_run.domain, "vm_sysctl");
        assert_gate(
            &dry_run,
            GateStage::DomainMode,
            GateDisposition::Allowed,
            GateReasonCode::DomainActuate,
        );
        assert_gate(
            &dry_run,
            GateStage::ApplyArmed,
            GateDisposition::Denied,
            GateReasonCode::ApplyNotRequested,
        );
        assert_eq!(dry_run.targets[0].target_id, "vm-sysctl:swappiness");
        assert_eq!(dry_run.targets[0].pipeline_stage, PipelineStage::ApplyGate);
        assert_eq!(dry_run.targets[0].reason, OutcomeReasonCode::GateDenied);
        assert!(!dry_run.targets[0].write_attempted);

        let observe = ActionOutcome::suppressed(&action, DomainMode::Observe, true);
        assert_gate(
            &observe,
            GateStage::DomainMode,
            GateDisposition::Denied,
            GateReasonCode::DomainObserve,
        );
        assert_eq!(observe.targets[0].pipeline_stage, PipelineStage::DomainGate);
        assert!(!observe.targets[0].write_attempted);

        let off = ActionOutcome::suppressed(&action, DomainMode::Off, true);
        assert_gate(
            &off,
            GateStage::DomainMode,
            GateDisposition::Denied,
            GateReasonCode::DomainOff,
        );
        assert_eq!(off.targets[0].pipeline_stage, PipelineStage::DomainGate);
        assert!(!off.targets[0].write_attempted);

        let memory = Arc::new(MemoryKernel::new());
        let mut actuator = Actuator::new_with_kernel(
            PathBuf::from("/state/f3-boot-disarm"),
            Box::new(SharedMemory(Arc::clone(&memory))),
        );
        actuator.set_correlation_id("cycle-boot-disarm".to_string());
        actuator.set_boot_state(boot_state(false));
        let denied = actuator.apply(&action).expect("boot gate result");
        assert_gate(
            &denied,
            GateStage::ApplyArmed,
            GateDisposition::Denied,
            GateReasonCode::ApplyDisarmedByBootState,
        );
        assert_eq!(denied.targets[0].pipeline_stage, PipelineStage::ApplyGate);
        assert_eq!(denied.targets[0].target_id, "vm-sysctl:swappiness");
        assert!(!denied.targets[0].write_attempted);
    }

    #[test]
    fn f3_actuator_matrix_reports_missing_unsupported_contract_and_allowlist_denials() {
        let memory = Arc::new(MemoryKernel::new());
        let mut actuator = Actuator::new_with_kernel(
            PathBuf::from("/state/f3-denials"),
            Box::new(SharedMemory(Arc::clone(&memory))),
        );
        actuator.set_correlation_id("cycle-denials".to_string());

        let missing = actuator
            .apply(&Action::cpu_epp(
                "balance_power".to_string(),
                "F3 missing target".to_string(),
            ))
            .expect("missing target result");
        assert_eq!(missing.domain, "cpu_epp");
        assert_eq!(missing.targets[0].target_id, "cpu:epp");
        assert_eq!(
            missing.targets[0].pipeline_stage,
            PipelineStage::CapabilityGate
        );
        assert_eq!(missing.targets[0].reason, OutcomeReasonCode::MissingTarget);
        assert_eq!(missing.targets[0].support, SupportState::Unsupported);
        assert!(!missing.targets[0].write_attempted);

        let unsupported_action = Action::vm_sysctl(
            PathBuf::from("/tmp/not-a-kernel-target"),
            "60".to_string(),
            "F3 unsupported target".to_string(),
        );
        let unsupported = actuator
            .apply(&unsupported_action)
            .expect("unsupported target result");
        assert_gate(
            &unsupported,
            GateStage::CapabilityValidation,
            GateDisposition::Denied,
            GateReasonCode::CapabilityDenied,
        );
        assert_eq!(
            unsupported.targets[0].reason,
            OutcomeReasonCode::UnsupportedTarget
        );
        assert_eq!(
            unsupported.targets[0].target_id,
            "vm-sysctl:not-a-kernel-target"
        );
        assert!(!unsupported.targets[0].write_attempted);

        let contract_action = Action::DeviceResumeLatency {
            path: PathBuf::from("/sys/bus/pci/devices/0000:00:01.0/power/pm_qos_resume_latency_us"),
            value: Some(1000),
            reason: "F3 contract denial".to_string(),
        };
        let contract = actuator
            .apply(&contract_action)
            .expect("contract denial result");
        assert_gate(
            &contract,
            GateStage::Contract,
            GateDisposition::Denied,
            GateReasonCode::ContractMissing,
        );
        assert_eq!(
            contract.targets[0].pipeline_stage,
            PipelineStage::ContractGate
        );
        assert_eq!(contract.targets[0].reason, OutcomeReasonCode::GateDenied);
        assert!(!contract.targets[0].write_attempted);

        actuator.enable_allowlist(Allowlist::seeded());
        let allowlist_action = Action::PcieAspm {
            device_dir: PathBuf::from("/sys/bus/pci/devices/0000:00:01.0"),
            enable: true,
            reason: "F3 unresolved HWID".to_string(),
        };
        let allowlist = actuator
            .apply(&allowlist_action)
            .expect("allowlist denial result");
        assert_gate(
            &allowlist,
            GateStage::HardwareAllowlist,
            GateDisposition::Denied,
            GateReasonCode::HwidUnresolved,
        );
        assert_eq!(
            allowlist.targets[0].pipeline_stage,
            PipelineStage::AllowlistGate
        );
        assert_eq!(allowlist.targets[0].reason, OutcomeReasonCode::GateDenied);
        assert!(!allowlist.targets[0].write_attempted);
    }

    #[test]
    fn f3_actuator_matrix_reports_success_failure_readback_drift_and_redundancy() {
        let path = PathBuf::from("/proc/sys/vm/swappiness");

        let success_memory = Arc::new(MemoryKernel::new());
        success_memory.write_raw(&path, "100");
        let mut success_actuator = Actuator::new_with_kernel(
            PathBuf::from("/state/f3-success"),
            Box::new(SharedMemory(Arc::clone(&success_memory))),
        );
        success_actuator.set_correlation_id("cycle-success".to_string());
        let success = success_actuator
            .apply(&vm_action("60"))
            .expect("successful fake write");
        assert_eq!(success.targets[0].target_id, "vm-sysctl:swappiness");
        assert_eq!(success.targets[0].pipeline_stage, PipelineStage::Readback);
        assert!(success.targets[0].write_attempted);
        assert_eq!(success.targets[0].write_outcome, WriteOutcome::Applied);
        assert_eq!(
            success.targets[0].readback,
            ReadbackOutcome::Confirmed {
                value: "60".to_string()
            }
        );
        assert_eq!(success.targets[0].ownership, OwnershipState::Optid);

        let failed_memory = MemoryKernel::new();
        failed_memory.write_raw(&path, "100");
        let failed_kernel = FaultKernel::new(Box::new(failed_memory));
        failed_kernel.fail_next_write(path.clone(), io::ErrorKind::PermissionDenied);
        let mut failed_actuator =
            Actuator::new_with_kernel(PathBuf::from("/state/f3-failure"), Box::new(failed_kernel));
        failed_actuator.set_correlation_id("cycle-failure".to_string());
        let failed = failed_actuator
            .apply(&vm_action("60"))
            .expect("typed failed write");
        assert_eq!(failed.targets[0].pipeline_stage, PipelineStage::Write);
        assert_eq!(failed.targets[0].reason, OutcomeReasonCode::WriteFailed);
        assert!(failed.targets[0].write_attempted);
        assert_eq!(
            failed.targets[0].write_outcome,
            WriteOutcome::Failed {
                error_kind: ErrorKindCode::PermissionDenied
            }
        );
        assert_eq!(failed.targets[0].readback, ReadbackOutcome::NotPerformed);

        let drift_memory = MemoryKernel::new();
        drift_memory.write_raw(&path, "100");
        let drift_kernel = FaultKernel::new(Box::new(drift_memory));
        drift_kernel.malform_content(path.clone(), "61\n".to_string());
        let mut drift_actuator =
            Actuator::new_with_kernel(PathBuf::from("/state/f3-drift"), Box::new(drift_kernel));
        drift_actuator.set_correlation_id("cycle-drift".to_string());
        let drift = drift_actuator
            .apply(&vm_action("60"))
            .expect("typed readback drift");
        assert!(drift.targets[0].write_attempted);
        assert_eq!(drift.targets[0].reason, OutcomeReasonCode::ReadbackMismatch);
        assert_eq!(drift.targets[0].ownership, OwnershipState::Drifted);
        assert_eq!(
            drift.targets[0].readback,
            ReadbackOutcome::Mismatch {
                expected: "60".to_string(),
                actual: "61".to_string()
            }
        );

        let epp_memory = Arc::new(MemoryKernel::new());
        let cpu_dir = PathBuf::from("/sys/devices/system/cpu/cpu0");
        let epp_path = cpu_dir.join("cpufreq/energy_performance_preference");
        epp_memory.add_dir_entry(Path::new("/sys/devices/system/cpu"), &cpu_dir);
        epp_memory.write_raw(&epp_path, "balance_power");
        let mut epp_actuator = Actuator::new_with_kernel(
            PathBuf::from("/state/f3-redundant"),
            Box::new(SharedMemory(Arc::clone(&epp_memory))),
        );
        let redundant = epp_actuator
            .apply(&Action::cpu_epp(
                "balance_power".to_string(),
                "F3 redundant write".to_string(),
            ))
            .expect("redundant outcome");
        assert_eq!(redundant.targets.len(), 1);
        assert_eq!(
            redundant.targets[0].reason,
            OutcomeReasonCode::RedundantValue
        );
        assert_eq!(redundant.targets[0].write_outcome, WriteOutcome::Redundant);
        assert!(!redundant.targets[0].write_attempted);
    }

    #[test]
    fn f3_restore_matrix_reports_actual_success_failure_malformed_and_not_applicable() {
        let device = PathBuf::from("/sys/bus/pci/devices/0000:00:01.0");
        let target = device.join("link/l1_aspm");
        let key = format!("aspm_{}", get_path_hash(&device));

        let success_memory = Arc::new(MemoryKernel::new());
        let state_dir = PathBuf::from("/state/f3-restore-success");
        success_memory.write_raw(&target, "1");
        success_memory.write_raw(
            &state_dir.join(format!("original_{key}")),
            &format!("{}\n0", device.display()),
        );
        let mut success_actuator = Actuator::new_with_kernel(
            state_dir.clone(),
            Box::new(SharedMemory(Arc::clone(&success_memory))),
        );
        success_actuator.set_correlation_id("cycle-restore-success".to_string());
        let restored = success_actuator
            .revert_key_outcome(&key)
            .expect("successful stale-key restore");
        assert_eq!(restored.target_id, format!("journal:{key}"));
        assert_eq!(restored.pipeline_stage, PipelineStage::Restore);
        assert_eq!(restored.reason, OutcomeReasonCode::RestoreApplied);
        assert!(restored.write_attempted);
        assert_eq!(restored.write_outcome, WriteOutcome::Restored);
        assert_eq!(restored.pending_restore, RestoreState::Restored);
        assert_eq!(
            success_memory
                .read_to_string(&target)
                .expect("restored value"),
            "0"
        );
        assert!(!success_memory.exists(&state_dir.join(format!("original_{key}"))));

        let failure_memory = MemoryKernel::new();
        let failure_state = PathBuf::from("/state/f3-restore-failure");
        failure_memory.write_raw(&target, "1");
        failure_memory.write_raw(
            &failure_state.join(format!("original_{key}")),
            &format!("{}\n0", device.display()),
        );
        let failure_kernel = FaultKernel::new(Box::new(failure_memory));
        failure_kernel.fail_next_write(target.clone(), io::ErrorKind::PermissionDenied);
        let mut failure_actuator =
            Actuator::new_with_kernel(failure_state, Box::new(failure_kernel));
        failure_actuator.set_correlation_id("cycle-restore-failure".to_string());
        let failed = failure_actuator
            .revert_key_outcome(&key)
            .expect("typed stale-key restore failure");
        assert_eq!(failed.reason, OutcomeReasonCode::RestoreFailed);
        assert!(failed.write_attempted);
        assert_eq!(
            failed.write_outcome,
            WriteOutcome::RestorationFailed {
                error_kind: ErrorKindCode::PermissionDenied
            }
        );
        assert_eq!(failed.pending_restore, RestoreState::Pending);
        assert_eq!(failed.ownership, OwnershipState::Optid);

        let malformed_memory = MemoryKernel::new();
        let malformed_state = PathBuf::from("/state/f3-restore-malformed");
        malformed_memory.write_raw(
            &malformed_state.join(format!("original_{key}")),
            "missing-original-value-line",
        );
        let mut malformed_actuator =
            Actuator::new_with_kernel(malformed_state, Box::new(malformed_memory));
        let malformed = malformed_actuator
            .revert_key_outcome(&key)
            .expect("typed malformed journal result");
        assert_eq!(malformed.reason, OutcomeReasonCode::RestoreFailed);
        assert!(!malformed.write_attempted);
        assert_eq!(
            malformed.write_outcome,
            WriteOutcome::RestorationFailed {
                error_kind: ErrorKindCode::InvalidData
            }
        );

        let mut no_journal = Actuator::new_with_kernel(
            PathBuf::from("/state/f3-no-restore"),
            Box::new(MemoryKernel::new()),
        );
        let not_applicable = no_journal
            .revert_key_outcome("vm_swappiness")
            .expect("non-restorable current inverse path");
        assert_eq!(not_applicable.reason, OutcomeReasonCode::NotApplicable);
        assert!(!not_applicable.write_attempted);
        assert_eq!(not_applicable.write_outcome, WriteOutcome::NotApplicable);
        assert_eq!(not_applicable.ownership, OwnershipState::Relinquished);
    }

    #[test]
    fn f3_observation_matrix_distinguishes_malformed_and_permission_denied_reads() {
        let malformed_path = PathBuf::from("/proc/pressure/cpu");
        let malformed_kernel = FaultKernel::new(Box::new(MemoryKernel::new()));
        malformed_kernel.malform_content(malformed_path.clone(), "not valid PSI".to_string());
        let malformed_snapshot = Snapshot::collect_with(&malformed_kernel, &malformed_kernel);
        let malformed = ObservationEnvelope::from_snapshot(&malformed_snapshot);
        let cpu = malformed
            .values
            .iter()
            .find(|value| value.component_id == "cpu-pressure")
            .expect("CPU PSI observation");
        assert_eq!(cpu.pipeline_stage, PipelineStage::Observation);
        assert_eq!(cpu.reason, ObservationReasonCode::Malformed);
        assert_eq!(cpu.support, SupportState::Unknown);
        assert!(cpu.value.is_null());

        let denied_kernel = FaultKernel::new(Box::new(MemoryKernel::new()));
        denied_kernel.fail_next_read(malformed_path, io::ErrorKind::PermissionDenied);
        let denied_snapshot = Snapshot::collect_with(&denied_kernel, &denied_kernel);
        let denied = ObservationEnvelope::from_snapshot(&denied_snapshot);
        let cpu = denied
            .values
            .iter()
            .find(|value| value.component_id == "cpu-pressure")
            .expect("CPU PSI observation");
        assert_eq!(cpu.pipeline_stage, PipelineStage::Observation);
        assert_eq!(cpu.reason, ObservationReasonCode::PermissionDenied);
        assert_eq!(cpu.support, SupportState::Unknown);
        assert!(cpu.value.is_null());
    }
}
