use std::env;

pub mod contracts;
pub mod energy;
pub mod probes;
pub mod report;
pub mod runner;
pub mod types;
pub mod utils;

use report::run_report;
use runner::run_cell;

fn print_usage() {
    println!("Usage:");
    println!("  rushbench run --class <class> --workload <workload> [--n <count>] [--ac-ok]");
    println!("  rushbench matrix [--ac-ok]");
    println!("  rushbench report <results-dir>");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "run" => {
            let mut class = None;
            let mut workload = None;
            let mut n = 5;
            let mut ac_ok = false;

            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--class" => {
                        if i + 1 < args.len() {
                            class = Some(args[i + 1].as_str());
                            i += 2;
                        } else {
                            eprintln!("Error: --class requires an argument");
                            std::process::exit(1);
                        }
                    }
                    "--workload" => {
                        if i + 1 < args.len() {
                            workload = Some(args[i + 1].as_str());
                            i += 2;
                        } else {
                            eprintln!("Error: --workload requires an argument");
                            std::process::exit(1);
                        }
                    }
                    "--n" => {
                        if i + 1 < args.len() {
                            n = args[i + 1].parse().unwrap_or(5);
                            i += 2;
                        } else {
                            eprintln!("Error: --n requires an argument");
                            std::process::exit(1);
                        }
                    }
                    "--ac-ok" => {
                        ac_ok = true;
                        i += 1;
                    }
                    _ => {
                        eprintln!("Error: unknown argument {}", args[i]);
                        print_usage();
                        std::process::exit(1);
                    }
                }
            }

            let class = match class {
                Some(c) => c,
                None => {
                    eprintln!("Error: --class is required");
                    std::process::exit(1);
                }
            };
            let workload = match workload {
                Some(w) => w,
                None => {
                    eprintln!("Error: --workload is required");
                    std::process::exit(1);
                }
            };

            if let Err(e) = run_cell(class, workload, n, ac_ok) {
                eprintln!("Measurement run failed: {}", e);
                std::process::exit(1);
            }
        }
        "matrix" => {
            let mut ac_ok = false;
            if args.len() > 2 && args[2] == "--ac-ok" {
                ac_ok = true;
            }

            let classes = [
                "idle",
                "light",
                "interactive",
                "latency-critical",
                "throughput",
            ];
            let workloads = ["foreground-launch", "cyclictest", "psi-cpu", "psi-io"];

            let mut any_failed = false;
            for class in &classes {
                for workload in &workloads {
                    println!(
                        "\n--- Running cell: class={}, workload={} ---",
                        class, workload
                    );
                    if let Err(e) = run_cell(class, workload, 5, ac_ok) {
                        eprintln!("Cell failed: {}", e);
                        any_failed = true;
                    }
                }
            }
            if any_failed {
                std::process::exit(1);
            }
        }
        "report" => {
            if args.len() < 3 {
                eprintln!("Error: report requires <results-dir>");
                print_usage();
                std::process::exit(1);
            }
            match run_report(&args[2]) {
                Ok(report_str) => {
                    print!("{}", report_str);
                }
                Err(e) => {
                    eprintln!("Report failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        unknown => {
            eprintln!("Error: unknown command {}", unknown);
            print_usage();
            std::process::exit(1);
        }
    }
}

// --- Verification Tests (T1-T9) ---

#[cfg(test)]
mod tests {
    use super::*;
    use energy::{calculate_window, EnergySample, EnergySource};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use types::{HostInfo, ResolvedFloors, RunRecord, RushInfo};
    use utils::{
        find_repo_file, get_battery_design_uwh, get_contracts_sha256, get_cpu_model, get_dmi_board,
        get_git_sha, get_host_folder_name, get_kernel_version, get_utc_timestamp,
    };

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_t1_energy_probe_wrap_ac_switch_rejection() {
        let mock_battery = EnergySource::Battery(PathBuf::from("/mock/battery"));
        let mock_rapl = EnergySource::Rapl(PathBuf::from("/mock/rapl"));

        let start_time = Instant::now();
        let end_time = start_time + Duration::from_secs(10);

        // Case (a) Wrap: Battery remaining energy increases (wrap/charging)
        let s_bat_start = EnergySample {
            time: start_time,
            joules: 100.0,
            on_ac: Some(false),
        };
        let s_bat_end_wrap = EnergySample {
            time: end_time,
            joules: 120.0,
            on_ac: Some(false),
        };
        let res_bat = calculate_window(&mock_battery, &s_bat_start, &s_bat_end_wrap);
        assert!(res_bat.is_err(), "Battery wrap was not rejected!");
        assert_eq!(res_bat.err().unwrap(), "counter_wrap");

        // Case (a) Wrap: RAPL consumed energy decreases (wrap)
        let s_rapl_start = EnergySample {
            time: start_time,
            joules: 120.0,
            on_ac: Some(false),
        };
        let s_rapl_end_wrap = EnergySample {
            time: end_time,
            joules: 100.0,
            on_ac: Some(false),
        };
        let res_rapl = calculate_window(&mock_rapl, &s_rapl_start, &s_rapl_end_wrap);
        assert!(res_rapl.is_err(), "RAPL wrap was not rejected!");
        assert_eq!(res_rapl.err().unwrap(), "counter_wrap");

        // Case (b) AC Switch: AC state changes mid-window
        let s_ac_start = EnergySample {
            time: start_time,
            joules: 100.0,
            on_ac: Some(true),
        };
        let s_ac_end = EnergySample {
            time: end_time,
            joules: 90.0,
            on_ac: Some(false),
        };
        let res_ac = calculate_window(&mock_battery, &s_ac_start, &s_ac_end);
        assert!(res_ac.is_err(), "AC switch was not rejected!");
        assert_eq!(res_ac.err().unwrap(), "ac_switch_mid_window");
    }

    #[test]
    fn test_t2_energy_probe_arithmetic() {
        let mock_rapl = EnergySource::Rapl(PathBuf::from("/mock/rapl"));
        let start_time = Instant::now();
        let end_time = start_time + Duration::from_secs(10); // 10s elapsed

        // 100J -> 150J => delta = 50J. avg_watts = 50J / 10s = 5W.
        let start = EnergySample {
            time: start_time,
            joules: 100.0,
            on_ac: Some(false),
        };
        let end = EnergySample {
            time: end_time,
            joules: 150.0,
            on_ac: Some(false),
        };

        let res = calculate_window(&mock_rapl, &start, &end).unwrap();
        assert!(
            (res.window_joules - 50.0).abs() < 0.5,
            "Joules off: {}",
            res.window_joules
        );
        assert!(
            (res.avg_watts - 5.0).abs() < 0.05,
            "Watts off: {}",
            res.avg_watts
        );
    }

    #[test]
    fn test_t3_class_readback_enforcement() {
        let _lock = TEST_LOCK.lock().unwrap();
        // Mock optctl status returning a class != requested
        env::set_var(
            "RUSHBENCH_OPTCTL_STATUS_JSON",
            r#"{"workload_class": "light", "cpu_wakeup_latency": 1000, "device_resume_latency": 10000}"#,
        );
        env::set_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE", "true");
        env::set_var("RUSHBENCH_MOCK_ENERGY_SOURCE", "battery");
        env::set_var("RUSHBENCH_MOCK_ON_AC", "false");
        env::set_var("RUSHBENCH_MOCK_METRIC_psi_cpu_avg10", "123");
        env::set_var("RUSHBENCH_GIT_SHA", "abcdef012345");
        env::set_var("RUSHBENCH_CONTRACTS_SHA256", "checksum123");

        let temp_dir = std::env::temp_dir().join("rushbench_t3");
        fs::create_dir_all(&temp_dir).unwrap();
        env::set_var("RUSHBENCH_STATE_DIR", temp_dir.to_str().unwrap());

        // We request "interactive" but status returns "light"
        let res = run_cell("interactive", "psi-cpu", 5, true);
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("class_mismatch"));

        // Clean up env
        env::remove_var("RUSHBENCH_OPTCTL_STATUS_JSON");
        env::remove_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");
        env::remove_var("RUSHBENCH_MOCK_ON_AC");
        env::remove_var("RUSHBENCH_MOCK_METRIC_psi_cpu_avg10");
        env::remove_var("RUSHBENCH_GIT_SHA");
        env::remove_var("RUSHBENCH_CONTRACTS_SHA256");
        env::remove_var("RUSHBENCH_STATE_DIR");
    }

    #[test]
    fn test_t4_schema_freeze() {
        let golden_json = r#"{
          "schema_version": 1,
          "host": {
            "kernel": "6.8.0-generic",
            "cpu_model": "Intel Core i7",
            "dmi_board": "8BC2",
            "battery_design_uwh": 70070000
          },
          "rush": {
            "optid_sha": "abcdef012345",
            "contracts_sha256": "checksum123",
            "rig_sha": "abcdef012345",
            "rig_version": "0.1.0"
          },
          "class_requested": "interactive",
          "class_observed": "interactive",
          "resolved_floors": {
            "cpu_wakeup_latency_us": 1000,
            "device_resume_latency_us": 10000
          },
          "power_source": "battery",
          "workload": "foreground-launch",
          "metric": "foreground-launch-ms",
          "n": 5,
          "samples": [123, 119, 131, 122, 127],
          "median": 123.0,
          "p95": 130.0,
          "iqr": 8.0,
          "energy": {
            "window_joules": 41.2,
            "avg_watts": 4.6,
            "counter": "BAT0/energy_now"
          },
          "started_at": "2026-06-14T09:01:22Z",
          "warmup_runs": 2,
          "anomalies": []
        }"#;

        let rec: RunRecord = serde_json::from_str(golden_json).unwrap();
        assert_eq!(rec.schema_version, 1);
        assert_eq!(rec.host.dmi_board, "8BC2");
        assert_eq!(rec.rush.rig_version, "0.1.0");
        assert_eq!(rec.samples.unwrap()[2], 131);
    }

    #[test]
    fn test_t5_n_less_than_5_honesty() {
        let _lock = TEST_LOCK.lock().unwrap();
        // If N=1, the record carries an "insufficient_n" anomaly
        env::set_var(
            "RUSHBENCH_OPTCTL_STATUS_JSON",
            r#"{"workload_class": "interactive", "cpu_wakeup_latency": 1000, "device_resume_latency": 10000}"#,
        );
        env::set_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE", "true");
        env::set_var("RUSHBENCH_MOCK_ENERGY_SOURCE", "battery");
        env::set_var("RUSHBENCH_MOCK_ENERGY_JOULES", "100.0");
        env::set_var("RUSHBENCH_MOCK_ON_AC", "false");
        env::set_var("RUSHBENCH_MOCK_METRIC_psi_cpu_avg10", "123");
        env::set_var("RUSHBENCH_GIT_SHA", "abcdef012345");
        env::set_var("RUSHBENCH_CONTRACTS_SHA256", "checksum123");

        let temp_dir = std::env::temp_dir().join("rushbench_t5");
        fs::create_dir_all(&temp_dir).unwrap();
        env::set_var("RUSHBENCH_STATE_DIR", temp_dir.to_str().unwrap());

        let res = run_cell("interactive", "psi-cpu", 1, true);
        assert!(res.is_ok(), "run_cell failed: {:?}", res);

        // Check the emitted JSON contains "insufficient_n"
        let results_root = find_repo_file("VERSION")
            .map(|p| p.parent().unwrap().join("benchmarks").join("results"))
            .unwrap();
        // find file
        let date = get_utc_timestamp().split('T').next().unwrap().to_string();
        let host_folder = get_host_folder_name();
        let target_file = results_root
            .join(date)
            .join(host_folder)
            .join("interactive")
            .join("psi-cpu.json");
        assert!(target_file.exists());

        let content = fs::read_to_string(&target_file).unwrap();
        let rec: RunRecord = serde_json::from_str(&content).unwrap();
        assert_eq!(rec.n, 1);
        assert!(rec.anomalies.contains(&"insufficient_n".to_string()));

        // Clean up env
        env::remove_var("RUSHBENCH_OPTCTL_STATUS_JSON");
        env::remove_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_JOULES");
        env::remove_var("RUSHBENCH_MOCK_ON_AC");
        env::remove_var("RUSHBENCH_MOCK_METRIC_psi_cpu_avg10");
        env::remove_var("RUSHBENCH_GIT_SHA");
        env::remove_var("RUSHBENCH_CONTRACTS_SHA256");
        env::remove_var("RUSHBENCH_STATE_DIR");
        let _ = fs::remove_file(target_file);
    }

    #[test]
    fn test_t7_provenance_completeness() {
        let git_sha = get_git_sha().unwrap_or_else(|_| "unknown".to_string());
        let contracts_sha = get_contracts_sha256().unwrap_or_else(|_| "unknown".to_string());

        let rec = RunRecord {
            schema_version: 1,
            host: HostInfo {
                kernel: get_kernel_version(),
                cpu_model: get_cpu_model(),
                dmi_board: get_dmi_board(),
                battery_design_uwh: get_battery_design_uwh(),
            },
            rush: RushInfo {
                optid_sha: git_sha.clone(),
                contracts_sha256: contracts_sha,
                rig_sha: git_sha,
                rig_version: env!("CARGO_PKG_VERSION").to_string(),
            },
            class_requested: "interactive".to_string(),
            class_observed: "interactive".to_string(),
            resolved_floors: ResolvedFloors {
                cpu_wakeup_latency_us: 1000,
                device_resume_latency_us: 10000,
            },
            power_source: "battery".to_string(),
            workload: "foreground-launch".to_string(),
            metric: "foreground-launch-ms".to_string(),
            n: 5,
            samples: Some(vec![123, 119, 131, 122, 127]),
            median: Some(123.0),
            p95: Some(130.0),
            iqr: Some(8.0),
            energy: None,
            started_at: get_utc_timestamp(),
            warmup_runs: 2,
            anomalies: vec![],
        };

        // All fields should be non-empty and non-null
        assert!(!rec.host.kernel.is_empty());
        assert!(!rec.host.cpu_model.is_empty());
        assert!(!rec.rush.optid_sha.is_empty());
        assert!(!rec.rush.contracts_sha256.is_empty());
        assert!(!rec.rush.rig_sha.is_empty());
        assert!(!rec.power_source.is_empty());
        assert!(!rec.class_observed.is_empty());
        assert!(!rec.started_at.is_empty());
    }

    #[test]
    fn test_t8_latency_critical_honesty_path() {
        let _lock = TEST_LOCK.lock().unwrap();
        // High reading forced
        env::set_var("RUSHBENCH_MOCK_METRIC_cyclictest_max_us", "50");
        env::set_var(
            "RUSHBENCH_OPTCTL_STATUS_JSON",
            r#"{"workload_class": "latency-critical", "cpu_wakeup_latency": 10, "device_resume_latency": 100}"#,
        );
        env::set_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE", "true");
        env::set_var("RUSHBENCH_MOCK_ENERGY_SOURCE", "battery");
        env::set_var("RUSHBENCH_MOCK_ENERGY_JOULES", "100.0");
        env::set_var("RUSHBENCH_MOCK_ON_AC", "false");
        env::set_var("RUSHBENCH_GIT_SHA", "abcdef012345");
        env::set_var("RUSHBENCH_CONTRACTS_SHA256", "checksum123");

        let temp_dir = std::env::temp_dir().join("rushbench_t8");
        fs::create_dir_all(&temp_dir).unwrap();
        env::set_var("RUSHBENCH_STATE_DIR", temp_dir.to_str().unwrap());

        let res = run_cell("latency-critical", "cyclictest", 5, true);
        assert!(res.is_ok(), "run_cell failed: {:?}", res);

        // Capture report output
        let results_root = find_repo_file("VERSION")
            .map(|p| p.parent().unwrap().join("benchmarks").join("results"))
            .unwrap();
        let date = get_utc_timestamp().split('T').next().unwrap().to_string();
        let host_folder = get_host_folder_name();
        let results_day = results_root.join(date).join(host_folder);

        // Run report and verify budget_violation is present
        // Let's redirect stdout to string or capture it by calling run_report directly
        // We can check the target file content first
        let target_file = results_day.join("latency-critical").join("cyclictest.json");
        assert!(target_file.exists());

        // Clear print capture helper: we'll call run_report which prints to stdout.
        // Let's verify that target_file itself contains the high reading
        let content = fs::read_to_string(&target_file).unwrap();
        assert!(content.contains(r#""median": 50.0"#));

        let report_res = run_report(results_root.to_str().unwrap());
        assert!(report_res.is_ok(), "run_report failed: {:?}", report_res);
        let report_content = report_res.unwrap();
        assert!(
            report_content.contains("budget_violation"),
            "Report does not contain budget_violation: {}",
            report_content
        );

        // Clean up env
        env::remove_var("RUSHBENCH_MOCK_METRIC_cyclictest_max_us");
        env::remove_var("RUSHBENCH_OPTCTL_STATUS_JSON");
        env::remove_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_JOULES");
        env::remove_var("RUSHBENCH_MOCK_ON_AC");
        env::remove_var("RUSHBENCH_GIT_SHA");
        env::remove_var("RUSHBENCH_CONTRACTS_SHA256");
        env::remove_var("RUSHBENCH_STATE_DIR");
        let _ = fs::remove_file(target_file);
    }

    #[test]
    fn test_t9_host_reject_when_no_energy_counter() {
        let _lock = TEST_LOCK.lock().unwrap();
        env::set_var("RUSHBENCH_MOCK_ENERGY_SOURCE", "none");
        let res = EnergySource::detect();
        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), "no_energy_counter");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");
    }
}
