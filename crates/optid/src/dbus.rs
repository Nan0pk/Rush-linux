//! D-Bus server surface exposed by `optid`.
//!
//! The interface `io.rushlinux.Optid1` is the public control API documented
//! in `packaging/dbus/io.rushlinux.Optid.xml`. `optctl` calls into this
//! interface; if the bus is offline (e.g. running outside systemd), `optctl`
//! falls back to reading the same state files directly, so the methods here
//! only need to mirror the file writes — they do not need to broadcast.

use std::fs;
use std::path::PathBuf;

use zbus::interface;

use crate::workload::Mode;

pub(crate) struct OptidServer {
    pub(crate) state_dir: PathBuf,
}

#[interface(name = "io.rushlinux.Optid1")]
impl OptidServer {
    fn status(&self) -> zbus::fdo::Result<String> {
        fs::read_to_string(self.state_dir.join("status"))
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to read status: {e}")))
    }

    fn explain(&self) -> zbus::fdo::Result<String> {
        fs::read_to_string(self.state_dir.join("decisions.log"))
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to read decisions.log: {e}")))
    }

    fn set_mode(&self, mode: &str) -> zbus::fdo::Result<()> {
        let mode_parsed = Mode::parse(mode)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("invalid mode: {mode}")))?;
        fs::write(self.state_dir.join("mode"), mode_parsed.to_string())
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to write mode: {e}")))
    }

    fn pin_application(&self, app_id: &str, class: &str) -> zbus::fdo::Result<()> {
        let classes = [
            "idle",
            "light",
            "interactive",
            "latency-critical",
            "throughput",
        ];
        if !classes.contains(&class) {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "invalid workload class: {class}"
            )));
        }
        if app_id == "--global" {
            fs::write(self.state_dir.join("workload_class_pin"), class).map_err(|e| {
                zbus::fdo::Error::Failed(format!("failed to write global pin: {e}"))
            })?;
            println!("Pinned global workload class to {class}");
            return Ok(());
        }
        let pins_dir = self.state_dir.join("pins");
        fs::create_dir_all(&pins_dir)
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to create pins dir: {e}")))?;
        fs::write(pins_dir.join(app_id), class)
            .map_err(|e| zbus::fdo::Error::Failed(format!("failed to write pin: {e}")))?;
        println!("Pinned application {app_id} to class {class}");
        Ok(())
    }

    #[zbus(property)]
    fn mode(&self) -> String {
        let text = fs::read_to_string(self.state_dir.join("mode")).unwrap_or_default();
        Mode::parse(&text).unwrap_or(Mode::Auto).to_string()
    }

    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}
