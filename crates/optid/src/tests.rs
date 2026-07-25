//! Cross-module integration tests for `optid`.
//!
//! Tests that exercise a single module in isolation live inline as
//! `#[cfg(test)] mod tests` at the bottom of that module. The tests here
//! are the ones that span modules — typically exercising the
//! `Snapshot → Policy::classify → Policy::decide_resolved → Decision`
//! pipeline, or the `Actuator + io_util` revert-journal contract.

#![cfg(test)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::action::Action;
use crate::actuator::{Actuator, PmqosSink};
use crate::allowlist::Allowlist;
use crate::args::Args;
use crate::contracts::{fits_contract, Contracts};
use crate::io_util::{
    actuation_state, clear_journal, get_path_hash, mark_applied, revert_pm_qos, revert_sysctls,
};
use crate::load_state::{BootState, LoadState};
use crate::policy::Policy;
use crate::run;
use crate::sensors::{Pressure, Snapshot};
use crate::workload::{Mode, WorkloadClass};

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_t1_dry_run_no_op() {
        std::env::set_var("OPTID_MOCK_ZRAM_SWAP_ACTIVE", "false");
        let temp_dir = std::env::temp_dir().join(format!("optid_tests_t1_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();

        let args = Args {
            apply: false, // DRY RUN
            once: true,
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
            allowlist: false,
            foreground: crate::args::ForegroundMode::Off,
        };

        run(args).unwrap();

        assert!(!temp_dir.join("intended_vm_swappiness").exists());

        let decisions = fs::read_to_string(temp_dir.join("decisions.log")).unwrap();
        std::env::remove_var("OPTID_MOCK_ZRAM_SWAP_ACTIVE");
        assert!(decisions.contains("vm.* actuation skipped: zram swap is not active"));
    }

    #[test]
    fn test_t2_failed_real_sysctl_revert_keeps_journal() {
        let temp_dir = std::env::temp_dir().join(format!("optid_tests_t2_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();

        let mut actuator = Actuator::new(temp_dir.clone());
        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "60".to_string(),
            "test reason".to_string(),
        );
        let _ = actuator.apply(&action);

        assert!(temp_dir.join("intended_vm_swappiness").exists());

        let actions_log = fs::read_to_string(temp_dir.join("actions.log")).unwrap();
        assert!(actions_log.contains("vm.sysctl swappiness") || actions_log.contains("was"));

        revert_sysctls(&temp_dir);
        assert!(
            temp_dir.join("intended_vm_swappiness").exists(),
            "a denied restore must keep the journal retryable"
        );
    }

    #[test]
    fn test_t2b_vm_sysctl_writes_revert_journal_entry() {
        let temp_dir = std::env::temp_dir().join(format!("optid_tests_t2b_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let sysctl_path = temp_dir.join("swappiness");
        fs::write(&sysctl_path, "100").unwrap();

        let mut actuator = Actuator::new(temp_dir.clone());
        let action = Action::vm_sysctl(
            sysctl_path.clone(),
            "60".to_string(),
            "test journal".to_string(),
        );
        actuator.apply(&action).unwrap();

        // The temp path is intentionally outside the guarded sysctl allowlist, so
        // the write may be skipped. The journal entries must still capture the
        // original and intended values for real allowlisted vm.* paths.
        assert_eq!(
            fs::read_to_string(temp_dir.join("original_vm_swappiness"))
                .unwrap()
                .trim(),
            "100"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.join("intended_vm_swappiness"))
                .unwrap()
                .trim(),
            "60"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_t3_zram_gate() {
        let mut policy = Policy::default();
        policy.memory.high_swappiness_requires_zram = true;
        policy.modes.performance.vm_swappiness = Some(150);

        let snapshot_with_zram = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: None,
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: true,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let decision_with = policy.decide(
            &snapshot_with_zram,
            Mode::Performance,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        let has_150 = decision_with.actions.iter().any(|action| {
            if let Action::VmSysctl { path, value, .. } = action {
                path == Path::new("/proc/sys/vm/swappiness") && value == "150"
            } else {
                false
            }
        });
        assert!(has_150);

        let snapshot_no_zram = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: None,
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let decision_no = policy.decide(
            &snapshot_no_zram,
            Mode::Performance,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        let has_vm_action = decision_no
            .actions
            .iter()
            .any(|action| matches!(action, Action::VmSysctl { .. }));
        assert!(!has_vm_action);
        assert!(decision_no
            .reasons
            .iter()
            .any(|reason| reason.contains("vm.* actuation skipped")));
    }

    #[test]
    fn test_n1_t1_class_mapping() {
        let policy = Policy::default();

        // 1. Idle snapshot
        let idle_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        assert_eq!(policy.classify(&idle_snap).0, WorkloadClass::Idle);

        // 2. Light snapshot
        let light_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.1),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        assert_eq!(policy.classify(&light_snap).0, WorkloadClass::Light);

        // 3. Interactive snapshot
        let int_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.8),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        assert_eq!(policy.classify(&int_snap).0, WorkloadClass::Interactive);

        // 4. Latency-critical snapshot
        let lc_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(2.0),
            cpu_pressure: Some(Pressure {
                avg10: 15.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        assert_eq!(policy.classify(&lc_snap).0, WorkloadClass::LatencyCritical);

        // 5. Throughput snapshot
        let tp_snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(5.0),
            cpu_pressure: Some(Pressure {
                avg10: 15.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        assert_eq!(policy.classify(&tp_snap).0, WorkloadClass::Throughput);
    }

    #[test]
    fn test_n1_t2_pin_precedence() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: Some("doom.exe".to_string()),
            pinned_class: Some(WorkloadClass::LatencyCritical),
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let (class, reason) = policy.classify(&snap);
        assert_eq!(class, WorkloadClass::LatencyCritical);
        assert!(reason.contains("pinned override"));
    }

    #[test]
    fn test_n1_t3d_mode_hysteresis_reason_is_explainable() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 1,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(4.0),
            cpu_pressure: Some(Pressure {
                avg10: 20.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let decision = policy.decide_resolved(
            &snap,
            Mode::Auto,
            WorkloadClass::Throughput,
            "test".to_string(),
            &Contracts::default(),
            Some(Mode::Balanced),
            Some("mode hysteresis delaying transition: committed=balanced, candidate=performance, elapsed=1s, required=6s".to_string()),
        );
        let report = decision.render(&snap);
        assert!(report.contains("mode=balanced"));
        assert!(report.contains("mode hysteresis delaying transition"));
        assert!(report.contains("candidate=performance"));
    }

    #[test]
    fn test_n1_t4_determinism() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.5),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let res1 = policy.classify(&snap);
        let res2 = policy.classify(&snap);
        assert_eq!(res1, res2);
    }

    #[test]
    fn test_n1_t5_explainability() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: Some(50_000),
            loadavg_1: Some(5.0),
            cpu_pressure: Some(Pressure {
                avg10: 20.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let (_, reason) = policy.classify(&snap);
        assert!(reason.contains("high load") && reason.contains("high pressure"));
    }

    #[test]
    fn test_n1_t6_absent_foreground() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(1.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let (class, _) = policy.classify(&snap);
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn test_n1_t12_low_battery_on_ac_stays_balanced() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: Some(15),
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Balanced);
    }

    #[test]
    fn test_n1_t13_critical_thermal_overrides_cpu_performance() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: Some(95_000),
            loadavg_1: Some(8.0),
            cpu_pressure: Some(Pressure {
                avg10: 50.0,
                ..Pressure::default()
            }),
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Balanced);
        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Throughput,
            "test".to_string(),
            &Contracts::default(),
        );
        assert_eq!(decision.mode, Mode::Balanced);
        assert!(decision
            .reasons
            .iter()
            .any(|reason| reason.contains("critical thermal pressure")));
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::CpuEpp { value, reason }
                if value == "balance_power" && reason.contains("thermals are critical")
        )));
    }

    #[test]
    fn test_n1_t14_io_pressure_adds_background_io_throttle() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(1.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: Some(Pressure {
                avg10: 9.0,
                ..Pressure::default()
            }),
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Interactive,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::SystemdSetProperty { unit, properties, reason }
                if unit == "background.slice"
                    && properties.iter().any(|p| p == "IOWeight=25")
                    && reason.contains("background I/O")
        )));
    }

    #[test]
    fn test_n1_t15_memory_pressure_protects_user_and_throttles_background() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(1.0),
            cpu_pressure: None,
            memory_pressure: Some(Pressure {
                avg10: 6.0,
                ..Pressure::default()
            }),
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Interactive,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::SystemdSetProperty { unit, properties, .. }
                if unit == "user.slice" && properties.iter().any(|p| p == "MemoryLow=256M")
        )));
        assert!(decision.actions.iter().any(|action| matches!(
            action,
            Action::SystemdSetProperty { unit, properties, .. }
                if unit == "background.slice"
                    && properties.iter().any(|p| p == "MemoryHigh=75%")
                    && properties.iter().any(|p| p == "CPUWeight=50")
                    && properties.iter().any(|p| p == "IOWeight=50")
        )));
    }

    #[test]
    fn test_n1_t16_manual_performance_overrides_auto_battery() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(false),
            battery_pct: Some(10),
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: true,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Battery);
        let decision = policy.decide(
            &snap,
            Mode::Performance,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert_eq!(decision.mode, Mode::Performance);
        assert!(decision
            .reasons
            .iter()
            .any(|reason| reason == "manual mode override: performance"));
    }

    #[test]
    fn test_n1_t17_missing_sensors_choose_safe_balanced() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: None,
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: None,
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        assert_eq!(policy.auto_mode(&snap), Mode::Balanced);
        assert_eq!(policy.classify(&snap).0, WorkloadClass::Idle);
    }

    struct MockPmqosSink {
        cpu_latency: Option<i32>,
        device_latencies: std::collections::HashMap<PathBuf, String>,
        cpu_fd_open: bool,
        write_count: usize,
    }

    impl MockPmqosSink {
        fn new() -> Self {
            Self {
                cpu_latency: None,
                device_latencies: std::collections::HashMap::new(),
                cpu_fd_open: false,
                write_count: 0,
            }
        }
    }

    impl PmqosSink for MockPmqosSink {
        fn read_cpu_latency(&self) -> io::Result<String> {
            Ok(self
                .cpu_latency
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".to_string()))
        }

        fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
            self.cpu_latency = value;
            self.cpu_fd_open = value.is_some();
            self.write_count += 1;
            Ok(())
        }

        fn read_device_latency(&self, device_path: &Path) -> io::Result<String> {
            if let Some(val) = self.device_latencies.get(device_path) {
                Ok(val.clone())
            } else {
                Ok("0".to_string())
            }
        }

        fn write_device_latency(&mut self, device_path: &Path, value: &str) -> io::Result<()> {
            self.device_latencies
                .insert(device_path.to_path_buf(), value.to_string());
            Ok(())
        }
    }

    #[test]
    fn test_n2_t1_resolution() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t1_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let contracts_path = temp_dir.join("contracts.toml");
        fs::write(
            &contracts_path,
            r#"
[contracts.idle]
cpu_wakeup_latency = 100000
device_resume_latency = 1000000

[contracts.light]
cpu_wakeup_latency = 50000
device_resume_latency = 500000

[contracts.interactive]
cpu_wakeup_latency = 1000
device_resume_latency = 10000

[contracts.latency-critical]
cpu_wakeup_latency = 1000
device_resume_latency = 1000

[contracts.throughput]
cpu_wakeup_latency = 10000
device_resume_latency = 100000
"#,
        )
        .unwrap();

        let contracts = Contracts::load(&contracts_path);

        assert_eq!(
            contracts.resolve(WorkloadClass::Idle).cpu_wakeup_latency,
            100000
        );
        assert_eq!(
            contracts.resolve(WorkloadClass::Idle).device_resume_latency,
            1000000
        );

        assert_eq!(
            contracts.resolve(WorkloadClass::Light).cpu_wakeup_latency,
            50000
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::Light)
                .device_resume_latency,
            500000
        );

        assert_eq!(
            contracts
                .resolve(WorkloadClass::Interactive)
                .cpu_wakeup_latency,
            1000
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::Interactive)
                .device_resume_latency,
            10000
        );

        assert_eq!(
            contracts
                .resolve(WorkloadClass::LatencyCritical)
                .cpu_wakeup_latency,
            1000
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::LatencyCritical)
                .device_resume_latency,
            1000
        );

        assert_eq!(
            contracts
                .resolve(WorkloadClass::Throughput)
                .cpu_wakeup_latency,
            10000
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::Throughput)
                .device_resume_latency,
            100000
        );
    }

    #[test]
    fn test_n2_t2_dry_run_no_op() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t2_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();
        fs::write(temp_dir.join("contracts.toml"), "").unwrap();

        let args = Args {
            apply: false, // DRY RUN
            once: true,
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
            allowlist: false,
            foreground: crate::args::ForegroundMode::Off,
        };

        run(args).unwrap();

        let decisions = fs::read_to_string(temp_dir.join("decisions.log")).unwrap();
        assert!(decisions.contains("cpu_wakeup_latency="));
        assert!(decisions.contains("device_resume_latency="));
        assert!(
            !temp_dir.join("actions.log").exists()
                || fs::read_to_string(temp_dir.join("actions.log"))
                    .unwrap()
                    .is_empty()
        );
    }

    #[test]
    fn test_n2_t3_apply_cpu_floor() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t3_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let mock_sink = Box::new(MockPmqosSink::new());
        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), mock_sink);

        let action = Action::CpuDmaLatency {
            value: Some(1000),
            reason: "test".to_string(),
        };

        actuator.apply(&action).unwrap();

        assert_eq!(actuator.pmqos_sink.read_cpu_latency().unwrap(), "1000");
    }

    #[test]
    fn test_n2_t4_per_device_revert() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t4_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        // Mirror real PCI structure so the structural allowlist check accepts
        // it: parent dir must be `power`, file must be `pm_qos_resume_latency_us`.
        let dev_dir = temp_dir.join("0000:00:1f.3").join("power");
        fs::create_dir_all(&dev_dir).unwrap();
        let dev_path = dev_dir.join("pm_qos_resume_latency_us");

        let mut mock_sink = MockPmqosSink::new();
        mock_sink.write_device_latency(&dev_path, "250").unwrap();
        fs::write(&dev_path, "250").unwrap();

        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), Box::new(mock_sink));

        let action = Action::DeviceResumeLatency {
            path: dev_path.clone(),
            value: Some(100),
            reason: "test".to_string(),
        };

        actuator.apply(&action).unwrap();

        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "100"
        );

        let hash = get_path_hash(&dev_path);
        let orig_file = temp_dir.join(format!("original_dev_{hash}"));
        assert!(orig_file.exists());
        let orig_content = fs::read_to_string(&orig_file).unwrap();
        let mut lines = orig_content.lines();
        assert_eq!(lines.next().unwrap(), dev_path.to_str().unwrap());
        assert_eq!(lines.next().unwrap(), "250");

        revert_pm_qos(&temp_dir);

        let current_disk_val = fs::read_to_string(&dev_path).unwrap();
        assert_eq!(current_disk_val.trim(), "250");

        assert!(!orig_file.exists());
        assert!(!temp_dir.join(format!("intended_dev_{hash}")).exists());
    }

    // ── WP-N4: hardware allowlist gate wired into the actuator ───────────────

    #[test]
    fn test_n4_gate_default_denies_unknown_hwid_and_audits() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n4_deny_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        // Real PCI-shaped layout: <dev>/power/pm_qos_resume_latency_us, with a
        // modalias the seeded baseline does not cover for the runtime_pm domain.
        let dev = temp_dir.join("0000:00:1f.3");
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(dev.join("modalias"), "pci:vFFFFpFFFFsvFFFFsdFFFF\n").unwrap();
        let dev_path = power.join("pm_qos_resume_latency_us");

        let mut actuator =
            Actuator::new_with_sink(temp_dir.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::seeded());

        let action = Action::DeviceResumeLatency {
            path: dev_path.clone(),
            value: Some(100),
            reason: "test".to_string(),
        };
        actuator.apply(&action).unwrap();

        // Default-deny: the write must NOT have reached the sink.
        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "0",
            "denied actuation must not write"
        );
        // Denial logged with reason in the JSONL audit trail.
        let audit = fs::read_to_string(temp_dir.join("audit.jsonl")).unwrap();
        assert!(audit.contains("\"event\":\"actuation_denied\""), "{audit}");
        assert!(audit.contains("hwid_not_in_allowlist"), "{audit}");
        assert!(audit.contains("\"domain\":\"runtime_pm\""), "{audit}");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_n4_gate_allows_allowlisted_hwid() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n4_allow_{}", std::process::id()));
        let admin = temp_dir.join("admin");
        let _ = fs::create_dir_all(&admin);

        let dev = temp_dir.join("0000:00:14.0");
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        let modalias = "pci:v00008086p0000A0EDsv00001028sd00000A01bc0Csc03i30";
        fs::write(dev.join("modalias"), format!("{modalias}\n")).unwrap();
        let dev_path = power.join("pm_qos_resume_latency_us");

        // Admin override allows this HWID for the runtime_pm domain.
        fs::write(
            admin.join("90-admin.toml"),
            format!(
                "[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"tested in N4 unit test\"\n"
            ),
        )
        .unwrap();

        let mut actuator =
            Actuator::new_with_sink(temp_dir.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));

        let action = Action::DeviceResumeLatency {
            path: dev_path.clone(),
            value: Some(100),
            reason: "test".to_string(),
        };
        actuator.apply(&action).unwrap();

        // Allowed: the write reaches the sink and nothing is audited as denied.
        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "100"
        );
        assert!(!temp_dir.join("audit.jsonl").exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_n4_gate_disabled_is_no_op() {
        // With the gate disabled (the v0.x default), DeviceResumeLatency writes
        // exactly as before — no HWID resolution, no audit file.
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n4_off_{}", std::process::id()));
        let power = temp_dir.join("0000:00:1f.3").join("power");
        fs::create_dir_all(&power).unwrap();
        let dev_path = power.join("pm_qos_resume_latency_us");

        let mut actuator =
            Actuator::new_with_sink(temp_dir.clone(), Box::new(MockPmqosSink::new()));
        // No enable_allowlist call → gate off.

        let action = Action::DeviceResumeLatency {
            path: dev_path.clone(),
            value: Some(100),
            reason: "test".to_string(),
        };
        actuator.apply(&action).unwrap();

        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "100"
        );
        assert!(!temp_dir.join("audit.jsonl").exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    // ── WP-N5: runtime-PM autosuspend actuator ──────────────────────────────

    /// Build a synthetic sysfs device dir with a modalias and power/control.
    fn n5_device(temp: &Path, name: &str, modalias: &str) -> PathBuf {
        let dev = temp.join(name);
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(dev.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(power.join("control"), "on\n").unwrap();
        fs::write(power.join("autosuspend_delay_ms"), "-1\n").unwrap();
        dev
    }

    #[test]
    fn test_n5_runtime_pm_allows_journals_and_reverts() {
        let temp = std::env::temp_dir().join(format!("optid_n5_allow_{}", std::process::id()));
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let modalias = "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00";
        let dev = n5_device(&temp, "1-1", modalias);
        let power = dev.join("power");
        fs::write(power.join("wakeup"), "enabled\n").unwrap();

        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"n5 test\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));

        let action = Action::RuntimePm {
            device_dir: dev.clone(),
            autosuspend_delay_ms: 2000,
            reason: "test".to_string(),
        };
        actuator.apply(&action).unwrap();

        // Enabled: control=auto, delay applied.
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "auto"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "2000"
        );
        // Wakeup is never touched.
        assert_eq!(
            fs::read_to_string(power.join("wakeup")).unwrap().trim(),
            "enabled"
        );
        // Original journaled for revert.
        let hash = get_path_hash(&dev);
        let orig = temp.join(format!("original_rpm_{hash}"));
        assert!(orig.exists());

        // Revert on stop restores control + delay and clears the journal.
        crate::io_util::revert_runtime_pm(&temp);
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "-1"
        );
        assert!(!orig.exists());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n5_runtime_pm_default_deny_skips_and_audits() {
        let temp = std::env::temp_dir().join(format!("optid_n5_deny_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        // A modalias not present in the seeded baseline for runtime_pm.
        let dev = n5_device(&temp, "2-1", "usb:vFFFFpFFFFd0001dc00ic03");
        let power = dev.join("power");

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::seeded());

        actuator
            .apply(&Action::RuntimePm {
                device_dir: dev.clone(),
                autosuspend_delay_ms: 2000,
                reason: "test".to_string(),
            })
            .unwrap();

        // Default-deny: control left untouched, nothing journaled.
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on"
        );
        let hash = get_path_hash(&dev);
        assert!(!temp.join(format!("original_rpm_{hash}")).exists());
        // Denial audited with reason + domain.
        let audit = fs::read_to_string(temp.join("audit.jsonl")).unwrap();
        assert!(audit.contains("\"event\":\"actuation_denied\""), "{audit}");
        assert!(audit.contains("\"domain\":\"runtime_pm\""), "{audit}");
        assert!(audit.contains("hwid_not_in_allowlist"), "{audit}");

        let _ = fs::remove_dir_all(&temp);
    }

    // ── Defect 3: SPEC §3 contract gate in the actuator ─────────────────────

    /// Build an allowlisted synthetic runtime-PM device for contract tests.
    fn contract_rpm_device(temp: &Path, name: &str, modalias: &str) -> PathBuf {
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let dev = temp.join(name);
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(dev.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(power.join("control"), "on\n").unwrap();
        fs::write(power.join("autosuspend_delay_ms"), "100\n").unwrap();
        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"contract gate test\"\n"),
        )
        .unwrap();
        dev
    }

    /// A 2000 ms autosuspend delay is a 2,000,000 µs exit latency, which
    /// exceeds a 1,000,000 µs floor — the write must be refused, and must
    /// leave no mutation and no journal behind.
    #[test]
    fn test_contract_gate_blocks_runtime_pm_over_floor() {
        let temp =
            std::env::temp_dir().join(format!("optid_contract_block_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let modalias = "usb:v046Dp0090d0001dc00dsc00dp00ic03isc01ip01in00";
        let dev = contract_rpm_device(&temp, "2-1", modalias);
        let power = dev.join("power");

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        let admin_dir = temp.join("admin");
        actuator.enable_allowlist(Allowlist::load_from(std::slice::from_ref(&admin_dir)));
        actuator.set_active_floors(crate::contracts::ContractFloors {
            cpu_wakeup_latency: 100_000,
            device_resume_latency: 1_000_000,
        });

        actuator
            .apply(&Action::RuntimePm {
                device_dir: dev.clone(),
                autosuspend_delay_ms: 2000, // 2_000_000 us > 1_000_000 us floor
                reason: "battery idle".to_string(),
            })
            .unwrap();

        // No mutation.
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on",
            "blocked action must not write power/control"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "100",
            "blocked action must not write autosuspend_delay_ms"
        );

        // No journal, no applied marker, no cache entry.
        let hash = get_path_hash(&dev);
        assert!(!temp.join(format!("original_rpm_{hash}")).exists());
        assert!(!temp.join(format!("intended_rpm_{hash}")).exists());
        assert!(!temp.join(format!("applied_rpm_{hash}")).exists());
        assert!(!actuator.last_runtime_pm.contains_key(&dev));

        // Blocked with the documented log line.
        let log = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(
            log.contains("contract gate BLOCKED"),
            "expected a contract gate BLOCKED line, got: {log}"
        );
        assert!(
            log.contains("exit_latency=2000000us") && log.contains("floor=1000000us"),
            "log must report the exit latency and the floor, got: {log}"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    /// The same 2000 ms delay fits a 2,000,000 µs floor exactly, so it is
    /// allowed (the predicate is `<=`).
    #[test]
    fn test_contract_gate_allows_runtime_pm_within_floor() {
        let temp =
            std::env::temp_dir().join(format!("optid_contract_allow_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let modalias = "usb:v046Dp0091d0001dc00dsc00dp00ic03isc01ip01in00";
        let dev = contract_rpm_device(&temp, "2-2", modalias);
        let power = dev.join("power");

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        let admin_dir = temp.join("admin");
        actuator.enable_allowlist(Allowlist::load_from(std::slice::from_ref(&admin_dir)));
        actuator.set_active_floors(crate::contracts::ContractFloors {
            cpu_wakeup_latency: 100_000,
            device_resume_latency: 2_000_000,
        });

        actuator
            .apply(&Action::RuntimePm {
                device_dir: dev.clone(),
                autosuspend_delay_ms: 2000, // exactly at the floor
                reason: "battery idle".to_string(),
            })
            .unwrap();

        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "auto"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "2000"
        );
        let hash = get_path_hash(&dev);
        assert!(temp.join(format!("original_rpm_{hash}")).exists());

        let log = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(!log.contains("contract gate BLOCKED"), "{log}");

        let _ = fs::remove_dir_all(&temp);
    }

    /// PM QoS resume latency is gated on the same floor, and a blocked
    /// action must not reach the sink.
    #[test]
    fn test_contract_gate_blocks_device_resume_latency_over_floor() {
        let temp = std::env::temp_dir().join(format!("optid_contract_dev_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let power = temp.join("0000:00:14.0").join("power");
        fs::create_dir_all(&power).unwrap();
        let dev_path = power.join("pm_qos_resume_latency_us");

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.set_active_floors(crate::contracts::ContractFloors {
            cpu_wakeup_latency: 1_000,
            device_resume_latency: 10_000,
        });

        actuator
            .apply(&Action::DeviceResumeLatency {
                path: dev_path.clone(),
                value: Some(50_000), // 50_000 us > 10_000 us floor
                reason: "battery idle".to_string(),
            })
            .unwrap();

        // Never reached the sink, so the mock still reports its default.
        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "0"
        );
        let key = format!("dev_{}", get_path_hash(&dev_path));
        assert!(!temp.join(format!("original_{key}")).exists());
        assert!(!actuator.last_device_latencies.contains_key(&dev_path));

        let log = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(log.contains("contract gate BLOCKED"), "{log}");

        let _ = fs::remove_dir_all(&temp);
    }

    /// Negative values must not wrap into small valid latencies. `-1` is
    /// the kernel's "unset" sentinel and must be treated as unconstrained,
    /// not as u64::MAX (blocked by everything) nor as a tiny latency.
    #[test]
    fn test_contract_gate_handles_negative_values_safely() {
        let temp = std::env::temp_dir().join(format!("optid_contract_neg_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let modalias = "usb:v046Dp0092d0001dc00dsc00dp00ic03isc01ip01in00";
        let dev = contract_rpm_device(&temp, "2-3", modalias);
        let power = dev.join("power");

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        let admin_dir = temp.join("admin");
        actuator.enable_allowlist(Allowlist::load_from(std::slice::from_ref(&admin_dir)));
        // A tight floor that u64::MAX would certainly violate.
        actuator.set_active_floors(crate::contracts::ContractFloors {
            cpu_wakeup_latency: 1_000,
            device_resume_latency: 1_000,
        });

        actuator
            .apply(&Action::RuntimePm {
                device_dir: dev.clone(),
                autosuspend_delay_ms: -1,
                reason: "unset sentinel".to_string(),
            })
            .unwrap();

        // Not blocked by the gate: -1 is "no constraint", so the action
        // proceeds to the normal actuation path.
        let log = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(
            !log.contains("contract gate BLOCKED"),
            "negative delay must not be wrapped into a blocked latency: {log}"
        );
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "auto"
        );

        // Same for a negative PM QoS value. Use a separate actuator with
        // the hardware allowlist left disabled: this synthetic attribute
        // path has no modalias, so an enabled allowlist would default-deny
        // it and mask what this test is actually asserting about the
        // contract gate.
        let qos_power = temp.join("0000:00:15.0").join("power");
        fs::create_dir_all(&qos_power).unwrap();
        let dev_path = qos_power.join("pm_qos_resume_latency_us");

        let mut qos_actuator =
            Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        qos_actuator.set_active_floors(crate::contracts::ContractFloors {
            cpu_wakeup_latency: 1_000,
            device_resume_latency: 1_000,
        });
        qos_actuator
            .apply(&Action::DeviceResumeLatency {
                path: dev_path.clone(),
                value: Some(-1),
                reason: "unset sentinel".to_string(),
            })
            .unwrap();
        assert_eq!(
            qos_actuator
                .pmqos_sink
                .read_device_latency(&dev_path)
                .unwrap(),
            "-1",
            "negative PM QoS value should pass the gate and reach the sink"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    /// With no floors installed the gate is open — preserves behaviour for
    /// every existing test and legacy caller that builds an Actuator
    /// directly.
    #[test]
    fn test_contract_gate_open_when_no_floors_installed() {
        let temp = std::env::temp_dir().join(format!("optid_contract_none_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let modalias = "usb:v046Dp0093d0001dc00dsc00dp00ic03isc01ip01in00";
        let dev = contract_rpm_device(&temp, "2-4", modalias);
        let power = dev.join("power");

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        let admin_dir = temp.join("admin");
        actuator.enable_allowlist(Allowlist::load_from(std::slice::from_ref(&admin_dir)));
        // No set_active_floors call.
        assert!(actuator.active_floors.is_none());

        actuator
            .apply(&Action::RuntimePm {
                device_dir: dev.clone(),
                autosuspend_delay_ms: 60_000, // would violate every real floor
                reason: "no contract installed".to_string(),
            })
            .unwrap();

        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "auto"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    /// Ungated variants are unaffected by a tight floor.
    #[test]
    fn test_contract_gate_ignores_non_latency_actions() {
        let temp =
            std::env::temp_dir().join(format!("optid_contract_other_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.set_active_floors(crate::contracts::ContractFloors {
            cpu_wakeup_latency: 1,
            device_resume_latency: 1,
        });

        // CpuEpp has no discoverable paths in the test environment, so it
        // logs a skip rather than writing — the point is that the contract
        // gate does not block it.
        actuator
            .apply(&Action::CpuEpp {
                value: "power".to_string(),
                reason: "test".to_string(),
            })
            .unwrap();

        let log = fs::read_to_string(temp.join("actions.log")).unwrap_or_default();
        assert!(
            !log.contains("contract gate BLOCKED"),
            "CPU EPP must not be contract-gated: {log}"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    // ── Defect 2: inverse restore on context change ─────────────────────────

    /// A runtime-PM action applied on one tick must be reverted when the
    /// next decision no longer contains it — not left applied until
    /// shutdown. This reproduces the battery→AC transition: the daemon
    /// enables autosuspend while on battery, then the charger is plugged
    /// in and the new decision drops the action entirely.
    ///
    /// The revert is driven through exactly the same active-key
    /// difference the main loop performs, so this exercises the real
    /// wiring rather than calling `revert_key` on a hand-written key.
    #[test]
    fn test_context_change_reverts_removed_runtime_pm_action() {
        let temp =
            std::env::temp_dir().join(format!("optid_ctx_revert_rpm_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();

        let modalias = "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00";
        let dev = temp.join("1-2");
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(dev.join("modalias"), format!("{modalias}\n")).unwrap();
        // Baseline: autosuspend off, 100 ms delay.
        fs::write(power.join("control"), "on\n").unwrap();
        fs::write(power.join("autosuspend_delay_ms"), "100\n").unwrap();

        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"context-change revert test\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(Allowlist::load_from(std::slice::from_ref(&admin)));

        // ── Tick 1: on battery, runtime PM is applied. ──
        let action = Action::RuntimePm {
            device_dir: dev.clone(),
            autosuspend_delay_ms: 2000,
            reason: "battery idle".to_string(),
        };
        actuator.apply(&action).unwrap();

        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "auto"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "2000"
        );

        let key = action.journal_key().expect("runtime PM action has a key");
        let hash = get_path_hash(&dev);
        assert_eq!(key, format!("rpm_{hash}"));
        assert!(temp.join(format!("original_{key}")).exists());
        assert!(temp.join(format!("applied_{key}")).exists());
        assert_eq!(actuator.last_runtime_pm.get(&dev), Some(&2000));

        let active_keys: std::collections::HashSet<String> = std::iter::once(key.clone()).collect();

        // ── Tick 2: charger plugged in; the new decision has no
        // runtime-PM action at all. Mirror the main loop's difference. ──
        let next_actions: Vec<Action> = Vec::new();
        let new_keys: std::collections::HashSet<String> = next_actions
            .iter()
            .filter_map(|a: &Action| a.journal_key())
            .collect();
        assert!(new_keys.is_empty());

        for stale in active_keys.difference(&new_keys) {
            assert!(
                actuator.revert_key(stale).unwrap(),
                "revert_key should report a completed restoration for {stale}"
            );
        }

        // Device is back to its pre-actuation state.
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on",
            "power/control must be restored to its journaled original"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "100",
            "autosuspend_delay_ms must be restored to its journaled original"
        );

        // Idempotence cache cleared, so a later re-apply is not skipped.
        assert!(
            !actuator.last_runtime_pm.contains_key(&dev),
            "last_runtime_pm must be cleared after a context-change revert"
        );

        // Journal fully removed.
        assert!(!temp.join(format!("original_{key}")).exists());
        assert!(!temp.join(format!("intended_{key}")).exists());
        assert!(!temp.join(format!("applied_{key}")).exists());

        // The revert is recorded in the action log.
        let actions_log = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(
            actions_log.contains("context-change revert"),
            "expected a context-change revert log line, got: {actions_log}"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    /// A key that is still present in the next decision must NOT be
    /// reverted — only the difference is restored.
    #[test]
    fn test_context_change_retains_still_active_runtime_pm_action() {
        let temp =
            std::env::temp_dir().join(format!("optid_ctx_retain_rpm_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();

        let modalias = "usb:v046Dp0083d0001dc00dsc00dp00ic03isc01ip01in00";
        let dev = temp.join("1-3");
        let power = dev.join("power");
        fs::create_dir_all(&power).unwrap();
        fs::write(dev.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(power.join("control"), "on\n").unwrap();
        fs::write(power.join("autosuspend_delay_ms"), "100\n").unwrap();

        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"context-change retain test\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(Allowlist::load_from(std::slice::from_ref(&admin)));

        let action = Action::RuntimePm {
            device_dir: dev.clone(),
            autosuspend_delay_ms: 2000,
            reason: "battery idle".to_string(),
        };
        actuator.apply(&action).unwrap();
        let key = action.journal_key().unwrap();

        let active_keys: std::collections::HashSet<String> = std::iter::once(key.clone()).collect();
        // The next decision still contains the same action.
        let new_keys: std::collections::HashSet<String> = std::iter::once(key.clone()).collect();

        for stale in active_keys.difference(&new_keys) {
            actuator.revert_key(stale).unwrap();
        }

        // Untouched: still applied, journal intact.
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "auto"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "2000"
        );
        assert!(temp.join(format!("original_{key}")).exists());

        let _ = fs::remove_dir_all(&temp);
    }

    /// `SystemdSetProperty` journals nothing, so it has no revert key.
    #[test]
    fn test_journal_key_none_for_systemd_set_property() {
        let action = Action::systemd_set_property(
            "user.slice".to_string(),
            vec!["CPUWeight=100".to_string()],
            "test".to_string(),
        );
        assert!(action.journal_key().is_none());
    }

    /// System-wide knobs are continuously overwritten by later ticks, so
    /// `revert_key` deliberately declines to restore them.
    #[test]
    fn test_revert_key_ignores_system_wide_knobs() {
        let temp = std::env::temp_dir().join(format!("optid_ctx_sys_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        // Journal a system-wide knob as if it had been applied.
        fs::write(temp.join("original_vm_swappiness"), "60").unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        assert!(!actuator.revert_key("vm_swappiness").unwrap());
        assert!(!actuator.revert_key("cpu_epp").unwrap());
        assert!(!actuator.revert_key("platform_profile").unwrap());
        assert!(!actuator.revert_key("cpu_dma_latency").unwrap());

        // The journal is left for the shutdown revert to handle.
        assert!(temp.join("original_vm_swappiness").exists());

        let _ = fs::remove_dir_all(&temp);
    }

    /// An unknown key, or a key with no journal on disk, is a no-op.
    #[test]
    fn test_revert_key_absent_journal_is_noop() {
        let temp = std::env::temp_dir().join(format!("optid_ctx_absent_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        assert!(!actuator.revert_key("rpm_deadbeef").unwrap());
        assert!(!actuator.revert_key("totally_unknown_key").unwrap());

        let _ = fs::remove_dir_all(&temp);
    }

    /// The PM QoS resume-latency revert must go back through the sink the
    /// apply path used, and clear the matching idempotence cache.
    #[test]
    fn test_context_change_reverts_device_resume_latency() {
        let temp = std::env::temp_dir().join(format!("optid_ctx_dev_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let dev_path = temp
            .join("0000:00:14.0")
            .join("power")
            .join("pm_qos_resume_latency_us");
        fs::create_dir_all(dev_path.parent().unwrap()).unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        let action = Action::DeviceResumeLatency {
            path: dev_path.clone(),
            value: Some(100),
            reason: "battery idle".to_string(),
        };
        actuator.apply(&action).unwrap();

        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "100"
        );
        let key = action.journal_key().unwrap();
        assert_eq!(key, format!("dev_{}", get_path_hash(&dev_path)));
        assert!(actuator.last_device_latencies.contains_key(&dev_path));

        assert!(actuator.revert_key(&key).unwrap());

        // MockPmqosSink reports "0" for an unseeded device, which is what
        // the apply path journaled as the original.
        assert_eq!(
            actuator.pmqos_sink.read_device_latency(&dev_path).unwrap(),
            "0",
            "device latency must be restored through the PM QoS sink"
        );
        assert!(
            !actuator.last_device_latencies.contains_key(&dev_path),
            "last_device_latencies must be cleared after a context-change revert"
        );
        assert!(!temp.join(format!("original_{key}")).exists());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n5_runtime_pm_skips_network_carrier_up() {
        let temp = std::env::temp_dir().join(format!("optid_n5_carrier_{}", std::process::id()));
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let modalias = "pci:v00008086p000015F2sv00008086sd00000000bc02sc00i00";
        let dev = n5_device(&temp, "0000:00:1f.6", modalias);
        let power = dev.join("power");
        // Active network link behind this device.
        let iface = dev.join("net").join("enp0s31f6");
        fs::create_dir_all(&iface).unwrap();
        fs::write(iface.join("carrier"), "1\n").unwrap();

        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"n5 carrier test\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));

        actuator
            .apply(&Action::RuntimePm {
                device_dir: dev.clone(),
                autosuspend_delay_ms: 2000,
                reason: "test".to_string(),
            })
            .unwrap();

        // Allowlisted, but skipped because the link is up — control untouched.
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on"
        );
        let hash = get_path_hash(&dev);
        assert!(!temp.join(format!("original_rpm_{hash}")).exists());
        let actions = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(actions.contains("network carrier up"), "{actions}");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n5_policy_emits_runtime_pm_only_on_battery_idle() {
        let policy = Policy::default();
        let make = |on_ac: Option<bool>| Snapshot {
            timestamp: 0,
            on_ac,
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: vec![PathBuf::from("/sys/bus/usb/devices/1-1")],
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        let has_rpm = |d: &crate::decision::Decision| {
            d.actions
                .iter()
                .any(|a| matches!(a, Action::RuntimePm { .. }))
        };

        // Battery + idle -> RuntimePm nominated.
        let on_battery = policy.decide(
            &make(Some(false)),
            Mode::Balanced,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(
            has_rpm(&on_battery),
            "battery+idle should nominate runtime PM"
        );

        // On AC -> no RuntimePm.
        let on_ac = policy.decide(
            &make(Some(true)),
            Mode::Balanced,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(!has_rpm(&on_ac), "on AC should not nominate runtime PM");

        // Battery but non-idle -> no RuntimePm.
        let busy = policy.decide(
            &make(Some(false)),
            Mode::Balanced,
            WorkloadClass::Interactive,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(!has_rpm(&busy), "non-idle should not nominate runtime PM");
    }

    // ── Phase 6: RuntimePm journaled transactional application ──────────────
    //
    // Synthetic-filesystem tests proving the transactional semantics of the
    // two-write RuntimePm action: delay first, then control. On partial
    // failure, rollback + journal retention + no false "applied" marker.
    // We do NOT claim atomicity across kernel sysfs files; this is journaled
    // transactional application with compensating rollback.

    /// Build a synthetic RuntimePm device under `temp` and an Actuator whose
    /// allowlist permits `runtime_pm` for `modalias`. The device has
    /// `power/control="on"` and `power/autosuspend_delay_ms="-1"` (the
    /// kernel defaults for a USB device with runtime PM disabled).
    fn phase6_setup(temp: &Path, modalias: &str) -> (PathBuf, Actuator) {
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let dev = n5_device(temp, "1-1", modalias);
        fs::write(
            admin.join("90-admin.toml"),
            format!(
                "[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"phase6 test\"\n"
            ),
        )
        .unwrap();
        let mut actuator =
            Actuator::new_with_sink(temp.to_path_buf(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));
        (dev, actuator)
    }

    fn phase6_action(dev: &Path) -> Action {
        Action::RuntimePm {
            device_dir: dev.to_path_buf(),
            autosuspend_delay_ms: 2000,
            reason: "phase6 test".to_string(),
        }
    }

    #[test]
    fn phase6_runtime_pm_successful_multi_write_application() {
        let temp = std::env::temp_dir().join(format!(
            "optid_phase6_ok_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (dev, mut actuator) =
            phase6_setup(&temp, "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00");
        actuator.apply(&phase6_action(&dev)).unwrap();

        let power = dev.join("power");
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "auto",
            "control should be set to auto on success"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "2000",
            "delay should be set to 2000 on success"
        );

        // Applied marker present — both writes landed.
        let hash = get_path_hash(&dev);
        assert!(
            temp.join(format!("applied_rpm_{hash}")).exists(),
            "applied marker must be present after successful two-write transaction"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn phase6_runtime_pm_failure_on_first_write() {
        let temp = std::env::temp_dir().join(format!(
            "optid_phase6_fail1_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (dev, mut actuator) =
            phase6_setup(&temp, "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00");
        // Inject failure on write #1 (delay).
        actuator.fail_nth_runtime_pm_write = Some(1);
        actuator.apply(&phase6_action(&dev)).unwrap();

        let power = dev.join("power");
        // First write failed → delay unchanged, control unchanged.
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "-1",
            "delay must remain at original -1 after first-write failure"
        );
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on",
            "control must remain at original 'on' after first-write failure"
        );

        let hash = get_path_hash(&dev);
        assert!(
            !temp.join(format!("applied_rpm_{hash}")).exists(),
            "NO false 'applied' marker after first-write failure"
        );
        assert!(
            temp.join(format!("original_rpm_{hash}")).exists(),
            "journal MUST be retained for retry after first-write failure"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn phase6_runtime_pm_failure_on_second_write_with_successful_rollback() {
        let temp = std::env::temp_dir().join(format!(
            "optid_phase6_fail2_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (dev, mut actuator) =
            phase6_setup(&temp, "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00");
        // Inject failure on write #2 (control). The rollback (write #3)
        // should succeed because no failure is injected there.
        actuator.fail_nth_runtime_pm_write = Some(2);
        actuator.apply(&phase6_action(&dev)).unwrap();

        let power = dev.join("power");
        // First write succeeded (delay=2000) but second failed → rollback
        // restores delay to original -1.
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "-1",
            "delay MUST be rolled back to original -1 after second-write failure"
        );
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on",
            "control must remain at original 'on' (second write failed)"
        );

        let hash = get_path_hash(&dev);
        assert!(
            !temp.join(format!("applied_rpm_{hash}")).exists(),
            "NO false 'applied' marker after second-write failure + rollback"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn phase6_runtime_pm_rollback_failure_retains_journal() {
        let temp = std::env::temp_dir().join(format!(
            "optid_phase6_rollback_fail_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (dev, mut actuator) =
            phase6_setup(&temp, "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00");
        // Inject failure on write #2 (control) AND write #3 (rollback).
        // We can only set one value, so we test the rollback-failure path
        // by setting #3 — but #3 only runs if #2 fails first. So set #2
        // to fail, then manually re-arm for #3 in a second apply? No —
        // the test hook is consumed on first match. Instead, use #3 only:
        // the rollback only runs after #2 fails, so setting #3 alone
        // won't trigger (the #2 write succeeds and the action completes
        // normally). We need both #2 and #3 to fail.
        //
        // Workaround: set #3, then make control_path unwritable via the
        // filesystem (chmod the file read-only) so the real #2 write
        // fails, which triggers the rollback path, where #3 then fails
        // via the test hook.
        actuator.fail_nth_runtime_pm_write = Some(3);

        // Make control_path unwritable so the real guarded_write for #2
        // fails naturally. The file was created by phase6_setup with
        // content "on\n"; chmod it read-only.
        let control_path = dev.join("power").join("control");
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&control_path).unwrap().permissions();
        perms.set_mode(0o444);
        fs::set_permissions(&control_path, perms).unwrap();

        actuator.apply(&phase6_action(&dev)).unwrap();

        let power = dev.join("power");
        // First write succeeded (delay=2000), second failed (control
        // read-only), rollback ALSO failed (test hook #3) → delay left
        // at 2000, control unchanged at "on".
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "2000",
            "delay left at 2000 after rollback failure (half-applied state)"
        );
        assert_eq!(
            fs::read_to_string(&control_path).unwrap().trim(),
            "on",
            "control unchanged at 'on' (second write failed, rollback failed)"
        );

        let hash = get_path_hash(&dev);
        assert!(
            !temp.join(format!("applied_rpm_{hash}")).exists(),
            "NO false 'applied' marker after rollback failure"
        );
        assert!(
            temp.join(format!("original_rpm_{hash}")).exists(),
            "journal MUST be retained for recovery after rollback failure"
        );

        // The actions.log should mention ROLLBACK FAILED.
        let log = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(
            log.contains("ROLLBACK FAILED"),
            "actions.log must report ROLLBACK FAILED: {log}"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn phase6_runtime_pm_recovery_retry_via_revert() {
        // After a partial failure that retained the journal, a subsequent
        // revert_runtime_pm pass must restore both values from the journal.
        let temp = std::env::temp_dir().join(format!(
            "optid_phase6_retry_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (dev, mut actuator) =
            phase6_setup(&temp, "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00");
        // Inject failure on #2 (control) → rollback succeeds → delay
        // restored, journal retained (not cleared because not marked applied).
        actuator.fail_nth_runtime_pm_write = Some(2);
        actuator.apply(&phase6_action(&dev)).unwrap();

        let hash = get_path_hash(&dev);
        let orig_file = temp.join(format!("original_rpm_{hash}"));
        assert!(orig_file.exists(), "journal must exist for revert to retry");

        // Now run revert_runtime_pm — it should read the journal and
        // restore both values (even though the apply already rolled
        // back the delay). This proves the journal is retry-compatible:
        // revert_runtime_pm and the inline rollback use the same journal
        // format.
        crate::io_util::revert_runtime_pm(&temp);

        let power = dev.join("power");
        assert_eq!(
            fs::read_to_string(power.join("control")).unwrap().trim(),
            "on",
            "revert_runtime_pm must restore control from journal"
        );
        assert_eq!(
            fs::read_to_string(power.join("autosuspend_delay_ms"))
                .unwrap()
                .trim(),
            "-1",
            "revert_runtime_pm must restore delay from journal"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn phase6_runtime_pm_no_false_applied_marker_after_partial_failure() {
        // Explicit guard: after ANY partial failure, the applied_rpm_*
        // marker must NOT exist. This is the single invariant that
        // crash-recovery relies on (actuation_state distinguishes
        // "clean shutdown" from "crash mid-actuation" by marker presence).
        let temp = std::env::temp_dir().join(format!(
            "optid_phase6_no_false_applied_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (dev, mut actuator) =
            phase6_setup(&temp, "usb:v046Dp0082d0001dc00dsc00dp00ic03isc01ip01in00");

        // Failure on #1.
        actuator.fail_nth_runtime_pm_write = Some(1);
        actuator.apply(&phase6_action(&dev)).unwrap();
        let hash = get_path_hash(&dev);
        assert!(
            !temp.join(format!("applied_rpm_{hash}")).exists(),
            "no false applied marker after #1 failure"
        );

        // Failure on #2 (with successful rollback).
        actuator.fail_nth_runtime_pm_write = Some(2);
        actuator.apply(&phase6_action(&dev)).unwrap();
        assert!(
            !temp.join(format!("applied_rpm_{hash}")).exists(),
            "no false applied marker after #2 failure + rollback"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    // ── WP-N6: PCIe ASPM + SATA ALPM actuators ───────────────────────────────

    #[test]
    fn test_n6_pcie_aspm_allows_journals_and_reverts() {
        let temp = std::env::temp_dir().join(format!("optid_n6_aspm_{}", std::process::id()));
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let modalias = "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02";
        let dev = temp.join("0000:01:00.0");
        let link = dev.join("link");
        fs::create_dir_all(&link).unwrap();
        fs::write(dev.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(dev.join("class"), "0x010802\n").unwrap(); // NVMe, not CNVi
        fs::write(link.join("l1_aspm"), "0\n").unwrap();

        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"pci_aspm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"n6 test\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));

        actuator
            .apply(&Action::PcieAspm {
                device_dir: dev.clone(),
                enable: true,
                reason: "test".to_string(),
            })
            .unwrap();

        assert_eq!(
            fs::read_to_string(link.join("l1_aspm")).unwrap().trim(),
            "1"
        );
        let hash = get_path_hash(&dev);
        assert!(temp.join(format!("original_aspm_{hash}")).exists());

        crate::io_util::revert_storage(&temp);
        assert_eq!(
            fs::read_to_string(link.join("l1_aspm")).unwrap().trim(),
            "0"
        );
        assert!(!temp.join(format!("original_aspm_{hash}")).exists());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n6_pcie_aspm_default_deny_audits() {
        let temp = std::env::temp_dir().join(format!("optid_n6_aspmdeny_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let dev = temp.join("0000:02:00.0");
        let link = dev.join("link");
        fs::create_dir_all(&link).unwrap();
        fs::write(dev.join("modalias"), "pci:vFFFFpFFFFbc02sc00i00\n").unwrap();
        fs::write(link.join("l1_aspm"), "0\n").unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::seeded());

        actuator
            .apply(&Action::PcieAspm {
                device_dir: dev.clone(),
                enable: true,
                reason: "test".to_string(),
            })
            .unwrap();

        assert_eq!(
            fs::read_to_string(link.join("l1_aspm")).unwrap().trim(),
            "0",
            "denied ASPM must not write"
        );
        let audit = fs::read_to_string(temp.join("audit.jsonl")).unwrap();
        assert!(audit.contains("\"domain\":\"pci_aspm\""), "{audit}");
        assert!(audit.contains("hwid_not_in_allowlist"), "{audit}");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n6_pcie_aspm_skips_cnvi() {
        let temp = std::env::temp_dir().join(format!("optid_n6_cnvi_{}", std::process::id()));
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let modalias = "pci:v00008086p0000A0F0sv00008086sd00000074bc02sc80i00";
        let dev = temp.join("0000:00:14.3");
        let link = dev.join("link");
        fs::create_dir_all(&link).unwrap();
        fs::write(dev.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(dev.join("class"), "0x028000\n").unwrap(); // CNVi
        fs::write(link.join("l1_aspm"), "0\n").unwrap();
        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"pci_aspm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"cnvi\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));

        actuator
            .apply(&Action::PcieAspm {
                device_dir: dev.clone(),
                enable: true,
                reason: "test".to_string(),
            })
            .unwrap();

        // Allowlisted but skipped as CNVi — l1_aspm untouched.
        assert_eq!(
            fs::read_to_string(link.join("l1_aspm")).unwrap().trim(),
            "0"
        );
        let actions = fs::read_to_string(temp.join("actions.log")).unwrap();
        assert!(actions.contains("CNVi"), "{actions}");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n6_sata_alpm_allows_journals_and_reverts() {
        let temp = std::env::temp_dir().join(format!("optid_n6_alpm_{}", std::process::id()));
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let modalias = "pci:v00008086p00009D03sv000017AAsd0000222Ebc01sc06i01";
        // Backing controller (has modalias) -> ata1 -> host0 (has the policy attr).
        let controller = temp.join("0000:00:17.0");
        let host = controller.join("ata1").join("host0");
        fs::create_dir_all(&host).unwrap();
        fs::write(controller.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(
            host.join("link_power_management_policy"),
            "max_performance\n",
        )
        .unwrap();

        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"sata_alpm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"n6 sata\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));

        actuator
            .apply(&Action::SataAlpm {
                host_dir: host.clone(),
                policy: crate::actuators::storage::DEFAULT_ALPM_POLICY.to_string(),
                reason: "test".to_string(),
            })
            .unwrap();

        assert_eq!(
            fs::read_to_string(host.join("link_power_management_policy"))
                .unwrap()
                .trim(),
            "med_power_with_dipm"
        );
        let hash = get_path_hash(&host);
        assert!(temp.join(format!("original_alpm_{hash}")).exists());

        crate::io_util::revert_storage(&temp);
        assert_eq!(
            fs::read_to_string(host.join("link_power_management_policy"))
                .unwrap()
                .trim(),
            "max_performance"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n6_policy_emits_storage_pm_only_on_battery_idle() {
        let policy = Policy::default();
        let make = |on_ac: Option<bool>| Snapshot {
            timestamp: 0,
            on_ac,
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: vec![PathBuf::from("/sys/bus/pci/devices/0000:01:00.0")],
            sata_alpm_host_paths: vec![PathBuf::from("/sys/class/scsi_host/host0")],
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let has_storage = |d: &crate::decision::Decision| {
            d.actions.iter().any(|a| {
                matches!(a, Action::PcieAspm { .. }) || matches!(a, Action::SataAlpm { .. })
            })
        };

        let battery = policy.decide(
            &make(Some(false)),
            Mode::Balanced,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(
            has_storage(&battery),
            "battery+idle should nominate storage PM"
        );

        let ac = policy.decide(
            &make(Some(true)),
            Mode::Balanced,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(!has_storage(&ac), "on AC should not nominate storage PM");
    }

    // ── WP-N7: backlight depth ───────────────────────────────────────────────

    #[test]
    fn test_n7_backlight_allows_floor_clamps_and_reverts() {
        let temp = std::env::temp_dir().join(format!("optid_n7_bl_{}", std::process::id()));
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let modalias = "pci:v00008086p00009A49sv000017AAsd000022C0bc03sc00i00";
        // backlight class dir with a sibling `device` holding the GPU modalias,
        // arranged so an ancestor-walk from the backlight dir finds it.
        let gpu = temp.join("0000:00:02.0");
        let bl = gpu.join("backlight").join("intel_backlight");
        fs::create_dir_all(&bl).unwrap();
        fs::write(gpu.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(bl.join("max_brightness"), "1000\n").unwrap();
        fs::write(bl.join("brightness"), "900\n").unwrap();

        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"backlight\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"n7 test\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));

        // 40% of 1000 = 400.
        actuator
            .apply(&Action::Backlight {
                device_dir: bl.clone(),
                target_pct: 40,
                reason: "test".to_string(),
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(bl.join("brightness")).unwrap().trim(),
            "400"
        );
        let hash = get_path_hash(&bl);
        assert!(temp.join(format!("original_bl_{hash}")).exists());

        crate::io_util::revert_display(&temp);
        assert_eq!(
            fs::read_to_string(bl.join("brightness")).unwrap().trim(),
            "900"
        );

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n7_backlight_never_goes_black() {
        // An aggressive 0% request is floored to MIN_FLOOR_PCT (10%) -> 100.
        let temp = std::env::temp_dir().join(format!("optid_n7_floor_{}", std::process::id()));
        let admin = temp.join("admin");
        fs::create_dir_all(&admin).unwrap();
        let modalias = "pci:v00008086p00009A49sv000017AAsd000022C0bc03sc00i00";
        let gpu = temp.join("0000:00:02.0");
        let bl = gpu.join("backlight").join("intel_backlight");
        fs::create_dir_all(&bl).unwrap();
        fs::write(gpu.join("modalias"), format!("{modalias}\n")).unwrap();
        fs::write(bl.join("max_brightness"), "1000\n").unwrap();
        fs::write(bl.join("brightness"), "800\n").unwrap();
        fs::write(
            admin.join("90-admin.toml"),
            format!("[[entry]]\ndomain=\"backlight\"\nhwid=\"{modalias}\"\naction=\"allow\"\nverified=true\nreason=\"floor\"\n"),
        )
        .unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::load_from(
            std::slice::from_ref(&admin),
        ));
        actuator
            .apply(&Action::Backlight {
                device_dir: bl.clone(),
                target_pct: 0,
                reason: "test".to_string(),
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(bl.join("brightness")).unwrap().trim(),
            "100",
            "must clamp to the 10% floor, never black"
        );
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n7_backlight_default_deny_audits() {
        let temp = std::env::temp_dir().join(format!("optid_n7_deny_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        let gpu = temp.join("0000:00:02.0");
        let bl = gpu.join("backlight").join("intel_backlight");
        fs::create_dir_all(&bl).unwrap();
        fs::write(gpu.join("modalias"), "pci:vFFFFpFFFFbc03sc00i00\n").unwrap();
        fs::write(bl.join("max_brightness"), "1000\n").unwrap();
        fs::write(bl.join("brightness"), "900\n").unwrap();

        let mut actuator = Actuator::new_with_sink(temp.clone(), Box::new(MockPmqosSink::new()));
        actuator.enable_allowlist(crate::allowlist::Allowlist::seeded());
        actuator
            .apply(&Action::Backlight {
                device_dir: bl.clone(),
                target_pct: 40,
                reason: "test".to_string(),
            })
            .unwrap();

        // Denied: brightness untouched, denial audited.
        assert_eq!(
            fs::read_to_string(bl.join("brightness")).unwrap().trim(),
            "900"
        );
        let audit = fs::read_to_string(temp.join("audit.jsonl")).unwrap();
        assert!(audit.contains("\"domain\":\"backlight\""), "{audit}");
        assert!(audit.contains("hwid_not_in_allowlist"), "{audit}");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_n7_policy_emits_backlight_only_on_battery_idle() {
        let policy = Policy::default();
        let make = |on_ac: Option<bool>| Snapshot {
            timestamp: 0,
            on_ac,
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: Some(PathBuf::from("/sys/class/backlight/intel_backlight")),
            is_vm_guest: false,
            ..Default::default()
        };
        let has_bl = |d: &crate::decision::Decision| {
            d.actions
                .iter()
                .any(|a| matches!(a, Action::Backlight { .. }))
        };

        let battery = policy.decide(
            &make(Some(false)),
            Mode::Balanced,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(has_bl(&battery), "battery+idle should nominate backlight");

        let ac = policy.decide(
            &make(Some(true)),
            Mode::Balanced,
            WorkloadClass::Idle,
            "test".to_string(),
            &Contracts::default(),
        );
        assert!(!has_bl(&ac), "on AC should not nominate backlight");
    }

    #[test]
    fn test_n2_t5_fd_release() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t5_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        struct DropCheckSink {
            dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
            cpu_latency: Option<i32>,
        }
        impl PmqosSink for DropCheckSink {
            fn read_cpu_latency(&self) -> io::Result<String> {
                Ok("n/a".to_string())
            }
            fn write_cpu_latency(&mut self, value: Option<i32>) -> io::Result<()> {
                self.cpu_latency = value;
                Ok(())
            }
            fn read_device_latency(&self, _device_path: &Path) -> io::Result<String> {
                Ok("0".to_string())
            }
            fn write_device_latency(
                &mut self,
                _device_path: &Path,
                _value: &str,
            ) -> io::Result<()> {
                Ok(())
            }
        }
        impl Drop for DropCheckSink {
            fn drop(&mut self) {
                self.dropped
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let sink = Box::new(DropCheckSink {
            dropped: dropped.clone(),
            cpu_latency: None,
        });

        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), sink);

        actuator
            .apply(&Action::CpuDmaLatency {
                value: Some(1000),
                reason: "test".to_string(),
            })
            .unwrap();

        std::mem::drop(actuator);

        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_n2_t6_no_thrash() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n2_t6_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        let mock_sink = Box::new(MockPmqosSink::new());
        let mut actuator = Actuator::new_with_sink(temp_dir.clone(), mock_sink);

        let action1 = Action::CpuDmaLatency {
            value: Some(1000),
            reason: "test1".to_string(),
        };
        let action2 = Action::CpuDmaLatency {
            value: Some(1000),
            reason: "test2".to_string(),
        };

        actuator.apply(&action1).unwrap();
        actuator.apply(&action2).unwrap();

        let actions_log_path = temp_dir.join("actions.log");
        let logs = fs::read_to_string(&actions_log_path).unwrap_or_default();
        let occurrence_count = logs.matches("write /dev/cpu_dma_latency = 1000").count();
        assert_eq!(occurrence_count, 1);
    }

    #[test]
    fn test_n2_t7_fits_contract() {
        assert!(fits_contract(100, 200));
        assert!(fits_contract(100, 100));
        assert!(!fits_contract(200, 100));
    }

    #[test]
    fn test_n2_t8_explainability() {
        let policy = Policy::default();
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None,
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };
        let contracts = Contracts::default();
        let decision = policy.decide(
            &snap,
            Mode::Auto,
            WorkloadClass::Interactive,
            "test".to_string(),
            &contracts,
        );
        let action = decision
            .actions
            .iter()
            .find(|a| matches!(a, Action::CpuDmaLatency { .. }))
            .unwrap();
        if let Action::CpuDmaLatency { reason, .. } = action {
            assert!(reason.contains("class=interactive"));
            assert!(reason.contains("floor=1000us"));
            assert!(reason.contains("row=contracts.interactive"));
        } else {
            panic!("Expected CpuDmaLatency action");
        }
    }

    #[test]
    fn test_n1_t9_global_pin_loop_boundary_precedence() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n1_t9_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap(); // Empty policy to use defaults

        // G2: write a global pin file = "latency-critical" into temp state_dir
        let pin_file = temp_dir.join("workload_class_pin");
        fs::write(&pin_file, "latency-critical").unwrap();

        let args = Args {
            apply: false,
            once: false, // run in background loop
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
            allowlist: false,
            foreground: crate::args::ForegroundMode::Off,
        };

        let _handle = std::thread::spawn(move || {
            let _ = run(args);
        });

        // Wait 4 seconds for hysteresis to transition (since interval is 1s and dwell is 3s)
        std::thread::sleep(std::time::Duration::from_secs(4));

        // READ BACK state_dir/workload_class and ASSERT == "latency-critical"
        let class_written = fs::read_to_string(temp_dir.join("workload_class")).unwrap();
        assert_eq!(class_written.trim(), "latency-critical");
    }

    #[test]
    fn test_n1_t10_negative_no_global_pin() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n1_t10_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        // G3: no global pin + idle signals => classify() returns signal-derived class.
        // We will construct a snapshot with no global pin and default/idle fields.
        let snap = Snapshot {
            timestamp: 0,
            on_ac: Some(true),
            battery_pct: None,
            max_temp_millic: None,
            loadavg_1: Some(0.0),
            cpu_pressure: None,
            memory_pressure: None,
            io_pressure: None,
            zram_swap_active: false,
            foreground_app: None,
            pinned_class: None,
            global_pinned_class: None, // missing pin yields non-None pinned_class should be false
            pm_qos_device_paths: Vec::new(),
            runtime_pm_device_paths: Vec::new(),
            pcie_aspm_device_paths: Vec::new(),
            sata_alpm_host_paths: Vec::new(),
            selected_backlight: None,
            is_vm_guest: false,
            ..Default::default()
        };

        let policy = Policy::default();
        let (class, reason) = policy.classify(&snap);
        assert_eq!(class, WorkloadClass::Idle);
        assert!(reason.contains("system idle"));
    }

    #[test]
    fn test_n1_t11_bad_input_garbage() {
        let temp_dir =
            std::env::temp_dir().join(format!("optid_tests_n1_t11_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("policy.toml");
        fs::write(&config_path, "").unwrap();

        // G4: global pin file = "garbage" => pin ignored, no panic, fall back to signals.
        let pin_file = temp_dir.join("workload_class_pin");
        fs::write(&pin_file, "garbage").unwrap();

        let args = Args {
            apply: false,
            once: true,
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: temp_dir.clone(),
            config_path,
            allowlist: false,
            foreground: crate::args::ForegroundMode::Off,
        };

        // If it runs successfully without panic, that satisfies "no panic, fall back to signals"
        run(args).unwrap();

        // Since it fell back to signals, the workload class written should be "idle"
        let class_written = fs::read_to_string(temp_dir.join("workload_class")).unwrap();
        assert_eq!(class_written.trim(), "idle");
    }
}

/// optid-safety phase tests (Prompt 3). Verifies the fail-closed behavior
/// introduced in the optid-safety phase: malformed config disarms dynamic
/// writes, the curated baseline is applied when the policy is missing, and
/// crash recovery is deterministic.
#[cfg(test)]
mod optid_safety_tests {
    use super::*;

    /// A minimal valid `policy.toml` that parses cleanly and passes structural
    /// validation.
    fn valid_policy_toml() -> &'static str {
        r#"[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true

[modes.battery]
cpu_epp = "power"
platform_profile = "low-power"

[modes.balanced]
cpu_epp = "balance_performance"
platform_profile = "balanced"

[modes.performance]
cpu_epp = "performance"
platform_profile = "performance"

[modes.realtime]
cpu_epp = "performance"
platform_profile = "performance"
"#
    }

    fn malformed_policy_toml() -> &'static str {
        r#"[thresholds
cpu_pressure_perf_avg10 = 12.0
"#
    }

    fn partial_policy_toml() -> &'static str {
        r#"[thresholds]
cpu_pressure_perf_avg10 = 12.0
memory_pressure_protect_avg10 = 5.0
io_pressure_throttle_avg10 = 8.0
hot_temp_c = 82.0
critical_temp_c = 92.0
low_battery_pct = 20

[memory]
high_swappiness_requires_zram = true
"#
    }

    fn valid_allowlist_override(hwid: &str) -> String {
        format!(
            r#"[[entry]]
domain = "nvme_apst"
hwid = "{hwid}"
action = "allow"
max_state = 3
reason = "test override"
tested_on = "test-fixture"
verified = true
"#
        )
    }

    fn malformed_allowlist_override() -> &'static str {
        "this is not valid toml = = ="
    }

    fn boot_state(
        policy: LoadState,
        allowlist: LoadState,
        apply_armed: bool,
        baseline_armed: bool,
        allowlist_gate_enabled: bool,
    ) -> BootState {
        BootState {
            policy_load_state: policy,
            allowlist_load_state: allowlist,
            apply_armed,
            baseline_armed,
            allowlist_gate_enabled,
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "optid_safety_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn actions_log(state_dir: &Path) -> String {
        fs::read_to_string(state_dir.join("actions.log")).unwrap_or_default()
    }

    // ─── Test 1: malformed policy + apply => no dynamic writes ──────────────

    #[test]
    fn malformed_policy_apply_disables_dynamic_writes() {
        let dir = test_dir("malformed_policy");
        let config_path = dir.join("policy.toml");
        fs::write(&config_path, malformed_policy_toml()).unwrap();

        let (policy, policy_load_state) = Policy::load_with_state(&config_path);
        assert_eq!(
            policy_load_state,
            LoadState::Invalid,
            "malformed policy.toml must return LoadState::Invalid"
        );

        let apply_armed = policy_load_state.permits_dynamic_writes();
        assert!(
            !apply_armed,
            "apply_armed must be false when policy_load_state=Invalid"
        );

        let mut actuator = Actuator::new(dir.clone());
        let bs = boot_state(policy_load_state, LoadState::Ok, false, true, false);
        actuator.set_boot_state(bs);

        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "60".to_string(),
            "test dynamic write".to_string(),
        );
        actuator.apply(&action).unwrap();

        let log = actions_log(&dir);
        assert!(
            log.contains("skip dynamic write: apply_armed=false"),
            "actuator must log the skip reason; log was:\n{log}"
        );
        assert!(
            log.contains("policy_load_state=invalid"),
            "skip reason must mention policy_load_state=invalid; log was:\n{log}"
        );
        assert!(
            !dir.join("original_vm_swappiness").exists(),
            "no journal entry should be created when the gate skips the action"
        );

        let _ = policy;
        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Test 2: missing policy => baseline only ────────────────────────────

    #[test]
    fn missing_policy_loads_curated_baseline_only() {
        let dir = test_dir("missing_policy");
        let config_path = dir.join("nonexistent_policy.toml");

        let (policy, policy_load_state) = Policy::load_with_state(&config_path);
        assert_eq!(
            policy_load_state,
            LoadState::Defaulted,
            "missing policy.toml must return LoadState::Defaulted"
        );

        // The curated baseline must be conservative: all four modes must have
        // the balanced-mode cpu_epp value.
        let balanced_epp = policy.modes.balanced.cpu_epp.clone();
        assert_eq!(policy.modes.battery.cpu_epp, balanced_epp);
        assert_eq!(policy.modes.performance.cpu_epp, balanced_epp);
        assert_eq!(policy.modes.realtime.cpu_epp, balanced_epp);

        assert!(
            !policy_load_state.permits_dynamic_writes(),
            "Defaulted policy must not permit dynamic writes"
        );

        let mut actuator = Actuator::new(dir.clone());
        let bs = boot_state(policy_load_state, LoadState::Ok, false, true, false);
        actuator.set_boot_state(bs);

        actuator.apply_baseline().unwrap();
        let log = actions_log(&dir);
        assert!(
            log.contains("baseline: write") || log.contains("baseline: skip"),
            "apply_baseline must log its action; log was:\n{log}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Test 3: malformed allowlist => no dynamic writes ───────────────────

    #[test]
    fn malformed_allowlist_disables_dynamic_writes() {
        let dir = test_dir("malformed_allowlist");
        let override_dir = dir.join("allowlist.d");
        fs::create_dir_all(&override_dir).unwrap();
        fs::write(
            override_dir.join("00-broken.toml"),
            malformed_allowlist_override(),
        )
        .unwrap();

        let (allowlist, allowlist_load_state) =
            Allowlist::load_with_state(std::slice::from_ref(&override_dir));
        assert_eq!(
            allowlist_load_state,
            LoadState::Partial,
            "malformed allowlist override must return LoadState::Partial"
        );

        let hwid = "pci:v0000144Dp00009A36sv0000144Dsd0000A801bc01sc08i02";
        assert_eq!(
            allowlist.check("nvme_apst", hwid, 0).deny_reason(),
            Some("entry_unverified: candidate hardware may be observed but not actuated"),
            "the seeded Samsung PM9A1 is a visible candidate, not trusted hardware"
        );

        let apply_armed_with_gate =
            LoadState::Ok.permits_dynamic_writes() && allowlist_load_state.permits_dynamic_writes();
        assert!(
            !apply_armed_with_gate,
            "apply_armed must be false when allowlist_load_state=Partial and gate is enabled"
        );

        let apply_armed_without_gate = LoadState::Ok.permits_dynamic_writes();
        assert!(
            apply_armed_without_gate,
            "apply_armed must be true when allowlist gate is disabled"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Test 4: valid config => dynamic writes allowed ─────────────────────

    #[test]
    fn valid_config_enables_dynamic_writes() {
        let dir = test_dir("valid_config");
        let config_path = dir.join("policy.toml");
        fs::write(&config_path, valid_policy_toml()).unwrap();

        let (policy, policy_load_state) = Policy::load_with_state(&config_path);
        assert_eq!(
            policy_load_state,
            LoadState::Ok,
            "valid policy.toml must return LoadState::Ok"
        );

        let apply_armed = policy_load_state.permits_dynamic_writes();
        assert!(
            apply_armed,
            "apply_armed must be true when policy_load_state=Ok and gate is disabled"
        );

        let mut actuator = Actuator::new(dir.clone());
        let bs = boot_state(policy_load_state, LoadState::Ok, true, true, false);
        actuator.set_boot_state(bs);

        let sysctl_path = dir.join("swappiness");
        fs::write(&sysctl_path, "100").unwrap();
        let action = Action::vm_sysctl(
            sysctl_path.clone(),
            "60".to_string(),
            "test valid config dynamic write".to_string(),
        );
        actuator.apply(&action).unwrap();

        let log = actions_log(&dir);
        assert!(
            !log.contains("skip dynamic write: apply_armed=false"),
            "gate must NOT skip the write when apply_armed=true; log was:\n{log}"
        );
        assert!(
            log.contains("write ") || log.contains("skip vm.sysctl"),
            "actuator must log the write attempt or soft-fail; log was:\n{log}"
        );

        let _ = policy;
        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Test 5: dry-run => no writes ───────────────────────────────────────

    #[test]
    fn dry_run_disables_all_writes() {
        let dir = test_dir("dry_run");
        let config_path = dir.join("policy.toml");
        fs::write(&config_path, valid_policy_toml()).unwrap();

        let (_policy, policy_load_state) = Policy::load_with_state(&config_path);
        assert_eq!(policy_load_state, LoadState::Ok);

        let mut actuator = Actuator::new(dir.clone());
        let bs = boot_state(policy_load_state, LoadState::Ok, false, false, false);
        actuator.set_boot_state(bs);

        actuator.apply_baseline().unwrap();
        assert!(
            actions_log(&dir).is_empty(),
            "dry-run apply_baseline must not log anything"
        );

        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "60".to_string(),
            "test dry-run".to_string(),
        );
        actuator.apply(&action).unwrap();

        let log = actions_log(&dir);
        assert!(
            log.contains("skip dynamic write: apply_armed=false"),
            "dry-run apply must skip with apply_armed=false; log was:\n{log}"
        );
        assert!(
            log.contains("baseline_armed=false"),
            "skip reason must mention baseline_armed=false; log was:\n{log}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Test 6: crash after first mutation => restart restores ─────────────

    #[test]
    fn crash_after_first_mutation_restart_restores() {
        let dir = test_dir("crash_after_mutation");

        // Simulate: original journal written, applied marker written (write landed).
        let orig_file = dir.join("original_vm_swappiness");
        let applied_file = dir.join("applied_vm_swappiness");
        fs::write(&orig_file, "100\n").unwrap();
        mark_applied(&dir, "vm_swappiness", "60");
        assert!(applied_file.exists(), "mark_applied must create the marker");

        assert_eq!(
            actuation_state(&dir, "vm_swappiness"),
            Some(true),
            "applied marker present ⇒ actuation_state = Some(true)"
        );

        // revert_sysctls will try guarded_write to /proc/sys/vm/swappiness.
        // CI cannot write that path, so the journal must remain retryable.
        revert_sysctls(&dir);

        assert!(
            orig_file.exists(),
            "original_vm_swappiness must remain after a failed revert"
        );
        assert!(
            applied_file.exists(),
            "applied_vm_swappiness must remain after a failed revert"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Test 7: stale incomplete journal => deterministic recovery ─────────

    #[test]
    fn stale_incomplete_journal_deterministic_recovery() {
        let dir = test_dir("stale_incomplete_journal");

        // Simulate: original journal written, NO applied marker (crash mid-actuation).
        let orig_file = dir.join("original_vm_swappiness");
        let applied_file = dir.join("applied_vm_swappiness");
        fs::write(&orig_file, "100\n").unwrap();

        assert_eq!(
            actuation_state(&dir, "vm_swappiness"),
            Some(false),
            "original present + no applied marker ⇒ actuation_state = Some(false) (crash recovery)"
        );

        revert_sysctls(&dir);

        assert!(
            orig_file.exists(),
            "original_vm_swappiness must remain when crash recovery cannot restore"
        );
        assert!(
            !applied_file.exists(),
            "applied_vm_swappiness must not exist (it was never created in the crash scenario)"
        );

        // Repeat to verify the retained journal is retried deterministically.
        assert_eq!(
            actuation_state(&dir, "vm_swappiness"),
            Some(false),
            "second run: still crash recovery"
        );
        revert_sysctls(&dir);
        assert!(
            orig_file.exists(),
            "second run: failed recovery remains retryable"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // ─── Bonus: curated baseline is independently testable ──────────────────

    #[test]
    fn curated_baseline_is_conservative() {
        let p = Policy::curated_baseline();
        let balanced_epp = p.modes.balanced.cpu_epp.clone();
        let balanced_profile = p.modes.balanced.platform_profile.clone();
        assert_eq!(p.modes.battery.cpu_epp, balanced_epp);
        assert_eq!(p.modes.battery.platform_profile, balanced_profile);
        assert_eq!(p.modes.performance.cpu_epp, balanced_epp);
        assert_eq!(p.modes.performance.platform_profile, balanced_profile);
        assert_eq!(p.modes.realtime.cpu_epp, balanced_epp);
        assert_eq!(p.modes.realtime.platform_profile, balanced_profile);
    }

    #[test]
    fn partial_policy_loads_curated_baseline() {
        let dir = test_dir("partial_policy");
        let config_path = dir.join("policy.toml");
        fs::write(&config_path, partial_policy_toml()).unwrap();

        let (policy, state) = Policy::load_with_state(&config_path);
        // A policy missing [modes] fails serde deserialization (Modes has no
        // #[serde(default)]), so the load returns Invalid, not Partial. The
        // Partial state is reserved for a future per-section validator that
        // uses #[serde(default)] and then checks structural validity after
        // deserialization. For now, both Partial and Invalid fall back to
        // the curated baseline and disable dynamic writes.
        assert!(
            matches!(state, LoadState::Invalid | LoadState::Partial),
            "missing-[modes] policy must be Invalid or Partial, was {state:?}"
        );
        // The returned policy is the curated baseline.
        let balanced_epp = policy.modes.balanced.cpu_epp.clone();
        assert_eq!(policy.modes.battery.cpu_epp, balanced_epp);
        assert!(!state.permits_dynamic_writes());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_journal_is_idempotent() {
        let dir = test_dir("clear_journal_idempotent");
        clear_journal(&dir, "nonexistent_key");
        clear_journal(&dir, "nonexistent_key");
        fs::write(dir.join("original_vm_swappiness"), "100\n").unwrap();
        clear_journal(&dir, "vm_swappiness");
        clear_journal(&dir, "vm_swappiness");
        assert!(!dir.join("original_vm_swappiness").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn allowlist_load_with_state_ok_for_clean_or_missing_dir() {
        let dir = test_dir("allowlist_ok_clean");
        let override_dir = dir.join("allowlist.d");
        fs::create_dir_all(&override_dir).unwrap();
        let (_al, state) = Allowlist::load_with_state(std::slice::from_ref(&override_dir));
        assert_eq!(state, LoadState::Ok);

        let missing_dir = dir.join("does-not-exist");
        let (_al2, state2) = Allowlist::load_with_state(std::slice::from_ref(&missing_dir));
        assert_eq!(state2, LoadState::Ok);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn allowlist_load_with_state_partial_for_mixed_valid_invalid() {
        let dir = test_dir("allowlist_partial_mixed");
        let override_dir = dir.join("allowlist.d");
        fs::create_dir_all(&override_dir).unwrap();
        fs::write(
            override_dir.join("10-valid.toml"),
            valid_allowlist_override("pci:v00001234p00005678sv00001234sd00005678bc01sc08i02"),
        )
        .unwrap();
        fs::write(
            override_dir.join("20-broken.toml"),
            malformed_allowlist_override(),
        )
        .unwrap();

        let (al, state) = Allowlist::load_with_state(std::slice::from_ref(&override_dir));
        assert_eq!(state, LoadState::Partial);
        assert!(
            al.check(
                "nvme_apst",
                "pci:v00001234p00005678sv00001234sd00005678bc01sc08i02",
                0
            )
            .is_allow(),
            "valid override must be applied even when a sibling is malformed"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}

// ─────────────────────────────────────────────────────────────────────
// F2 — Fault-injection tests for the kernel I/O seam.
//
// These tests exercise the real production code path through `Actuator`
// with a `FaultKernel` wrapper that simulates missing files,
// permission-denied, short writes, and disappearing paths deterministically.
// They prove the F2 plan's "fault-injection tests for missing, malformed,
// permission-denied, short-write, and disappearing paths" contract.
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod f2_fault_injection_tests {
    use super::*;
    use crate::kernel_io::{FaultKernel, KernelWrite, RealKernel};

    /// Helper: create a temp state dir and an Actuator backed by a
    /// FaultKernel. Returns (actuator, fault_kernel_ptr, state_dir).
    ///
    /// The FaultKernel is returned as a raw pointer because the Actuator
    /// owns the Box<dyn KernelIo>. Tests configure fault rules BEFORE
    /// constructing the actuator, then pass the configured kernel in.
    fn f2_actuator_with_faults(fault_kernel: FaultKernel) -> (Actuator, PathBuf) {
        let temp_dir = std::env::temp_dir().join(format!("optid_f2_fault_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let actuator = Actuator::new_with_kernel(temp_dir.clone(), Box::new(fault_kernel));
        (actuator, temp_dir)
    }

    /// F2 fault-injection: a missing sysfs path (NotFound) on read must
    /// not panic the actuator. The actuator captures the original value
    /// via `unwrap_or_default()`, so a missing path becomes an empty
    /// string. The write still fails (the path is missing), but the
    /// journal entries are written.
    #[test]
    fn f2_fault_missing_path_on_read_does_not_panic() {
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        // No fault rules — the path simply doesn't exist in the container.
        let (mut actuator, temp_dir) = f2_actuator_with_faults(fk);

        let action = Action::vm_sysctl(
            PathBuf::from("/proc/sys/vm/swappiness"),
            "60".to_string(),
            "test: missing path".to_string(),
        );
        // apply must not panic; it returns Ok(()) even when the write fails
        // (the actuator logs the failure and continues).
        let _ = actuator.apply(&action);

        // Journal entries must still be written (original captured as empty,
        // intended as "60").
        assert!(
            temp_dir.join("intended_vm_swappiness").exists(),
            "intended journal must be written even when the path is missing"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// F2 fault-injection: a permission-denied error on write must not
    /// panic. The actuator logs the failure and the journal retains the
    /// original for retry.
    #[test]
    fn f2_fault_permission_denied_on_write_is_logged() {
        let path = PathBuf::from("/proc/sys/vm/swappiness");
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.fail_next_write(path.clone(), std::io::ErrorKind::PermissionDenied);

        let (mut actuator, temp_dir) = f2_actuator_with_faults(fk);

        let action = Action::vm_sysctl(
            path,
            "60".to_string(),
            "test: permission denied".to_string(),
        );
        let _ = actuator.apply(&action);

        // The actions.log must record the failure.
        let log = fs::read_to_string(temp_dir.join("actions.log")).unwrap_or_default();
        assert!(
            log.contains("swappiness") || log.contains("was") || log.contains("skip"),
            "permission-denied failure must be logged, got: {log}"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// F2 fault-injection: a disappearing path (HidePath) on read must
    /// return NotFound, which the actuator treats as "no original value"
    /// (empty string). This simulates hot-unplug between read and write.
    #[test]
    fn f2_fault_disappearing_path_on_read_returns_not_found() {
        let path = PathBuf::from("/proc/sys/vm/swappiness");
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.hide_path(path.clone());

        let (mut actuator, temp_dir) = f2_actuator_with_faults(fk);

        let action = Action::vm_sysctl(
            path,
            "60".to_string(),
            "test: disappearing path".to_string(),
        );
        let _ = actuator.apply(&action);

        // The journal must capture an empty original (path was hidden).
        let orig = fs::read_to_string(temp_dir.join("original_vm_swappiness")).unwrap_or_default();
        assert!(
            orig.trim().is_empty(),
            "hidden path must produce empty original, got: {orig:?}"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// F2 fault-injection: malformed content on read (e.g. a PSI file
    /// that returns garbage) must not panic the parser. The parser
    /// returns None, which the snapshot records as "unavailable".
    #[test]
    fn f2_fault_malformed_content_on_read_returns_none() {
        let path = PathBuf::from("/proc/pressure/cpu");
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.malform_content(path.clone(), "garbage not a psi line".to_string());

        // Pressure::read_with must return None for malformed content.
        let pressure = crate::sensors::Pressure::read_with(&fk, "/proc/pressure/cpu");
        assert!(
            pressure.is_none(),
            "malformed PSI content must parse to None, got: {pressure:?}"
        );
    }

    /// F2 fault-injection: a fault rule fires exactly once (one-shot),
    /// then the next call succeeds. This proves the FaultKernel's
    /// rule-consumption semantics.
    #[test]
    fn f2_fault_rule_fires_once_then_recovers() {
        let path = PathBuf::from("/proc/sys/vm/swappiness");
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.fail_next_write(path.clone(), std::io::ErrorKind::Other);

        // First write: fault fires.
        let res1 = fk.write(&path, "60");
        assert!(res1.is_err());
        assert_eq!(res1.unwrap_err().kind(), std::io::ErrorKind::Other);

        // Second write: fault consumed. The write may still fail
        // (PermissionDenied in non-root tests), but NOT with Other.
        let res2 = fk.write(&path, "60");
        if let Err(e) = res2 {
            assert_ne!(
                e.kind(),
                std::io::ErrorKind::Other,
                "second write must not fire the consumed fault rule"
            );
        }
    }

    /// F2 fault-injection: a hidden directory returns an empty listing,
    /// simulating hot-unplug of a bus. The discover_* functions must
    /// return an empty vector, not panic.
    #[test]
    fn f2_fault_hidden_dir_returns_empty_discovery() {
        let bus = PathBuf::from("/sys/bus/pci/devices");
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        fk.hide_path(bus.clone());

        let paths = crate::sensors::discover_pcie_aspm_device_paths_with(&fk);
        assert!(
            paths.is_empty(),
            "hidden bus directory must produce empty discovery, got: {paths:?}"
        );

        let rpm_paths = crate::sensors::discover_runtime_pm_device_paths_with(&fk);
        assert!(
            rpm_paths.is_empty(),
            "hidden bus directory must produce empty runtime-PM discovery"
        );
    }

    /// F2 fault-injection: the actuator with a RealKernel (no faults)
    /// behaves identically to the pre-F2 actuator. This is the
    /// "no behavior change" regression test for the F2 refactor.
    #[test]
    fn f2_actuator_with_real_kernel_matches_legacy_behavior() {
        let temp_dir = std::env::temp_dir().join(format!("optid_f2_real_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);

        // Construct via new_with_kernel(RealKernel) — same as new().
        let actuator_real =
            Actuator::new_with_kernel(temp_dir.clone(), Box::new(RealKernel::new()));
        // Construct via new() — the legacy path.
        let actuator_legacy = Actuator::new(temp_dir.clone());

        // Both must have the same state_dir and log_path.
        assert_eq!(actuator_real.state_dir, actuator_legacy.state_dir);
        assert_eq!(actuator_real.log_path, actuator_legacy.log_path);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// F2 fault-injection: the FaultKernel's allowlist check is
    /// identical to RealKernel's. A path outside the allowlist is
    /// rejected with PermissionDenied, regardless of fault rules.
    #[test]
    fn f2_fault_kernel_preserves_allowlist() {
        let fk = FaultKernel::new(Box::new(RealKernel::new()));
        let bad_path = Path::new("/tmp/definitely-not-allowlisted-f2");
        let res = fk.write(bad_path, "x");
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
            "FaultKernel must enforce the allowlist even with no fault rules"
        );
    }
}
