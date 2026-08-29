//! `optid-recover` — S3D one-shot recovery executable.

#[path = "../recovery.rs"]
mod recovery;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use recovery::{recover_directory, DEFAULT_RECOVERY_DIR, RECOVERY_FAILURE_EXIT};

fn usage() {
    eprintln!("Usage: optid-recover [--recovery-dir PATH] [--status-file PATH]");
    #[cfg(feature = "test-simulation")]
    eprintln!("       optid-recover [--machine-root PATH]   (test-simulation builds only)");
}

fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, content)?;
    fs::File::open(&temp)?.sync_all()?;
    fs::rename(&temp, path)?;
    fs::File::open(parent)?.sync_all()
}

fn main() {
    let mut recovery_dir = PathBuf::from(DEFAULT_RECOVERY_DIR);
    let mut status_file = PathBuf::from("/run/optid/recovery-status.json");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // I2 — rebase every recorded target path onto a simulated machine
            // tree so this binary can be exercised end to end without a write
            // reaching a real kernel path. The flag does not exist in a shipped
            // build: it is compiled only with the non-default
            // `test-simulation` feature.
            #[cfg(feature = "test-simulation")]
            "--machine-root" => {
                let Some(value) = args.next() else {
                    usage();
                    std::process::exit(2);
                };
                recovery::set_simulated_machine_root(Some(PathBuf::from(value)));
            }
            "--recovery-dir" => {
                let Some(value) = args.next() else {
                    usage();
                    std::process::exit(2);
                };
                recovery_dir = PathBuf::from(value);
            }
            "--status-file" => {
                let Some(value) = args.next() else {
                    usage();
                    std::process::exit(2);
                };
                status_file = PathBuf::from(value);
            }
            "--help" | "-h" => {
                usage();
                return;
            }
            _ => {
                eprintln!("optid-recover: unknown argument {arg}");
                usage();
                std::process::exit(2);
            }
        }
    }

    let summary = recover_directory(&recovery_dir);
    let rendered = serde_json::to_string_pretty(&summary)
        .expect("RecoverySummary serialization is infallible");
    if let Err(error) = atomic_write(&status_file, &rendered) {
        eprintln!("optid-recover: cannot write recovery status: {error}");
        std::process::exit(RECOVERY_FAILURE_EXIT);
    }
    println!("{rendered}");
    if !summary.succeeded() {
        std::process::exit(RECOVERY_FAILURE_EXIT);
    }
}
