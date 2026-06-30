//! testos-launcher — runs on the HOST machine (the dev workstation, not the test box).
//!
//! Responsibilities:
//! 1. `testos-launcher build` — invoke `bash testos/build-testos.sh` to produce the .raw image.
//! 2. `testos-launcher write <device>` — dd the image to a USB device (e.g. /dev/sdX).
//! 3. `testos-launcher preview <device>` — show what's on the USB (bench list, free space).
//! 4. `testos-launcher version` — print the testOS version.
//!
//! The launcher does NOT trigger any reboot — the user manually reboots the test
//! machine from the USB. This keeps the host OS untouched (your "minimal intrusion"
//! requirement).
//!
//! Safety: `write` refuses to operate on a device that looks like a system disk
//! (mounted, in use, or matching the host's root device).

use std::path::{Path, PathBuf};
use std::process::Command;

const IMAGE_PATH: &str = "build/testos.raw";
const BUILD_SCRIPT: &str = "testos/build-testos.sh";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    match args[1].as_str() {
        "build" => cmd_build(&args[2..]),
        "write" => cmd_write(&args[2..]),
        "preview" => cmd_preview(&args[2..]),
        "version" => cmd_version(),
        "--help" | "-h" | "help" => {
            print_usage();
        }
        other => {
            eprintln!("Unknown command: {}", other);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!("testOS launcher — build and write the bootable benchmark USB.");
    println!();
    println!("Usage:");
    println!("  testos-launcher build [--clean]            Build the testOS .raw image.");
    println!("  testos-launcher write /dev/sdX            Write the image to USB device /dev/sdX.");
    println!("  testos-launcher preview /dev/sdX          Show what's on the USB (bench list, free space).");
    println!("  testos-launcher version                   Print testOS version.");
    println!();
    println!("Typical flow:");
    println!("  1. testos-launcher build");
    println!("  2. testos-launcher write /dev/sdX   (find your USB with `lsblk`)");
    println!("  3. Plug USB into test machine, reboot, pick USB from boot menu.");
    println!("  4. testOS boots, runs benchmarks, writes results to the USB.");
    println!("  5. Reboot test machine back to its host OS.");
    println!(
        "  6. testos-ingest pull /dev/sdX       (pulls and formats results, commits to repo)."
    );
}

fn cmd_build(args: &[String]) {
    let mut script_args = Vec::new();
    for a in args {
        match a.as_str() {
            "--clean" | "-c" => script_args.push("--clean"),
            other => {
                eprintln!("Unknown build argument: {}", other);
                std::process::exit(1);
            }
        }
    }

    let script_path = PathBuf::from(BUILD_SCRIPT);
    if !script_path.exists() {
        eprintln!("ERROR: build script not found at {}", script_path.display());
        eprintln!("       Run this from the Rush-linux repo root.");
        std::process::exit(1);
    }

    println!("Building testOS image...");
    let status = Command::new("bash")
        .arg(&script_path)
        .args(&script_args)
        .status()
        .expect("failed to spawn bash");

    if !status.success() {
        eprintln!("Build failed.");
        std::process::exit(1);
    }

    let img = Path::new(IMAGE_PATH);
    if img.exists() {
        let size = std::fs::metadata(img).map(|m| m.len()).unwrap_or(0);
        println!();
        println!(
            "Image ready: {} ({} MiB)",
            img.display(),
            size / (1024 * 1024)
        );
        println!();
        println!("Next: find your USB device with `lsblk`, then:");
        println!("  sudo testos-launcher write /dev/sdX");
    } else {
        eprintln!(
            "Build script reported success but image not found at {}",
            img.display()
        );
        std::process::exit(1);
    }
}

fn cmd_write(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: testos-launcher write /dev/sdX");
        std::process::exit(1);
    }
    let device = &args[0];
    let dev_path = Path::new(device);

    if !dev_path.exists() {
        eprintln!("ERROR: device {} does not exist.", device);
        std::process::exit(1);
    }

    let img = Path::new(IMAGE_PATH);
    if !img.exists() {
        eprintln!(
            "ERROR: image not found at {}. Run `testos-launcher build` first.",
            img.display()
        );
        std::process::exit(1);
    }

    // Safety check: refuse to write to a mounted or in-use device.
    if is_device_mounted(device) {
        eprintln!(
            "ERROR: device {} appears to be mounted. Unmount it first:",
            device
        );
        eprintln!("  sudo umount {}*", device);
        std::process::exit(1);
    }

    // Safety check: refuse to write to the host's root device.
    if is_root_device(device) {
        eprintln!(
            "ERROR: device {} looks like the host's root disk. Refusing to overwrite.",
            device
        );
        eprintln!("       If this is wrong, check `lsblk` and `findmnt /`.");
        std::process::exit(1);
    }

    // Confirm with the user.
    let size = std::fs::metadata(img).map(|m| m.len()).unwrap_or(0);
    println!(
        "About to write {} ({} MiB) to {}.",
        img.display(),
        size / (1024 * 1024),
        device
    );
    println!("ALL DATA ON {} WILL BE LOST.", device);
    print!("Type the device name again to confirm: ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut confirm = String::new();
    std::io::stdin().read_line(&mut confirm).ok();
    if confirm.trim() != device {
        eprintln!("Confirmation did not match. Aborting.");
        std::process::exit(1);
    }

    println!("Writing... (this takes a minute or two)");
    let status = Command::new("dd")
        .arg(format!("if={}", img.display()))
        .arg(format!("of={}", device))
        .arg("bs=4M")
        .arg("status=progress")
        .arg("conv=fsync")
        .status()
        .expect("failed to spawn dd");

    if !status.success() {
        eprintln!("dd failed.");
        std::process::exit(1);
    }

    // Sync and tell the kernel to re-read the partition table.
    let _ = Command::new("sync").status();
    let _ = Command::new("blockdev")
        .arg("--flushbufs")
        .arg(device)
        .status();
    let _ = Command::new("partprobe").arg(device).status();

    println!();
    println!("Done. The USB is bootable.");
    println!("  - Plug it into the test machine.");
    println!("  - Reboot, pick the USB from the boot menu.");
    println!("  - testOS will boot, show a menu, run benchmarks, write results to the USB.");
    println!("  - After testOS reboots the machine back, run `testos-ingest pull {}` to collect results.", device);
}

fn cmd_preview(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: testos-launcher preview /dev/sdX");
        std::process::exit(1);
    }
    let device = &args[0];
    println!("Device: {}", device);
    println!();

    // Show partitions.
    let _ = Command::new("lsblk").arg(device).status();

    // Try to find the testOS partition by mounting it temporarily.
    // For preview we look for the partition with label RUSHESP.
    let parts = find_partitions(device);
    let mut found_esp = false;
    for p in &parts {
        if let Some(label) = read_partition_label(p) {
            if label == "RUSHESP" {
                found_esp = true;
                println!();
                println!("Found Rush ESP at {}", p);
                // Mount read-only temporarily.
                let mnt = "/mnt/testos-preview";
                let _ = std::fs::create_dir_all(mnt);
                let mount_ok = Command::new("mount")
                    .arg("-o")
                    .arg("ro")
                    .arg(p)
                    .arg(mnt)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if mount_ok {
                    let bench_list = Path::new(mnt).join("testos/bench-list.toml");
                    if bench_list.exists() {
                        println!("Bench list on USB:");
                        match testos::BenchList::load(&bench_list) {
                            Ok(list) => {
                                for (i, b) in list.benches.iter().enumerate() {
                                    println!("  [{}] {} ({}s)", i + 1, b.name, b.estimated_seconds);
                                }
                                println!(
                                    "  Total ETA: {}",
                                    testos::BenchList::format_duration(
                                        list.total_estimated_seconds()
                                    )
                                );
                            }
                            Err(e) => println!("  (failed to parse: {})", e),
                        }
                    } else {
                        println!("No bench-list.toml on the ESP. Build may be incomplete.");
                    }
                    let _ = Command::new("umount").arg(mnt).status();
                } else {
                    println!("  (could not mount ESP for preview)");
                }
                break;
            }
        }
    }
    if !found_esp {
        println!();
        println!(
            "No Rush ESP partition found on {}. Wrong device, or USB not yet written?",
            device
        );
    }
}

fn cmd_version() {
    println!("testOS launcher v{}", env!("CARGO_PKG_VERSION"));
    println!("Schema version: {}", testos::SCHEMA_VERSION);
}

fn is_device_mounted(device: &str) -> bool {
    // Parse /proc/mounts.
    let text = match std::fs::read_to_string("/proc/mounts") {
        Ok(t) => t,
        Err(_) => return false,
    };
    for line in text.lines() {
        let dev = line.split_whitespace().next().unwrap_or("");
        if dev == device || dev.starts_with(&format!("{}p", device)) || dev.starts_with(device) {
            // Conservative: any line whose device starts with our device string.
            // This catches /dev/sda1 etc.
            return true;
        }
    }
    false
}

fn is_root_device(device: &str) -> bool {
    // findmnt / -> source device. Compare the base name.
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "/"])
        .output();
    if let Ok(o) = out {
        let root_dev = String::from_utf8_lossy(&o.stdout).trim().to_string();
        // Strip partition digit(s) to get the base device.
        let root_base = strip_partition(&root_dev);
        let dev_base = strip_partition(device);
        if !root_base.is_empty() && root_base == dev_base {
            return true;
        }
    }
    false
}

fn strip_partition(dev: &str) -> String {
    // /dev/sda1 -> /dev/sda, /dev/nvme0n1p2 -> /dev/nvme0n1
    if let Some(stripped) = dev.strip_prefix("/dev/") {
        // nvme/mmcblk style: ends with p<digit>
        if let Some(idx) = stripped.rfind("p") {
            let after = &stripped[idx + 1..];
            if after.chars().all(|c| c.is_ascii_digit()) && !after.is_empty() {
                return format!("/dev/{}", &stripped[..idx]);
            }
        }
        // sd style: ends with a single digit
        let mut chars = stripped.chars().rev();
        if let Some(c) = chars.next() {
            if c.is_ascii_digit() {
                return format!("/dev/{}", &stripped[..stripped.len() - 1]);
            }
        }
    }
    dev.to_string()
}

fn find_partitions(device: &str) -> Vec<String> {
    // lsblk -ln -o NAME <device>
    let out = Command::new("lsblk")
        .args(["-ln", "-o", "NAME", device])
        .output();
    let mut parts = Vec::new();
    if let Ok(o) = out {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let name = line.trim();
            if name.is_empty() || name == device.strip_prefix("/dev/").unwrap_or(device) {
                continue;
            }
            parts.push(format!("/dev/{}", name));
        }
    }
    parts
}

fn read_partition_label(part: &str) -> Option<String> {
    let out = Command::new("lsblk")
        .args(["-ln", "-o", "LABEL", part])
        .output()
        .ok()?;
    let label = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}
