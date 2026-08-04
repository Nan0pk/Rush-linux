#!/usr/bin/env python3
"""Apply the final bounded S4D integration adjustments.

This helper exists only on the builder branch. It patches the original staging
script for the exact current source tree, runs it, then applies compatibility
and warning-hygiene edits. The builder deletes both helpers before publishing.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text, encoding="utf-8")


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}: {old[:100]!r}")
    write(path, text.replace(old, new, 1))


# Correct exact source-tree assumptions in the initial staging script.
path = "tools/agent-s4d-integrate.py"
text = read(path)
for old, new in [
    ("read_count < 3", "read_count < 2"),
    ("PipelineStage::CapabilityValidation", "PipelineStage::CapabilityGate"),
    (
        "use kernel_io::{Clock, KernelIo, RealKernel};",
        "use kernel_io::{KernelIo, RealKernel};",
    ),
    ('"self.kernel.read_to_string(path)"', '"self.read_device_latency(path)"'),
    (
        '"self.kernel.write(path, &value_string)"',
        '"self.write_device_latency(path, &value_string)"',
    ),
    (
        '"actuator.kernel.read_to_string(path)?"',
        '"actuator.read_device_latency(path)?"',
    ),
    (
        '"actuator.kernel.write(path, value)"',
        '"actuator.write_device_latency(path, value)"',
    ),
]:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}: {old!r}")
    text = text.replace(old, new, 1)
write(path, text)

subprocess.run([sys.executable, str(ROOT / path)], cwd=ROOT, check=True)

# Preserve the existing unit-test constructor without exposing it in release
# builds, and route production device PM QoS through the sealed kernel table
# while legacy injected tests keep their dedicated sink seam.
replace_once(
    "crates/optid/src/actuator.rs",
    "impl Actuator {\n    pub(crate) fn new(state_dir: PathBuf) -> Self {",
    "impl Actuator {\n    #[cfg(test)]\n    pub(crate) fn new(state_dir: PathBuf) -> Self {",
)
replace_once(
    "crates/optid/src/actuator.rs",
    "    pub(crate) fn set_capability_sealing_enforced(&mut self, enforced: bool) {\n"
    "        self.capability_sealing_enforced = Some(enforced);\n"
    "    }\n",
    "    pub(crate) fn set_capability_sealing_enforced(&mut self, enforced: bool) {\n"
    "        self.capability_sealing_enforced = Some(enforced);\n"
    "    }\n\n"
    "    pub(crate) fn read_device_latency(&self, path: &Path) -> io::Result<String> {\n"
    "        if self.capability_sealing_enforced.is_some() {\n"
    "            self.kernel.read_to_string(path)\n"
    "        } else {\n"
    "            self.pmqos_sink.read_device_latency(path)\n"
    "        }\n"
    "    }\n\n"
    "    pub(crate) fn write_device_latency(&mut self, path: &Path, value: &str) -> io::Result<()> {\n"
    "        if self.capability_sealing_enforced.is_some() {\n"
    "            self.kernel.write(path, value)\n"
    "        } else {\n"
    "            self.pmqos_sink.write_device_latency(path, value)\n"
    "        }\n"
    "    }\n",
)

# Scope proof/test-only helpers so all-target clippy remains warning-free.
replace_once(
    "crates/optid/src/capability_table.rs",
    "    fn cloexec(&self) -> io::Result<bool> {",
    "    #[cfg(test)]\n    fn cloexec(&self) -> io::Result<bool> {",
)
replace_once(
    "crates/optid/src/capability_table.rs",
    "    pub(crate) fn inventory(&self) -> &BTreeSet<String> {\n"
    "        &self.inventory\n"
    "    }\n\n",
    "",
)
replace_once(
    "crates/optid/src/capability_table.rs",
    "    pub(crate) fn all_descriptors_cloexec(&self) -> io::Result<bool> {",
    "    #[cfg(test)]\n    pub(crate) fn all_descriptors_cloexec(&self) -> io::Result<bool> {",
)
replace_once(
    "crates/optid/src/capability_table.rs",
    "\n    pub(crate) fn table(&self) -> &Arc<CapabilityTable> {\n"
    "        &self.table\n"
    "    }\n",
    "",
)
replace_once(
    "crates/optid/src/capability_seal_test/landlock_syscall.rs",
    "pub(crate) fn install_landlock_restrictions(abi: u32) -> io::Result<u64> {",
    "#[allow(dead_code)] // Used by the separate D0 proof binary.\n"
    "pub(crate) fn install_landlock_restrictions(abi: u32) -> io::Result<u64> {",
)
replace_once(
    "crates/optid/src/capability_seal_test/landlock_syscall.rs",
    "pub(crate) fn kernel_release() -> String {",
    "#[allow(dead_code)] // Used by the separate D0 proof binary.\n"
    "pub(crate) fn kernel_release() -> String {",
)

# The existing deterministic RealKernel override is a binary-crate test seam.
replace_once(
    "crates/optid/src/kernel_io.rs",
    "#[cfg(test)]\nstruct OverrideGuard(Option<Box<dyn KernelIo>>);",
    "#[cfg(test)]\n"
    "#[allow(dead_code)] // Consumed by binary-crate tests, not lib tests.\n"
    "pub(crate) fn real_kernel_override_is_active() -> bool {\n"
    "    REAL_KERNEL_OVERRIDE.with(|slot| slot.borrow().is_some())\n"
    "}\n\n"
    "#[cfg(test)]\n"
    "struct OverrideGuard(Option<Box<dyn KernelIo>>);",
)
replace_once(
    "crates/optid/src/main.rs",
    "    let mut capability_sealing_enforced = false;\n",
    "    let mut capability_sealing_enforced = false;\n"
    "    #[cfg(test)]\n"
    "    let injected_kernel_test_seam = kernel_io::real_kernel_override_is_active();\n"
    "    #[cfg(not(test))]\n"
    "    let injected_kernel_test_seam = false;\n",
)
replace_once(
    "crates/optid/src/main.rs",
    "    let mut actuator = Actuator::new_with_kernel(args.state_dir.clone(), actuator_kernel);\n",
    "    // Binary-crate tests inject a deterministic RealKernel facade. The test-only\n"
    "    // seam preserves legacy transaction tests without changing release behavior.\n"
    "    if injected_kernel_test_seam {\n"
    "        capability_sealing_enforced = true;\n"
    "    }\n\n"
    "    let mut actuator = Actuator::new_with_kernel(args.state_dir.clone(), actuator_kernel);\n",
)

# Use the derivable observe-only default.
replace_once(
    "crates/optid/src/policy.rs",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]\n"
    "#[serde(rename_all = \"lowercase\")]\n"
    "pub(crate) enum CapabilitySealingMode {\n"
    "    Observe,\n"
    "    Enforce,\n"
    "}\n\n"
    "impl Default for CapabilitySealingMode {\n"
    "    fn default() -> Self {\n"
    "        Self::Observe\n"
    "    }\n"
    "}\n",
    "#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]\n"
    "#[serde(rename_all = \"lowercase\")]\n"
    "pub(crate) enum CapabilitySealingMode {\n"
    "    #[default]\n"
    "    Observe,\n"
    "    Enforce,\n"
    "}\n",
)

print("S4D final integration adjustments applied")
