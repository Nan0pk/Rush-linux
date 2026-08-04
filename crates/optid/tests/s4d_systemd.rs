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
fn s4d_apply_unit_restarts_only_through_supervised_recovery_graph() {
    let root = repository_root();
    let apply = fs::read_to_string(root.join("packaging/systemd/optid-apply.service"))
        .expect("read apply unit");
    let recover = fs::read_to_string(root.join("packaging/systemd/optid-recover.service"))
        .expect("read recovery unit");
    let mirror = fs::read_to_string(
        root.join("mkosi/mkosi.extra/usr/lib/systemd/system/optid-apply.service"),
    )
    .expect("read mkosi apply unit");

    assert_eq!(apply, mirror, "packaging and image units must be identical");
    assert!(apply.contains("RestartForceExitStatus=75"));
    assert!(apply.contains("Requires=optid-recover.service"));
    assert!(apply.contains("After=multi-user.target systemd-udevd.service optid-recover.service"));
    assert!(recover.contains("Before=optid-apply.service"));
    assert!(recover.contains("PartOf=optid-apply.service"));
    assert!(recover.contains("Restart=no"));
}

#[test]
fn s4d_startup_seals_before_any_worker_or_dbus_input() {
    let root = repository_root();
    let source = fs::read_to_string(root.join("crates/optid/src/main.rs"))
        .expect("read production entry point");

    let seal = source
        .find("table.seal(&state_roots)")
        .expect("production seal call");
    let dbus = source
        .find("spawn_dbus_servers(")
        .expect("D-Bus startup call");
    let foreground = source
        .find("foreground::subscribe(")
        .expect("foreground worker startup call");

    assert!(seal < dbus, "Landlock must be installed before D-Bus");
    assert!(seal < foreground, "Landlock must be installed before workers");
}

#[test]
fn s4d_topology_rebuild_hands_back_before_status_75() {
    let root = repository_root();
    let source = fs::read_to_string(root.join("crates/optid/src/main.rs"))
        .expect("read production entry point");
    let branch = source
        .split("TopologyDecision::Rebuild =>")
        .nth(1)
        .expect("topology rebuild branch");
    let handback = branch
        .find("restore_all_owned")
        .expect("handback before rebuild");
    let exit = branch
        .find("RunExit::TopologyRebuild")
        .expect("dedicated rebuild exit");
    assert!(handback < exit, "owned levers must be handed back before exit 75");
}
