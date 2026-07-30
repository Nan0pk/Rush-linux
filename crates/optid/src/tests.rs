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
