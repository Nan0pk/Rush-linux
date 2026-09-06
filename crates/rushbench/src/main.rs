use std::env;

pub mod contracts;
pub mod energy;
pub mod pair_plan;
pub mod preset;
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
    println!("  rushbench run preset=mixed-load-001 --tag=<lever>-<hostname> [--cycles <n>] [--out <dir>] [--ac-ok]");
    println!("  rushbench pair-plan --pairs <n> --seed <u64> [--out <file>]");
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
            let mut preset: Option<String> = None;
            let mut tag: Option<String> = None;
            let mut out: Option<String> = None;
            let mut cycles = preset::REQUIRED_CYCLES;

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
                    "--cycles" => {
                        if i + 1 < args.len() {
                            cycles = match args[i + 1].parse() {
                                Ok(value) => value,
                                Err(_) => {
                                    eprintln!("Error: --cycles must be an integer");
                                    std::process::exit(1);
                                }
                            };
                            i += 2;
                        } else {
                            eprintln!("Error: --cycles requires an argument");
                            std::process::exit(1);
                        }
                    }
                    other if other.starts_with("preset=") => {
                        preset = Some(other["preset=".len()..].to_string());
                        i += 1;
                    }
                    other if other.starts_with("--tag=") => {
                        tag = Some(other["--tag=".len()..].to_string());
                        i += 1;
                    }
                    "--tag" => {
                        if i + 1 < args.len() {
                            tag = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            eprintln!("Error: --tag requires an argument");
                            std::process::exit(1);
                        }
                    }
                    other if other.starts_with("--out=") => {
                        out = Some(other["--out=".len()..].to_string());
                        i += 1;
                    }
                    "--out" => {
                        if i + 1 < args.len() {
                            out = Some(args[i + 1].clone());
                            i += 2;
                        } else {
                            eprintln!("Error: --out requires an argument");
                            std::process::exit(1);
                        }
                    }
                    _ => {
                        eprintln!("Error: unknown argument {}", args[i]);
                        print_usage();
                        std::process::exit(1);
                    }
                }
            }

            if let Some(preset_name) = preset {
                let tag = match tag {
                    Some(t) => t,
                    None => {
                        eprintln!("Error: preset runs require --tag=<lever>-<hostname>");
                        std::process::exit(1);
                    }
                };
                let out_dir = match out {
                    Some(dir) => std::path::PathBuf::from(dir),
                    None => {
                        eprintln!("Error: preset runs require --out=<evidence arm directory>");
                        std::process::exit(1);
                    }
                };
                if let Err(e) = preset::run_preset(&preset_name, cycles, &tag, &out_dir, ac_ok) {
                    eprintln!("Preset run failed: {}", e);
                    std::process::exit(1);
                }
                return;
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
        "pair-plan" => {
            let mut pairs: Option<usize> = None;
            let mut seed: Option<u64> = None;
            let mut out: Option<std::path::PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--pairs" => {
                        let value = args.get(i + 1).unwrap_or_else(|| {
                            eprintln!("Error: --pairs requires an argument");
                            std::process::exit(1);
                        });
                        pairs = match value.parse::<usize>() {
                            Ok(value) if value > 0 => Some(value),
                            _ => {
                                eprintln!("Error: --pairs must be at least 1");
                                std::process::exit(1);
                            }
                        };
                        i += 2;
                    }
                    "--seed" => {
                        let value = args.get(i + 1).unwrap_or_else(|| {
                            eprintln!("Error: --seed requires an argument");
                            std::process::exit(1);
                        });
                        seed = match value.parse::<u64>() {
                            Ok(value) => Some(value),
                            Err(_) => {
                                eprintln!("Error: --seed must be an unsigned integer");
                                std::process::exit(1);
                            }
                        };
                        i += 2;
                    }
                    "--out" => {
                        let value = args.get(i + 1).unwrap_or_else(|| {
                            eprintln!("Error: --out requires an argument");
                            std::process::exit(1);
                        });
                        out = Some(std::path::PathBuf::from(value));
                        i += 2;
                    }
                    unknown => {
                        eprintln!("Error: unknown pair-plan argument {unknown}");
                        print_usage();
                        std::process::exit(1);
                    }
                }
            }

            let pairs = pairs.unwrap_or_else(|| {
                eprintln!("Error: pair-plan requires --pairs <n>");
                std::process::exit(1);
            });
            let seed = seed.unwrap_or_else(|| {
                eprintln!("Error: pair-plan requires --seed <u64>");
                std::process::exit(1);
            });
            let plan = pair_plan::build_pair_plan(pairs, seed).unwrap_or_else(|error| {
                eprintln!("Error: {error}");
                std::process::exit(1);
            });
            let json = serde_json::to_string_pretty(&plan).unwrap_or_else(|error| {
                eprintln!("Error: failed to serialize pair plan: {error}");
                std::process::exit(1);
            });

            if let Some(path) = out {
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    if let Err(error) = std::fs::create_dir_all(parent) {
                        eprintln!("Error: failed to create pair-plan directory: {error}");
                        std::process::exit(1);
                    }
                }
                if let Err(error) = std::fs::write(&path, format!("{json}\n")) {
                    eprintln!("Error: failed to write pair plan: {error}");
                    std::process::exit(1);
                }
                println!("Wrote pair plan to {}", path.display());
            } else {
                println!("{json}");
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

// --- battery-counter window guards ---

#[cfg(test)]
mod counter_guard_tests {
    use crate::energy::{calculate_window, EnergySample, EnergySource};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn battery() -> EnergySource {
        EnergySource::Battery(PathBuf::from("/sys/class/power_supply/BAT1/energy_now"))
    }

    fn window(start_j: f64, end_j: f64, secs: u64, on_ac: Option<bool>) -> Result<f64, String> {
        let now = Instant::now();
        let start = EnergySample {
            time: now,
            joules: start_j,
            on_ac,
        };
        let end = EnergySample {
            time: now + Duration::from_secs(secs),
            joules: end_j,
            on_ac,
        };
        calculate_window(&battery(), &start, &end).map(|info| info.avg_watts)
    }

    /// A counter pinned at full charge reports no change; averaging that in as
    /// 0 W is how a run started at 100% produced two silent minutes.
    #[test]
    fn a_battery_window_that_did_not_move_is_refused() {
        let err = window(100_000.0, 100_000.0, 60, Some(false)).expect_err("must refuse");
        assert_eq!(err, "battery_counter_did_not_move");
    }

    /// And the step that follows must not be believed either: 188 W is not a
    /// figure this class of machine can draw.
    #[test]
    fn an_implausible_step_is_refused() {
        let err = window(100_000.0, 88_696.0, 60, Some(false)).expect_err("must refuse");
        assert!(err.starts_with("counter_step_artifact"), "{err}");
        assert!(err.contains("188"), "{err}");
    }

    #[test]
    fn a_plausible_discharge_is_accepted() {
        let watts = window(100_000.0, 97_000.0, 60, Some(false)).expect("must accept");
        assert!((watts - 50.0).abs() < 0.5, "{watts}");
    }

    /// On AC the same counter legitimately does not move, and the preset already
    /// marks energy unsupported there — so this path must not add a second,
    /// different refusal.
    #[test]
    fn an_unmoved_counter_on_ac_is_not_treated_as_a_battery_artifact() {
        let watts = window(100_000.0, 100_000.0, 60, Some(true)).expect("must accept");
        assert_eq!(watts, 0.0);
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
    use types::{EnergyInfo, HostInfo, ResolvedFloors, RunRecord, RushInfo};
    use utils::{
        get_battery_design_uwh, get_contracts_sha256, get_cpu_model, get_dmi_board, get_git_sha,
        get_host_folder_name, get_kernel_version, get_utc_timestamp,
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
    fn test_t9_avg_watts_positive() {
        // Use mock samples where energy decreases, ensuring avg_watts > 0
        let mock_source = EnergySource::Battery(PathBuf::from("/mock/battery"));
        let start = EnergySample {
            time: Instant::now(),
            joules: 200.0,
            on_ac: Some(false),
        };
        let end = EnergySample {
            time: start.time + Duration::from_secs(10),
            joules: 100.0,
            on_ac: Some(false),
        };
        let info = calculate_window(&mock_source, &start, &end).expect("Window calculation failed");
        assert!(info.avg_watts > 0.0, "avg_watts should be positive");
    }

    #[test]
    fn test_t3_class_readback_enforcement() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        // Read from temp_dir (RUSHBENCH_STATE_DIR), not the repo benchmarks/ path
        let date = get_utc_timestamp().split('T').next().unwrap().to_string();
        let host_folder = get_host_folder_name();
        let target_file = temp_dir
            .join(date)
            .join(host_folder)
            .join("interactive")
            .join("psi-cpu.json");
        assert!(
            target_file.exists(),
            "Result file not found at {:?}",
            target_file
        );

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
    fn test_t6_real_energy_extension_does_not_bloat_samples() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_root = env::temp_dir().join(format!("rushbench_t6_{suffix}"));
        let temp_sysfs = temp_root.join("sysfs");
        let temp_state = temp_root.join("state");
        let rapl_dir = temp_sysfs.join("sys/class/powercap/intel-rapl:0");
        fs::create_dir_all(&rapl_dir).unwrap();
        fs::create_dir_all(&temp_state).unwrap();
        fs::write(rapl_dir.join("energy_uj"), "1000000").unwrap();

        env::set_var("RUSHBENCH_SYSFS_ROOT", &temp_sysfs);
        env::set_var("RUSHBENCH_STATE_DIR", &temp_state);
        env::set_var("RUSHBENCH_MIN_ENERGY_WINDOW_SEC", "0.05");
        env::set_var("RUSHBENCH_WARMUP_SEC", "0");
        env::set_var("RUSHBENCH_MOCK_METRIC_psi_cpu_avg10", "0");
        env::set_var(
            "RUSHBENCH_OPTCTL_STATUS_JSON",
            r#"{"workload_class": "interactive", "cpu_wakeup_latency": 1000, "device_resume_latency": 10000}"#,
        );
        env::set_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE", "true");
        env::set_var("RUSHBENCH_GIT_SHA", "abcdef012345");
        env::set_var("RUSHBENCH_CONTRACTS_SHA256", "checksum123");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_JOULES");
        env::remove_var("RUSHBENCH_MOCK_ON_AC");

        let res = run_cell("interactive", "psi-cpu", 5, true);
        assert!(res.is_ok(), "run_cell failed: {:?}", res);

        let date = get_utc_timestamp().split('T').next().unwrap().to_string();
        let host_folder = get_host_folder_name();
        let target_file = temp_state
            .join(date)
            .join(host_folder)
            .join("interactive")
            .join("psi-cpu.json");
        let content = fs::read_to_string(&target_file).unwrap();
        let rec: RunRecord = serde_json::from_str(&content).unwrap();
        assert_eq!(rec.n, 5);
        assert_eq!(rec.samples.unwrap().len(), 5);

        env::remove_var("RUSHBENCH_SYSFS_ROOT");
        env::remove_var("RUSHBENCH_STATE_DIR");
        env::remove_var("RUSHBENCH_MIN_ENERGY_WINDOW_SEC");
        env::remove_var("RUSHBENCH_WARMUP_SEC");
        env::remove_var("RUSHBENCH_MOCK_METRIC_psi_cpu_avg10");
        env::remove_var("RUSHBENCH_OPTCTL_STATUS_JSON");
        env::remove_var("RUSHBENCH_OPTCTL_APPLY_OVERRIDE");
        env::remove_var("RUSHBENCH_GIT_SHA");
        env::remove_var("RUSHBENCH_CONTRACTS_SHA256");
        let _ = fs::remove_dir_all(&temp_root);
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
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // High reading forced: 1500 µs exceeds the corrected 1 ms (1000 µs)
        // latency-critical CPU wakeup floor, so the report must surface a
        // budget_violation. The previous 10 µs floor made any non-RT kernel
        // permanently violate the contract; the 1 ms correction restores the
        // honesty path's intent (a real outlier above the floor).
        env::set_var("RUSHBENCH_MOCK_METRIC_cyclictest_max_us", "1500");
        env::set_var(
            "RUSHBENCH_OPTCTL_STATUS_JSON",
            r#"{"workload_class": "latency-critical", "cpu_wakeup_latency": 1000, "device_resume_latency": 1000}"#,
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

        // Capture report output — read from temp_dir (RUSHBENCH_STATE_DIR)
        let date = get_utc_timestamp().split('T').next().unwrap().to_string();
        let host_folder = get_host_folder_name();
        let results_day = temp_dir.join(date).join(host_folder);

        // Run report and verify budget_violation is present
        let target_file = results_day.join("latency-critical").join("cyclictest.json");
        assert!(
            target_file.exists(),
            "Result file not found at {:?}",
            target_file
        );

        // Verify that target_file itself contains the high reading
        let content = fs::read_to_string(&target_file).unwrap();
        assert!(content.contains(r#""median": 1500.0"#));

        let report_res = run_report(temp_dir.to_str().unwrap());
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
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        env::set_var("RUSHBENCH_MOCK_ENERGY_SOURCE", "none");
        let res = EnergySource::detect();
        assert!(res.is_err());
        assert_eq!(res.err().unwrap(), "no_energy_counter");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");
    }

    #[test]
    fn test_t9_energy_detection_priority() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // Setup temp directory
        let rand_dir = format!(
            "rushbench_test_sysfs_{}",
            Instant::now().elapsed().as_nanos()
        );
        let temp_sysfs = env::temp_dir().join(rand_dir);
        fs::create_dir_all(&temp_sysfs).unwrap();

        let mock_bat_dir = temp_sysfs.join("sys/class/power_supply/BAT0");
        let mock_rapl_dir = temp_sysfs.join("sys/class/powercap/intel-rapl:0");

        env::set_var("RUSHBENCH_SYSFS_ROOT", &temp_sysfs);
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");

        // Scenario A: Only battery is present
        fs::create_dir_all(&mock_bat_dir).unwrap();
        let bat_file = mock_bat_dir.join("energy_now");
        fs::write(&bat_file, "1000").unwrap();

        let source = EnergySource::detect().expect("Should detect battery");
        assert!(matches!(source, EnergySource::Battery(_)));

        // Scenario B: Both battery and RAPL are present, and RAPL is readable
        fs::create_dir_all(&mock_rapl_dir).unwrap();
        let rapl_file = mock_rapl_dir.join("energy_uj");
        fs::write(&rapl_file, "2000").unwrap();

        let source = EnergySource::detect().expect("Should detect RAPL");
        assert!(matches!(source, EnergySource::Rapl(_)));

        // Scenario C: Both battery and RAPL are present, but RAPL cannot be read.
        // Use a directory at the RAPL path — read_to_string always fails on a directory,
        // even for root (which can bypass 0o000 permission restrictions).
        fs::remove_file(&rapl_file).unwrap();
        fs::create_dir(&rapl_file).unwrap();

        let source =
            EnergySource::detect().expect("Should fall back to battery when RAPL is unreadable");
        assert!(matches!(source, EnergySource::Battery(_)));

        // Restore RAPL as a readable file for Scenario D
        fs::remove_dir(&rapl_file).unwrap();
        fs::write(&rapl_file, "2000").unwrap();

        // Scenario D: Only RAPL is present (battery removed)
        fs::remove_file(&bat_file).unwrap();
        let source = EnergySource::detect();
        assert!(source.is_ok());
        assert!(matches!(source.unwrap(), EnergySource::Rapl(_)));

        // Scenario E: No energy counter at all
        fs::remove_file(&rapl_file).unwrap();
        let source = EnergySource::detect();
        assert!(source.is_err());
        assert_eq!(source.err().unwrap(), "no_energy_counter");

        // Clean up
        env::remove_var("RUSHBENCH_SYSFS_ROOT");
        let _ = fs::remove_dir_all(&temp_sysfs);
    }

    #[test]
    fn test_t10_real_energy_advance() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Clear any mock env vars just in case they are set by other tests
        env::remove_var("RUSHBENCH_MOCK_ENERGY_SOURCE");
        env::remove_var("RUSHBENCH_MOCK_ENERGY_JOULES");
        env::remove_var("RUSHBENCH_MOCK_ON_AC");

        let source = match EnergySource::detect() {
            Ok(s) => s,
            Err(_) => {
                println!("test_t10: SKIP — no energy counter on this host (CI container).");
                return;
            }
        };
        let start = source.sample().expect("Failed to sample start energy");
        println!("test_t10: start = {:?}", start);

        if matches!(source, EnergySource::Battery(_)) && start.on_ac == Some(true) {
            println!("test_t10: SKIP — battery source on AC power (cannot measure discharging).");
            return;
        }

        let start_time = Instant::now();
        let mut counter_advanced = false;
        let mut end = start.clone();

        // Burn CPU for up to 45 seconds or until the energy reading changes
        while start_time.elapsed() < Duration::from_secs(45) {
            let mut x = 0;
            for idx in 0..1_000_000 {
                x ^= idx;
            }
            std::hint::black_box(x);

            if let Ok(sample) = source.sample() {
                if sample.joules != start.joules {
                    end = sample;
                    counter_advanced = true;
                    break;
                }
            }
        }

        if !counter_advanced {
            // The energy counter never changed in 45 s.
            // This happens when there is no battery/RAPL available (AC-only, CI container,
            // or a battery controller with very coarse resolution). Skip — do NOT claim
            // the energy-advance logic has been verified on this host.
            println!(
                "test_t10: SKIP — energy counter did not advance in 45 s (no real battery/RAPL)."
            );
            return;
        }

        // Counter advanced: now we can assert real energy semantics.
        let info = calculate_window(&source, &start, &end)
            .expect("calculate_window failed after counter advanced");
        println!("Real energy test window info: {:?}", info);
        assert!(
            info.avg_watts > 0.0,
            "avg_watts should be > 0 when counter advanced: {:?}",
            info
        );
        assert!(
            info.window_joules > 0.0,
            "window_joules should be > 0 when counter advanced: {:?}",
            info
        );
    }

    #[test]
    fn test_report_energy_analysis_workload_filter() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let rand_dir = format!(
            "rushbench_test_report_{}",
            Instant::now().elapsed().as_nanos()
        );
        let temp_dir = env::temp_dir().join(rand_dir);
        let date_folder = get_utc_timestamp().split('T').next().unwrap().to_string();
        let host_folder = get_host_folder_name();

        let results_day = temp_dir.join(&date_folder).join(&host_folder);
        fs::create_dir_all(results_day.join("idle")).unwrap();
        fs::create_dir_all(results_day.join("interactive")).unwrap();

        // 1. Write an idle record with workload "cyclictest" (avg_watts = 2.5)
        let record_idle_cyclictest = RunRecord {
            schema_version: 1,
            host: HostInfo {
                kernel: "test-kernel".to_string(),
                cpu_model: "test-cpu".to_string(),
                dmi_board: "test-board".to_string(),
                battery_design_uwh: 1000000,
            },
            rush: RushInfo {
                optid_sha: "test-sha".to_string(),
                contracts_sha256: "test-checksum".to_string(),
                rig_sha: "test-sha".to_string(),
                rig_version: "0.1.0".to_string(),
            },
            class_requested: "idle".to_string(),
            class_observed: "idle".to_string(),
            resolved_floors: ResolvedFloors {
                cpu_wakeup_latency_us: 1000,
                device_resume_latency_us: 10000,
            },
            power_source: "battery".to_string(),
            workload: "cyclictest".to_string(),
            metric: "cyclictest-max-us".to_string(),
            n: 5,
            samples: Some(vec![10, 10, 10, 10, 10]),
            median: Some(10.0),
            p95: Some(10.0),
            iqr: Some(0.0),
            energy: Some(EnergyInfo {
                window_joules: 75.0,
                avg_watts: 2.5,
                counter: "BAT0/energy_now".to_string(),
            }),
            started_at: "2026-06-14T09:00:00Z".to_string(),
            warmup_runs: 2,
            anomalies: vec![],
        };

        // 2. Write an interactive record with workload "cyclictest" (avg_watts = 5.0)
        let record_interactive_cyclictest = RunRecord {
            class_requested: "interactive".to_string(),
            class_observed: "interactive".to_string(),
            workload: "cyclictest".to_string(),
            metric: "cyclictest-max-us".to_string(),
            energy: Some(EnergyInfo {
                window_joules: 150.0,
                avg_watts: 5.0,
                counter: "BAT0/energy_now".to_string(),
            }),
            ..record_idle_cyclictest.clone()
        };

        // Write both records
        fs::write(
            results_day.join("idle/cyclictest.json"),
            serde_json::to_string_pretty(&record_idle_cyclictest).unwrap(),
        )
        .unwrap();

        fs::write(
            results_day.join("interactive/cyclictest.json"),
            serde_json::to_string_pretty(&record_interactive_cyclictest).unwrap(),
        )
        .unwrap();

        // Run report and verify that we see the comparative energy analysis
        let report = run_report(temp_dir.to_str().unwrap()).unwrap();
        assert!(
            report.contains("Idle average power draw: 2.50 W"),
            "Report missing correct idle power. Report:\n{}",
            report
        );
        assert!(
            report.contains("Interactive average power draw: 5.00 W"),
            "Report missing correct interactive power. Report:\n{}",
            report
        );
        assert!(
            report.contains("Idle power draw is less than interactive power draw"),
            "Report missing expected comparison text. Report:\n{}",
            report
        );

        // 3. Write an idle record with a non-matching workload ("foreground-launch", avg_watts = 1.0)
        // This should NOT override the 2.5 W idle power computed from "cyclictest"
        let record_idle_launch = RunRecord {
            workload: "foreground-launch".to_string(),
            metric: "foreground-launch-ms".to_string(),
            energy: Some(EnergyInfo {
                window_joules: 30.0,
                avg_watts: 1.0,
                counter: "BAT0/energy_now".to_string(),
            }),
            ..record_idle_cyclictest.clone()
        };
        fs::create_dir_all(results_day.join("idle_launch")).unwrap();
        fs::write(
            results_day.join("idle_launch/foreground-launch.json"),
            serde_json::to_string_pretty(&record_idle_launch).unwrap(),
        )
        .unwrap();

        let report_after = run_report(temp_dir.to_str().unwrap()).unwrap();
        // The idle power must remain 2.5 W (from cyclictest), not 1.0 W (from foreground-launch)
        assert!(
            report_after.contains("Idle average power draw: 2.50 W"),
            "Report idle power was overwritten by incorrect workload. Report:\n{}",
            report_after
        );

        // Cleanup
        fs::remove_dir_all(&temp_dir).unwrap();
    }
}
