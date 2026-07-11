//! WP-N4 — `optctl` allowlist management (`allow` / `deny` / `list-allow`).
//!
//! These are the admin-facing surface of the hardware allowlist specified in
//! docs/research/0006-hw-allowlist-db-design.md §1.8. `allow`/`deny` write a
//! runtime override entry into `/etc/optid/allowlist.d/90-admin.toml` (the
//! highest-precedence persisted layer, §1.9); optid picks it up on its next
//! load/reload. The `90-` prefix ensures admin entries sort after distro
//! drop-ins so they win on a tie (§1.9).
//!
//! optctl deliberately does NOT decide allow/deny itself — it only records the
//! admin's explicit decision into the file optid reads. The gate logic lives in
//! `optid::allowlist`. `list-allow` shows the persisted override files (the
//! compiled-in seeded baseline lives inside the optid binary and is surfaced by
//! optid itself, not readable from here).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_ADMIN_DIR: &str = "/etc/optid/allowlist.d";
const ADMIN_FILE: &str = "90-admin.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryAction {
    Allow,
    Deny,
}

impl EntryAction {
    fn as_str(self) -> &'static str {
        match self {
            EntryAction::Allow => "allow",
            EntryAction::Deny => "deny",
        }
    }
}

/// Resolve a user-supplied target into a canonical HWID (kernel MODALIAS).
///
/// Accepts a modalias directly (`pci:…`, `usb:…`, `acpi:…`), a sysfs device
/// directory (reads its `modalias` attribute), or — for anything else, e.g.
/// `/dev/nvme0` — shells out to `udevadm info` per 0006 §1.8.
pub(crate) fn resolve_hwid(target: &str) -> io::Result<String> {
    if target.starts_with("pci:") || target.starts_with("usb:") || target.starts_with("acpi:") {
        return Ok(target.to_string());
    }

    let path = Path::new(target);
    if path.is_dir() {
        let modalias = fs::read_to_string(path.join("modalias")).map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("could not read modalias for {target}: {e}"),
            )
        })?;
        let trimmed = modalias.trim();
        if trimmed.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("empty modalias for {target}"),
            ));
        }
        return Ok(trimmed.to_string());
    }

    // Fall back to udevadm for device nodes / paths we can't read directly.
    let output = std::process::Command::new("udevadm")
        .args(["info", "--query=property", target])
        .output()
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("could not resolve HWID for {target}: udevadm unavailable: {e}"),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("udevadm could not resolve {target}"),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("MODALIAS=") {
            return Ok(value.trim().to_string());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{target} has no MODALIAS property"),
    ))
}

/// Render a single `[[entry]]` TOML block. Strings are escaped defensively.
pub(crate) fn format_entry(
    action: EntryAction,
    domain: &str,
    hwid: &str,
    max_state: Option<u32>,
    reason: Option<&str>,
) -> String {
    let mut block = String::from("\n[[entry]]\n");
    block.push_str(&format!("domain = {}\n", toml_string(domain)));
    block.push_str(&format!("hwid = {}\n", toml_string(hwid)));
    block.push_str(&format!("action = \"{}\"\n", action.as_str()));
    if action == EntryAction::Allow {
        // `optctl allow` records a candidate. It must not silently convert an
        // operator request into a claim of completed hardware verification.
        block.push_str("verified = false\n");
    }
    if let Some(n) = max_state {
        block.push_str(&format!("max_state = {n}\n"));
    }
    let reason = reason.unwrap_or("added via optctl");
    block.push_str(&format!("reason = {}\n", toml_string(reason)));
    block
}

fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Append an override entry to `<dir>/90-admin.toml`, creating the dir/file as
/// needed. Returns the path written.
pub(crate) fn write_entry(
    dir: &Path,
    action: EntryAction,
    domain: &str,
    target: &str,
    max_state: Option<u32>,
    reason: Option<&str>,
) -> io::Result<(PathBuf, String)> {
    let hwid = resolve_hwid(target)?;
    fs::create_dir_all(dir)?;
    let file = dir.join(ADMIN_FILE);
    let block = format_entry(action, domain, &hwid, max_state, reason);

    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?;
    f.write_all(block.as_bytes())?;
    Ok((file, hwid))
}

/// Print every persisted override file in `dir` (faithful TOML listing).
pub(crate) fn list(dir: &Path) -> io::Result<()> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            println!("no allowlist overrides in {} (seeded baseline is compiled into optid; run `optctl explain` against a running daemon to see the effective list)", dir.display());
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let mut files: Vec<_> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
        .collect();
    files.sort();
    if files.is_empty() {
        println!("no allowlist override files in {}", dir.display());
        return Ok(());
    }
    for path in files {
        println!("# {}", path.display());
        print!("{}", fs::read_to_string(&path)?);
    }
    Ok(())
}

/// Dispatch `allow` / `deny` / `list-allow`. `positional[0]` is the command.
pub(crate) fn run(positional: &[String]) -> io::Result<()> {
    let command = positional.first().map(String::as_str).unwrap_or("");
    let dir = match std::env::var("OPTID_ALLOWLIST_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => PathBuf::from(DEFAULT_ADMIN_DIR),
    };

    match command {
        "list-allow" => list(&dir),
        "allow" | "deny" => {
            let action = if command == "allow" {
                EntryAction::Allow
            } else {
                EntryAction::Deny
            };
            // optctl <allow|deny> <domain> <hwid|dev-path> [--max-state N] [--reason "..."]
            let domain = positional.get(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("usage: optctl {command} <domain> <hwid|dev-path> [--max-state N] [--reason \"...\"]"),
                )
            })?;
            let target = positional.get(2).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("optctl {command} requires a <hwid|dev-path>"),
                )
            })?;

            let mut max_state = None;
            let mut reason = None;
            let mut i = 3;
            while i < positional.len() {
                match positional[i].as_str() {
                    "--max-state" => {
                        let v = positional.get(i + 1).ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "--max-state requires a value",
                            )
                        })?;
                        max_state = Some(v.parse::<u32>().map_err(|_| {
                            io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "--max-state must be an integer",
                            )
                        })?);
                        i += 2;
                    }
                    "--reason" => {
                        reason = positional.get(i + 1).cloned();
                        i += 2;
                    }
                    other => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("unexpected argument: {other}"),
                        ));
                    }
                }
            }

            let (file, hwid) =
                write_entry(&dir, action, domain, target, max_state, reason.as_deref())?;
            println!(
                "wrote {} entry for {hwid} (domain {domain}) to {}",
                action.as_str(),
                file.display()
            );
            if action == EntryAction::Allow {
                println!(
                    "candidate recorded as verified=false; optid can explain it but will not write to it"
                );
                println!(
                    "collect hardware evidence, then ask the maintainer to promote it to verified=true"
                );
            } else {
                println!("optid applies the deny on next load/reload (SIGHUP or restart).");
            }
            Ok(())
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown allowlist command: {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_hwid_passthrough_modalias() {
        assert_eq!(
            resolve_hwid("pci:v0000144Dp00009A36").unwrap(),
            "pci:v0000144Dp00009A36"
        );
        assert_eq!(resolve_hwid("usb:v1234p5678").unwrap(), "usb:v1234p5678");
    }

    #[test]
    fn resolve_hwid_reads_sysfs_dir() {
        let base = std::env::temp_dir().join(format!("optctl_allow_dir_{}", std::process::id()));
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("modalias"), "pci:v0000ABCDp00001234\n").unwrap();
        assert_eq!(
            resolve_hwid(base.to_str().unwrap()).unwrap(),
            "pci:v0000ABCDp00001234"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn format_entry_includes_fields() {
        let block = format_entry(
            EntryAction::Allow,
            "nvme_apst",
            "pci:vAAAA",
            Some(3),
            Some("tested on bench"),
        );
        assert!(block.contains("domain = \"nvme_apst\""));
        assert!(block.contains("hwid = \"pci:vAAAA\""));
        assert!(block.contains("action = \"allow\""));
        assert!(block.contains("verified = false"));
        assert!(block.contains("max_state = 3"));
        assert!(block.contains("reason = \"tested on bench\""));
    }

    #[test]
    fn write_entry_appends_to_admin_file() {
        let base = std::env::temp_dir().join(format!("optctl_allow_write_{}", std::process::id()));
        let (file, hwid) = write_entry(
            &base,
            EntryAction::Deny,
            "pci_aspm",
            "pci:v00008086p00002526",
            None,
            Some("L1.2 link drop"),
        )
        .unwrap();
        assert_eq!(hwid, "pci:v00008086p00002526");
        assert!(file.ends_with("90-admin.toml"));
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains("action = \"deny\""));
        assert!(content.contains("pci:v00008086p00002526"));

        // A second write appends rather than truncates.
        write_entry(
            &base,
            EntryAction::Allow,
            "nvme_apst",
            "pci:v0000144Dp00009A36",
            Some(3),
            None,
        )
        .unwrap();
        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content.matches("[[entry]]").count(), 2);
        let _ = fs::remove_dir_all(&base);
    }
}
