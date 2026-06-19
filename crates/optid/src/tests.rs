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
use crate::args::Args;
use crate::contracts::{fits_contract, Contracts};
use crate::io_util::{get_path_hash, revert_pm_qos, revert_sysctls};
use crate::policy::Policy;
use crate::run;
use crate::sensors::{Pressure, Snapshot};
use crate::workload::{Mode, WorkloadClass};

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_t1_dry_run_no_op() {
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
        };

        run(args).unwrap();

        assert!(!temp_dir.join("intended_vm_swappiness").exists());

        let decisions = fs::read_to_string(temp_dir.join("decisions.log")).unwrap();
        assert!(decisions.contains("vm.* actuation skipped: zram swap is not active"));
    }

    #[test]
    fn test_t2_apply_allowlisted_and_t4_revert() {
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
        assert!(!temp_dir.join("intended_vm_swappiness").exists());
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
                "[[entry]]\ndomain=\"runtime_pm\"\nhwid=\"{modalias}\"\naction=\"allow\"\nreason=\"tested in N4 unit test\"\n"
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
        };

        // If it runs successfully without panic, that satisfies "no panic, fall back to signals"
        run(args).unwrap();

        // Since it fell back to signals, the workload class written should be "idle"
        let class_written = fs::read_to_string(temp_dir.join("workload_class")).unwrap();
        assert_eq!(class_written.trim(), "idle");
    }
}
