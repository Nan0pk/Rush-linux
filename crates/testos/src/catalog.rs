//! Bench catalog — the editable list of benchmarks testOS knows how to run.
//!
//! Source of truth: `testos/bench-list.toml` in the repo root.
//!
//! The launcher reads this to show the menu and estimate runtime; the runner
//! reads this to know what commands to execute after boot. Adding a new test
//! means adding one entry to the TOML — no code changes required.

use serde::{Deserialize, Serialize};

/// The kind of benchmark — controls how the runner invokes it and what it expects back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BenchKind {
    /// Shell command whose stdout is a single number (e.g. "1500"). The runner captures it.
    ShellNumeric,
    /// Shell command that writes its own JSON to a file passed via $TESTOS_RESULT_FILE.
    ShellJson,
    /// Shell command whose exit code is the only signal (0 = pass). Used for stress tests.
    ShellPassFail,
    /// Calls `rushbench run --class <class> --workload <workload>` and parses its JSON output.
    Rushbench,
}

/// One benchmark entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bench {
    /// Short identifier used in filenames, e.g. "fio-seq-read".
    pub id: String,
    /// Human-readable name shown in the menu, e.g. "fio — sequential read IOPS".
    pub name: String,
    /// Which manifest scenario this benchmark contributes to.
    pub scenario: String,
    /// What kind of command this is.
    pub kind: BenchKind,
    /// The shell command to run, executed via `bash -c`.
    pub command: String,
    /// Estimated wall-clock seconds for one run (used to compute total ETA).
    pub estimated_seconds: u64,
    /// Optional: requires battery (true means the runner will prompt the user to unplug AC).
    #[serde(default)]
    pub requires_battery: bool,
    /// Optional: free-form notes shown in the menu as "What it measures".
    #[serde(default)]
    pub notes: Option<String>,
    /// Optional: one-sentence "Why it matters" shown in the menu and the
    /// per-benchmark progress UI. Backward-compatible — older catalogs
    /// without this field still load (the runner falls back to the `notes`
    /// text or an empty string). Added by the boot-reliability/UI PR so the
    /// UI never embeds hard-coded descriptions in code.
    #[serde(default)]
    pub significance: Option<String>,
    /// Optional: the unit for shell-numeric benchmarks (e.g. "ms", "us",
    /// "percent", "Gbit/s", "requests/s"). If absent, shell-numeric results
    /// record unit="numeric" (legacy behavior). shell-json benchmarks get
    /// their unit from the JSON the command writes.
    #[serde(default)]
    pub unit: Option<String>,
}

/// The full editable list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchList {
    /// Catalog version — bump when the TOML schema changes.
    pub version: u32,
    /// The benchmarks, in display order.
    pub benches: Vec<Bench>,
}

impl BenchList {
    /// Load from a TOML file path.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read bench list at {}: {}", path.display(), e))?;
        let list: BenchList =
            toml::from_str(&text).map_err(|e| format!("failed to parse bench list TOML: {}", e))?;
        Ok(list)
    }

    /// Sum of all estimated runtimes (used to compute "Run all" ETA).
    pub fn total_estimated_seconds(&self) -> u64 {
        self.benches.iter().map(|b| b.estimated_seconds).sum()
    }

    /// Format a duration as "Xm Ys" for display.
    pub fn format_duration(secs: u64) -> String {
        if secs < 60 {
            format!("{}s", secs)
        } else {
            let m = secs / 60;
            let s = secs % 60;
            if s == 0 {
                format!("{}m", m)
            } else {
                format!("{}m {}s", m, s)
            }
        }
    }
}

impl Bench {
    /// Return the "Why it matters" text for the UI, falling back to the
    /// `notes` field (shown as "What it measures") when `significance` is
    /// absent. Older catalogs without `significance` still render something
    /// useful rather than an empty line.
    pub fn significance_or_fallback(&self) -> &str {
        if let Some(s) = &self.significance {
            let t = s.trim();
            if !t.is_empty() {
                return t;
            }
        }
        if let Some(n) = &self.notes {
            let t = n.trim();
            if !t.is_empty() {
                return t;
            }
        }
        ""
    }

    /// Return the "What it measures" text for the UI. This is the `notes`
    /// field; we treat it as optional and the UI degrades to an empty line
    /// when absent.
    pub fn measures_text(&self) -> &str {
        self.notes.as_deref().map(str::trim).unwrap_or("")
    }
}
