//! testos-runner — runs INSIDE testOS after boot.
//!
//! Responsibilities:
//! 1. Show a menu: "Run all" or pick individual benchmarks from the list.
//! 2. For each selected benchmark: print a banner, run it, capture stdout/stderr/exit,
//!    write one JSON result file to the results directory on the USB stick.
//! 3. Show progress with per-test ETA.
//! 4. Honor Esc (read from /dev/console) to abort the run early — partial results saved.
//! 5. Write a top-level RunManifest.json when done (or aborted).
//! 6. Reboot back to the host OS.
//!
//! Where to find things after boot:
//! - The USB stick is mounted at /run/testos/usb (mounted by the testOS initrd/init script).
//! - Bench list TOML is at /run/testos/usb/testos/bench-list.toml (copied at build time).
//! - Results directory is /run/testos/usb/testos-results/<UTC-timestamp>/.
//!
//! The runner itself is installed at /usr/bin/testos-runner inside the testOS image.

use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use testos::{
    Bench, BenchKind, BenchList, BenchResult, HostFingerprint, RunManifest, SCHEMA_VERSION,
};

const USB_MOUNT: &str = "/run/testos/usb";
const RESULTS_SUBDIR: &str = "testos-results";
const BENCH_LIST_REL: &str = "testos/bench-list.toml";
const TESTOS_VERSION_FALLBACK: &str = "0.7.0-beta.1";

fn main() {
    println!();
    println!("════════════════════════════════════════════════════");
    println!("  testOS — Rush Linux benchmark environment");
    println!("════════════════════════════════════════════════════");
    println!();

    // 1. Verify USB mount exists.
    let usb = Path::new(USB_MOUNT);
    if !usb.exists() {
        eprintln!("ERROR: USB mount point {} does not exist.", USB_MOUNT);
        eprintln!("       The testOS initrd should have created it. Boot may be incomplete.");
        eprintln!("       Aborting.");
        std::process::exit(1);
    }

    // 2. Load bench list from the USB.
    let bench_list_path = usb.join(BENCH_LIST_REL);
    let list = match BenchList::load(&bench_list_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "ERROR: cannot load bench list from {}: {}",
                bench_list_path.display(),
                e
            );
            std::process::exit(1);
        }
    };

    println!(
        "Loaded {} benchmarks from catalog v{}.",
        list.benches.len(),
        list.version
    );
    println!("USB mounted at: {}", USB_MOUNT);
    println!();

    // 3. Capture host fingerprint once per run.
    let host = HostFingerprint::capture();
    println!("Host fingerprint: {}", host.fingerprint);
    println!("  CPU:    {}", host.cpu_model);
    println!("  Board:  {}", host.dmi_board);
    println!("  Kernel: {}", host.kernel);
    println!();

    // 4. Show menu.
    let selection = match show_menu(&list) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Menu error: {}", e);
            std::process::exit(1);
        }
    };

    if selection.is_empty() {
        println!("Nothing selected. Rebooting back to host OS.");
        reboot_host();
        return;
    }

    // 5. Create results directory on the USB.
    let started_at = iso_utc_now();
    let results_dir = usb.join(RESULTS_SUBDIR).join(started_at.replace(':', "-"));
    if let Err(e) = std::fs::create_dir_all(&results_dir) {
        eprintln!(
            "ERROR: cannot create results dir {}: {}",
            results_dir.display(),
            e
        );
        std::process::exit(1);
    }

    let mode = if selection.len() == list.benches.len() {
        "all".to_string()
    } else {
        format!(
            "selection:{}",
            selection
                .iter()
                .map(|b| b.id.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    };

    println!();
    println!("Results will be written to: {}", results_dir.display());
    println!("Mode: {}", mode);
    println!();

    // 6. Start the Esc watcher thread.
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        watch_for_esc(tx);
    });

    // 7. Run each selected benchmark.
    let mut attempted = Vec::new();
    let mut passed = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = Vec::new();

    let total = selection.len();
    for (idx, bench) in selection.iter().enumerate() {
        // Check for abort signal.
        if rx.try_recv().is_ok() {
            println!();
            println!("!! Aborted by user (Esc pressed). Saving partial results. !!");
            for remaining in &selection[idx..] {
                skipped.push(remaining.id.clone());
            }
            break;
        }

        let progress = format!("[{}/{}] ", idx + 1, total);
        let eta = BenchList::format_duration(bench.estimated_seconds);
        println!("{}", &progress);
        println!("{}— {} ({})", "  ".to_string() + &progress, bench.name, eta);

        if bench.requires_battery && host.battery_design_uwh == 0 {
            println!("   SKIPPED: requires battery but no battery present.");
            skipped.push(bench.id.clone());
            attempted.push(bench.id.clone());
            continue;
        }

        if bench.requires_battery {
            println!("   This benchmark requires battery power.");
            println!("   Please unplug the AC adapter now.");
            print!("   Press Enter when ready (or 's' to skip): ");
            io::stdout().flush().unwrap();
            let mut line = String::new();
            io::stdin().read_line(&mut line).ok();
            if line.trim() == "s" {
                println!("   Skipped by user.");
                skipped.push(bench.id.clone());
                attempted.push(bench.id.clone());
                continue;
            }
        }

        let started = Instant::now();
        let started_iso = iso_utc_now();
        let (status, value, unit, stdout, stderr, exit_code) = run_benchmark(bench, &results_dir);
        let elapsed = started.elapsed().as_secs_f64();
        let finished_iso = iso_utc_now();

        let result = BenchResult {
            schema_version: SCHEMA_VERSION,
            bench_id: bench.id.clone(),
            bench_name: bench.name.clone(),
            scenario: bench.scenario.clone(),
            status: status.clone(),
            started_at: started_iso,
            finished_at: finished_iso,
            elapsed_seconds: elapsed,
            value,
            unit: unit.clone(),
            stdout: Some(BenchResult::stdout_truncated(&stdout)),
            stderr: Some(BenchResult::stdout_truncated(&stderr)),
            exit_code,
            host: host.clone(),
        };

        let result_path = results_dir.join(format!("{}.json", bench.id));
        match serde_json::to_string_pretty(&result) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&result_path, json) {
                    eprintln!(
                        "   WARNING: failed to write result to {}: {}",
                        result_path.display(),
                        e
                    );
                }
            }
            Err(e) => eprintln!("   WARNING: failed to serialize result: {}", e),
        }

        attempted.push(bench.id.clone());
        match status.as_str() {
            "pass" => {
                passed.push(bench.id.clone());
                let val_str = match (&value, &unit) {
                    (Some(v), Some(u)) => format!(" — {} {}", v, u),
                    _ => String::new(),
                };
                println!(
                    "   PASS{} ({})",
                    val_str,
                    BenchList::format_duration(elapsed as u64)
                );
            }
            "fail" => {
                failed.push(bench.id.clone());
                println!("   FAIL ({})", BenchList::format_duration(elapsed as u64));
            }
            _ => {
                skipped.push(bench.id.clone());
                println!(
                    "   SKIPPED ({})",
                    BenchList::format_duration(elapsed as u64)
                );
            }
        }
        println!();
        // Sync the USB filesystem so we don't lose results on a sudden reboot.
        let _ = Command::new("sync").status();
    }

    // 8. Write the top-level manifest.
    let finished_at = iso_utc_now();
    let testos_version = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines().find(|l| l.starts_with("VERSION=")).and_then(|l| {
                l.split('=')
                    .nth(1)
                    .map(|s| s.trim().trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| TESTOS_VERSION_FALLBACK.to_string());

    let manifest = RunManifest {
        schema_version: SCHEMA_VERSION,
        started_at: started_at.clone(),
        finished_at,
        mode,
        attempted,
        passed,
        failed,
        skipped,
        host: host.clone(),
        testos_version,
    };

    let manifest_path = results_dir.join("manifest.json");
    match serde_json::to_string_pretty(&manifest) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&manifest_path, json) {
                eprintln!(
                    "WARNING: failed to write manifest to {}: {}",
                    manifest_path.display(),
                    e
                );
            }
        }
        Err(e) => eprintln!("WARNING: failed to serialize manifest: {}", e),
    }

    // Capture system-level logs into the results directory for post-mortem
    // analysis. These are invaluable for debugging boot failures, hardware
    // detection issues, and benchmark crashes. We capture:
    //   - dmesg.txt: kernel ring buffer (hardware detection, driver errors)
    //   - journal.txt: full systemd journal from this boot (service failures,
    //     login events, mount issues)
    //   - uname.txt: kernel version, architecture, hostname
    //   - cpuinfo.txt: CPU model, flags, core count (helps interpret results)
    //   - meminfo.txt: memory size, swap, hugepages
    //   - cmdline.txt: kernel command line (verifies testos.* params applied)
    println!("  Capturing system logs...");
    let logs_dir = results_dir.join("system-logs");
    let _ = std::fs::create_dir_all(&logs_dir);

    let captures: [(&str, &str); 9] = [
        ("dmesg.txt", "dmesg"),
        ("journal.txt", "journalctl -b --no-pager -o cat"),
        ("uname.txt", "uname -a"),
        ("cpuinfo.txt", "cat /proc/cpuinfo"),
        ("meminfo.txt", "cat /proc/meminfo"),
        ("cmdline.txt", "cat /proc/cmdline"),
        (
            "lsblk.txt",
            "lsblk -o NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,LABEL",
        ),
        (
            "lspci.txt",
            "lspci -nn 2>/dev/null || echo 'lspci not installed'",
        ),
        (
            "lsusb.txt",
            "lsusb 2>/dev/null || echo 'lsusb not installed'",
        ),
    ];
    for (filename, cmd) in captures.iter() {
        let content = match Command::new("bash").arg("-c").arg(*cmd).output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
            Err(e) => format!("(capture failed: {})", e),
        };
        let _ = std::fs::write(logs_dir.join(filename), content);
    }

    let _ = Command::new("sync").status();

    println!("════════════════════════════════════════════════════");
    println!("  Run complete");
    println!(
        "  Passed: {}   Failed: {}   Skipped: {}",
        manifest.passed.len(),
        manifest.failed.len(),
        manifest.skipped.len()
    );
    println!("  Results: {}", results_dir.display());
    println!();
    println!("  Syncing USB... ");
    let _ = Command::new("sync").status();
    println!("  Done. Unplug the USB if you like.");
    println!();
    println!("  Rebooting back to host OS in 5 seconds...");
    println!("  (Ctrl-C to stay in testOS shell)");
    println!("════════════════════════════════════════════════════");

    std::thread::sleep(std::time::Duration::from_secs(5));
    reboot_host();
}

/// Show the menu and return the list of selected benchmarks.
fn show_menu(list: &BenchList) -> Result<Vec<Bench>, String> {
    let total_eta = BenchList::format_duration(list.total_estimated_seconds());
    println!("Available benchmarks:");
    println!("  [0] Run all (estimated {})", total_eta);
    for (i, b) in list.benches.iter().enumerate() {
        let eta = BenchList::format_duration(b.estimated_seconds);
        let bat = if b.requires_battery { " [battery]" } else { "" };
        println!("  [{}] {} ({}){}", i + 1, b.name, eta, bat);
    }
    println!();

    loop {
        print!("Select (comma-separated numbers, or 0 for all, or 'q' to quit): ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let line = line.trim();

        if line == "q" {
            return Ok(Vec::new());
        }
        if line == "0" {
            return Ok(list.benches.clone());
        }

        // Parse comma-separated indices.
        let mut picked: Vec<Bench> = Vec::new();
        let mut bad = false;
        for tok in line.split(',') {
            let tok = tok.trim();
            if tok.is_empty() {
                continue;
            }
            match tok.parse::<usize>() {
                Ok(n) if n >= 1 && n <= list.benches.len() => {
                    let b = list.benches[n - 1].clone();
                    if !picked.iter().any(|x| x.id == b.id) {
                        picked.push(b);
                    }
                }
                _ => {
                    bad = true;
                    println!("  '{}' is not a valid selection.", tok);
                    break;
                }
            }
        }
        if !bad && !picked.is_empty() {
            return Ok(picked);
        }
        if !bad && picked.is_empty() {
            println!("  No valid selections. Try again.");
        }
    }
}

/// Run a single benchmark and return (status, value, unit, stdout, stderr, exit_code).
fn run_benchmark(
    bench: &Bench,
    results_dir: &Path,
) -> (
    String,
    Option<f64>,
    Option<String>,
    String,
    String,
    Option<i32>,
) {
    // For ShellJson we pass the result file path via env.
    let result_file = results_dir.join(format!("{}.out.json", bench.id));
    let _ = std::fs::remove_file(&result_file);

    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(&bench.command);
    cmd.env("TESTOS_RESULT_FILE", &result_file);
    cmd.env("TESTOS_BENCH_ID", &bench.id);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return (
                "fail".to_string(),
                None,
                None,
                String::new(),
                format!("Failed to spawn benchmark: {}", e),
                None,
            );
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();

    if !output.status.success() && exit_code.is_none() {
        // Killed by signal.
        return ("fail".to_string(), None, None, stdout, stderr, exit_code);
    }

    match bench.kind {
        BenchKind::ShellPassFail => {
            if output.status.success() {
                ("pass".to_string(), None, None, stdout, stderr, exit_code)
            } else {
                ("fail".to_string(), None, None, stdout, stderr, exit_code)
            }
        }
        BenchKind::ShellNumeric => {
            // Last non-empty line of stdout is the number.
            let num = stdout
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .and_then(|l| l.trim().parse::<f64>().ok());
            match num {
                Some(v) => (
                    "pass".to_string(),
                    Some(v),
                    Some("numeric".to_string()),
                    stdout,
                    stderr,
                    exit_code,
                ),
                None => ("fail".to_string(), None, None, stdout, stderr, exit_code),
            }
        }
        BenchKind::ShellJson => {
            // Read the result file the command was supposed to write.
            let text = match std::fs::read_to_string(&result_file) {
                Ok(t) => t,
                Err(_) => return ("fail".to_string(), None, None, stdout, stderr, exit_code),
            };
            // Expect JSON like {"value": 1234.5, "unit": "iops"}
            #[derive(serde::Deserialize)]
            struct Out {
                value: f64,
                unit: String,
            }
            match serde_json::from_str::<Out>(&text) {
                Ok(o) => (
                    "pass".to_string(),
                    Some(o.value),
                    Some(o.unit),
                    stdout,
                    stderr,
                    exit_code,
                ),
                Err(_) => ("fail".to_string(), None, None, stdout, stderr, exit_code),
            }
        }
        BenchKind::Rushbench => {
            // rushbench writes its own JSON. We just check exit code and look for
            // the median in stdout (best-effort). The rushbench result path is
            // not used here — we just treat the median as the value.
            // This is a simple wrapper; full integration can come later.
            if output.status.success() {
                // Try to extract "median: 1.23" from stdout.
                let med = stdout.lines().find_map(|l| {
                    let l = l.trim();
                    if l.starts_with("median:") {
                        l.trim_start_matches("median:").trim().parse::<f64>().ok()
                    } else {
                        None
                    }
                });
                (
                    "pass".to_string(),
                    med,
                    Some("ms".to_string()),
                    stdout,
                    stderr,
                    exit_code,
                )
            } else {
                ("fail".to_string(), None, None, stdout, stderr, exit_code)
            }
        }
    }
}

/// Watch /dev/console for Esc to abort the run. Best-effort — if this fails
/// to read the console, the user can still Ctrl-C the runner.
fn watch_for_esc(tx: std::sync::mpsc::Sender<()>) {
    use std::os::unix::io::AsRawFd;
    let path = "/dev/console";
    let f = match std::fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(_) => return, // silent — common in non-tty environments
    };
    let fd = f.as_raw_fd();
    let mut buf = [0u8; 1];
    loop {
        // Read one byte; this blocks. We don't care about most keys.
        let n = unsafe { libc_read(fd, buf.as_mut_ptr(), 1) };
        if n <= 0 {
            // Read error or EOF; bail.
            return;
        }
        if buf[0] == 0x1b {
            // Esc.
            let _ = tx.send(());
            return;
        }
    }
}

// Direct libc binding to avoid pulling in the `libc` crate.
extern "C" {
    fn read(fd: i32, buf: *mut std::os::raw::c_void, count: usize) -> isize;
}
unsafe fn libc_read(fd: i32, buf: *mut u8, count: usize) -> isize {
    read(fd, buf as *mut _, count)
}

fn iso_utc_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now();
    let secs = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Simple ISO 8601 from epoch seconds, no chrono dependency needed in this binary.
    // Format: YYYY-MM-DDTHH:MM:SSZ
    iso_from_epoch(secs)
}

fn iso_from_epoch(epoch: u64) -> String {
    // Days since epoch, then calendar breakdown. Naive but correct for 1970-2099.
    let secs_per_day: u64 = 86400;
    let days = epoch / secs_per_day;
    let secs_in_day = epoch % secs_per_day;
    let hour = secs_in_day / 3600;
    let min = (secs_in_day % 3600) / 60;
    let sec = secs_in_day % 60;

    // Convert days to year/month/day. Algorithm from Howard Hinnant's date library.
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hour, min, sec
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn reboot_host() {
    // Try `systemctl reboot` first; fall back to `reboot` then direct syscall.
    let _ = Command::new("systemctl").arg("reboot").status();
    let _ = Command::new("reboot").status();
    // If we get here, reboot failed. Try the syscall.
    unsafe { libc_reboot(LINUX_REBOOT_CMD_RESTART) };
    // Last resort.
    eprintln!("Failed to reboot. Type 'reboot' manually.");
}

const LINUX_REBOOT_CMD_RESTART: i32 = 0x01234567;
extern "C" {
    fn reboot(magic: i32, magic2: i32, cmd: i32, arg: *mut std::os::raw::c_void) -> i32;
}
unsafe fn libc_reboot(cmd: i32) -> i32 {
    reboot(0xfee1deadu32 as i32, 672274793, cmd, std::ptr::null_mut())
}

// Unused imports kept out of the binary — removed to avoid trait-call errors.
// (The runner doesn't currently use BufRead::read_line as a method reference;
// we just call .read_line() on stdin directly elsewhere.)
