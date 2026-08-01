//! Cross-module and production-surface tests for `optid`.

#![cfg(test)]

// Preserve the existing cross-module suite unchanged. The additional F2 test
// below lives at the binary-crate surface so it can enter through `run(args)`.
#[path = "tests_impl.rs"]
mod existing;

mod f2_production_surface {
    use std::fs;
    use std::io;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::args::{Args, ForegroundMode};
    use crate::kernel_io::{with_real_kernel_override, FaultKernel, MemoryKernel};

    #[test]
    fn f2_production_daemon_run_consumes_injected_kernel_io() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock must be after the Unix epoch")
            .as_nanos();
        let state_dir = std::env::temp_dir().join(format!(
            "optid_f2_production_surface_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&state_dir).expect("create isolated daemon state directory");

        let config_path = state_dir.join("policy.toml");
        fs::write(&config_path, "").expect("write minimal policy fixture");

        let decisions_log = state_dir.join("decisions.log");
        let fault = FaultKernel::new(Box::new(MemoryKernel::new()));
        fault.fail_next_write(decisions_log.clone(), io::ErrorKind::PermissionDenied);

        let args = Args {
            apply: false,
            once: true,
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: state_dir.clone(),
            config_path,
            allowlist: false,
            foreground: ForegroundMode::Off,
        };

        let result = with_real_kernel_override(Box::new(fault), || crate::run(args));
        let error = result.expect_err(
            "the daemon entry point must observe the fault injected through RealKernel",
        );

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            error.to_string().contains("injected append fault"),
            "the error must originate from FaultKernel, got: {error}"
        );
        assert!(
            !decisions_log.exists(),
            "bypassing the injected seam would have created decisions.log"
        );

        fs::remove_dir_all(&state_dir).expect("remove isolated daemon state directory");
    }
}

mod f3_production_surface {
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::args::{Args, ForegroundMode};
    use crate::envelope::{ControlCycleEnvelope, ENVELOPE_SCHEMA_VERSION};
    use crate::kernel_io::{
        with_real_kernel_override, Clock, EventSource, KernelRead, KernelWrite, MemoryKernel,
    };
    use crate::policy::DomainMode;

    #[derive(Clone)]
    struct SharedKernel(Arc<MemoryKernel>);

    impl KernelRead for SharedKernel {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.0.read_to_string(path)
        }

        fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
            self.0.read_dir(path)
        }

        fn exists(&self, path: &Path) -> bool {
            self.0.exists(path)
        }

        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            self.0.read_link(path)
        }

        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            self.0.canonicalize(path)
        }
    }

    impl KernelWrite for SharedKernel {
        fn write(&self, path: &Path, value: &str) -> io::Result<()> {
            self.0.write(path, value)
        }

        fn write_state_file(&self, path: &Path, value: &str) -> io::Result<()> {
            self.0.write_state_file(path, value)
        }

        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            self.0.create_dir_all(path)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.0.rename(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            self.0.remove_file(path)
        }

        fn append(&self, path: &Path, text: &str) -> io::Result<()> {
            self.0.append(path, text)
        }
    }

    impl Clock for SharedKernel {
        fn now_unix(&self) -> u64 {
            self.0.now_unix()
        }
    }

    impl EventSource for SharedKernel {
        fn wait(&self, duration: Duration) -> bool {
            let _ = duration;
            false
        }
    }

    #[test]
    fn f3_production_daemon_run_writes_one_correlated_versioned_cycle() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock must be after the Unix epoch")
            .as_nanos();
        let state_dir = std::env::temp_dir().join(format!(
            "optid_f3_production_surface_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&state_dir).expect("create isolated daemon state directory");

        let config_path = state_dir.join("policy.toml");
        let policy = format!(
            "{}\n[domains.cpu_epp]\nmode = \"off\"\n[domains.platform_profile]\nmode = \"observe\"\n[domains.vm_sysctl]\nmode = \"actuate\"\n",
            include_str!("../../../config/optid/policy.toml")
        );
        fs::write(&config_path, policy).expect("write F3 policy fixture");

        let memory = Arc::new(MemoryKernel::new());
        memory.advance_clock(1_700_000_000);
        let shared = SharedKernel(Arc::clone(&memory));
        let args = Args {
            apply: false,
            once: true,
            help: false,
            version: false,
            interval_sec: 1,
            state_dir: state_dir.clone(),
            config_path,
            allowlist: false,
            foreground: ForegroundMode::Off,
        };

        with_real_kernel_override(Box::new(shared), || crate::run(args))
            .expect("real daemon run must complete through injected kernel I/O");

        let status = memory
            .read_to_string(&state_dir.join("status"))
            .expect("daemon must write human status through injected I/O");
        let status_json = memory
            .read_to_string(&state_dir.join("status.json"))
            .expect("daemon must write machine status through injected I/O");
        let history = memory
            .read_to_string(&state_dir.join("control-cycles.jsonl"))
            .expect("daemon must append machine cycle history through injected I/O");
        let decisions = memory
            .read_to_string(&state_dir.join("decisions.log"))
            .expect("daemon must append correlated text decisions through injected I/O");

        let cycle: ControlCycleEnvelope = serde_json::from_str(&status_json)
            .expect("status.json must match the versioned schema");
        assert_eq!(cycle.schema_version, ENVELOPE_SCHEMA_VERSION);
        assert_eq!(cycle.cycle_timestamp, 1_700_000_000);
        assert!(!cycle.observation.values.is_empty());
        assert!(!cycle.decision.workload_class.is_empty());
        assert!(!cycle.decision.selected_mode.is_empty());
        assert!(!cycle.decision.contract.workload_class.is_empty());

        let cpu_epp = cycle
            .domains
            .iter()
            .find(|domain| domain.domain == "cpu_epp")
            .expect("off domain must remain visible");
        let platform_profile = cycle
            .domains
            .iter()
            .find(|domain| domain.domain == "platform_profile")
            .expect("observe domain must remain visible");
        let vm_sysctl = cycle
            .domains
            .iter()
            .find(|domain| domain.domain == "vm_sysctl")
            .expect("actuate domain must remain visible");
        assert_eq!(cpu_epp.selected_mode, DomainMode::Off);
        assert_eq!(platform_profile.selected_mode, DomainMode::Observe);
        assert_eq!(vm_sysctl.selected_mode, DomainMode::Actuate);

        let history_lines: Vec<&str> = history.lines().collect();
        assert_eq!(
            history_lines.len(),
            1,
            "one --once run must append one cycle"
        );
        let historical: ControlCycleEnvelope =
            serde_json::from_str(history_lines[0]).expect("JSONL entry must match the same schema");
        assert_eq!(historical, cycle);

        let correlation = &cycle.correlation_id;
        assert!(status.contains(correlation));
        assert!(decisions.contains(correlation));
        assert_eq!(
            correlation, "optid-000000006553f100-0000000000000000",
            "fake clock and boot sequence must make the first cycle deterministic"
        );

        fs::remove_dir_all(&state_dir).expect("remove isolated daemon state directory");
    }
}
