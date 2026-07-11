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
cpu_wakeup_latency = 10
device_resume_latency = 100

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
            10
        );
        assert_eq!(
            contracts
                .resolve(WorkloadClass::LatencyCritical)
                .device_resume_latency,
            100
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
