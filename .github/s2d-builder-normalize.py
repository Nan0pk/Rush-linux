#!/usr/bin/env python3
from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one normalization, found {count}")
    target.write_text(text.replace(old, new, 1), encoding="utf-8")


replace_once(
    "crates/optid/src/reconciler/tests/s2d.rs",
    "use crate::kernel_io::{Clock, FaultKernel, KernelRead, KernelWrite};",
    "use crate::envelope::GateDisposition;\nuse crate::kernel_io::{Clock, FaultKernel, KernelRead, KernelWrite};",
)
replace_once(
    "crates/optid/src/reconciler/tests/s2d.rs",
    "    let mut reconciler =\n        s2d_reconciler(state_dir, recovery_dir.clone(), &mut actuator, \"repeat-generation\");",
    "    let reconciler =\n        s2d_reconciler(state_dir, recovery_dir.clone(), &mut actuator, \"repeat-generation\");",
)
for variant in ("FailSyncFile", "FailSyncDir"):
    replace_once(
        "crates/optid/src/kernel_io_impl.rs",
        f"    {variant} {{",
        f"    #[allow(dead_code)]\n    {variant} {{",
    )
for method in ("fail_next_sync_file", "fail_next_sync_dir"):
    replace_once(
        "crates/optid/src/kernel_io_impl.rs",
        f"    pub fn {method}(",
        f"    #[allow(dead_code)]\n    pub fn {method}(",
    )
replace_once(
    "crates/optid/src/reconciler/transaction.rs",
    "const TRANSACTION_SCHEMA_VERSION: u32 = 1;\nconst DEFAULT_RECOVERY_DIR: &str = \"/var/lib/optid/recovery\";",
    "const TRANSACTION_SCHEMA_VERSION: u32 = 1;\n#[cfg(not(test))]\nconst DEFAULT_RECOVERY_DIR: &str = \"/var/lib/optid/recovery\";",
)
replace_once(
    "crates/optid/src/reconciler/transaction.rs",
    "impl TransactionPhase {\n    fn is_terminal(self) -> bool {",
    "impl TransactionPhase {\n    #[cfg(test)]\n    fn is_terminal(self) -> bool {",
)
replace_once(
    "crates/optid/src/reconciler/transaction.rs",
    "    fn active_records(\n",
    "    #[cfg(test)]\n    fn active_records(\n",
)
replace_once(
    "crates/optid/src/reconciler/transaction.rs",
    "    #[cfg(test)]\n    {\n        return state_dir.join(\"s2d-recovery\");\n    }",
    "    #[cfg(test)]\n    {\n        state_dir.join(\"s2d-recovery\")\n    }",
)
replace_once(
    "crates/optid/src/reconciler/state.rs",
    "    fn load_with_systemd(\n",
    "    #[cfg(test)]\n    fn load_with_systemd(\n",
)
replace_once(
    "crates/optid/src/reconciler/tests/unit.rs",
    "    reconciler.record_restore_outcome(&plan, &outcome, &io);",
    "    reconciler\n        .record_restore_outcome(&plan, &outcome, &io)\n        .expect(\"record restore outcome\");",
)
replace_once(
    "crates/optid/src/kernel_io_impl.rs",
    '''    pub fn add_dir_entry(&self, directory: &Path, entry: &Path) {
        self.dirs
            .lock()
            .expect("MemoryKernel dirs mutex poisoned")
            .entry(directory.to_path_buf())
            .or_default()
            .push(entry.to_path_buf());
    }
}''',
    '''    pub fn add_dir_entry(&self, directory: &Path, entry: &Path) {
        let mut dirs = self
            .dirs
            .lock()
            .expect("MemoryKernel dirs mutex poisoned");
        let entries = dirs.entry(directory.to_path_buf()).or_default();
        if !entries.iter().any(|candidate| candidate == entry) {
            entries.push(entry.to_path_buf());
        }
    }

    fn remove_dir_entry(&self, directory: &Path, entry: &Path) {
        if let Some(entries) = self
            .dirs
            .lock()
            .expect("MemoryKernel dirs mutex poisoned")
            .get_mut(directory)
        {
            entries.retain(|candidate| candidate != entry);
        }
    }
}''',
)
replace_once(
    "crates/optid/src/kernel_io_impl.rs",
    '''    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.write_raw(path, value);
        Ok(())
    }''',
    '''    fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
        self.write_raw(path, value);
        if let Some(parent) = path.parent() {
            self.add_dir_entry(parent, path);
        }
        Ok(())
    }''',
)
replace_once(
    "crates/optid/src/kernel_io_impl.rs",
    '''    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut files = self
            .files
            .lock()
            .expect("MemoryKernel files mutex poisoned");
        let content = files.remove(from).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "MemoryKernel: rename source not found",
            )
        })?;
        files.insert(to.to_path_buf(), content);
        Ok(())
    }''',
    '''    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut files = self
            .files
            .lock()
            .expect("MemoryKernel files mutex poisoned");
        let content = files.remove(from).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "MemoryKernel: rename source not found",
            )
        })?;
        files.insert(to.to_path_buf(), content);
        drop(files);
        if let Some(parent) = from.parent() {
            self.remove_dir_entry(parent, from);
        }
        if let Some(parent) = to.parent() {
            self.add_dir_entry(parent, to);
        }
        Ok(())
    }''',
)
replace_once(
    "crates/optid/src/kernel_io_impl.rs",
    '''    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "MemoryKernel: remove_file not found",
                )
            })
    }''',
    '''    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .remove(path)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "MemoryKernel: remove_file not found",
                )
            })?;
        if let Some(parent) = path.parent() {
            self.remove_dir_entry(parent, path);
        }
        Ok(())
    }''',
)
replace_once(
    "crates/optid/src/kernel_io_impl.rs",
    '''    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .entry(path.to_path_buf())
            .or_default()
            .push_str(text);
        Ok(())
    }''',
    '''    fn append(&self, path: &Path, text: &str) -> io::Result<()> {
        self.files
            .lock()
            .expect("MemoryKernel files mutex poisoned")
            .entry(path.to_path_buf())
            .or_default()
            .push_str(text);
        if let Some(parent) = path.parent() {
            self.add_dir_entry(parent, path);
        }
        Ok(())
    }''',
)
