use std::fs;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn s3d_apply_unit_orders_recovery_before_daemon() {
    let root = repository_root();
    let apply = fs::read_to_string(root.join("packaging/systemd/optid-apply.service"))
        .expect("read optid-apply.service");
    let recover = fs::read_to_string(root.join("packaging/systemd/optid-recover.service"))
        .expect("read optid-recover.service");

    assert!(apply.contains("Type=notify"));
    assert!(apply.contains("NotifyAccess=main"));
    assert!(apply.contains("WatchdogSec="));
    assert!(apply.contains("ExecStartPre=/usr/libexec/optid-recover"));
    assert!(apply.contains("RestartPreventExitStatus=78"));
    assert!(recover.contains("Type=oneshot"));
    assert!(recover.contains("Before=optid-apply.service"));
    assert!(recover.contains("ExecStart=/usr/libexec/optid-recover"));
}

#[test]
fn s3d_failed_recovery_prevents_automatic_actuation_restart_loop() {
    let root = repository_root();
    let apply = fs::read_to_string(root.join("packaging/systemd/optid-apply.service"))
        .expect("read optid-apply.service");
    assert!(apply.contains("StartLimitIntervalSec="));
    assert!(apply.contains("StartLimitBurst="));
    assert!(apply.contains("RestartPreventExitStatus=78"));
}
