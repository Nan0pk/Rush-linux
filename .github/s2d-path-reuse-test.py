#!/usr/bin/env python3
from pathlib import Path

path = Path("crates/optid/src/reconciler/tests/s2d.rs")
text = path.read_text(encoding="utf-8")
old = '''#[test]
fn s2d_path_reuse_identity_mismatch_is_rejected() {
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-path-reuse");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = MemoryKernel::new();
    memory.write_raw(&path, "60");
    memory.write_link(&path, Path::new("/devices/original/swappiness"));
    let action = vm_action(&path, "10");
    let desired = s2d_desired(&path, "10");
    let original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    let engine = TransactionEngine::new(recovery_dir, "identity-generation".to_string());
    engine
        .prepare(&memory, &action, &desired, &original)
        .expect("prepare original identity");
    memory.write_link(&path, Path::new("/devices/reused/swappiness"));

    let error = engine
        .prepare(&memory, &action, &desired, &original)
        .expect_err("path reuse must be rejected");
    assert_eq!(error.kind, TransactionErrorKind::IdentityMismatch);
    assert_eq!(memory.read_to_string(&path).expect("target unchanged"), "60");
}
'''
new = '''#[test]
fn s2d_path_reuse_identity_mismatch_is_rejected() {
    let recovery_dir = PathBuf::from("/var/lib/optid/recovery-path-reuse");
    let path = PathBuf::from("/proc/sys/vm/swappiness");
    let memory = Arc::new(MemoryKernel::new());
    let io = S2dSharedKernel(Arc::clone(&memory));
    memory.write_raw(&path, "60");
    memory.write_link(&path, Path::new("/devices/original/swappiness"));
    let action = vm_action(&path, "10");
    let desired = s2d_desired(&path, "10");
    let original = StoredValue::Scalar {
        value: "60".to_string(),
    };
    let engine = TransactionEngine::new(recovery_dir, "identity-generation".to_string());
    engine
        .prepare(&io, &action, &desired, &original)
        .expect("prepare original identity");
    memory.write_link(&path, Path::new("/devices/reused/swappiness"));

    let error = engine
        .prepare(&io, &action, &desired, &original)
        .expect_err("path reuse must be rejected");
    assert_eq!(error.kind, TransactionErrorKind::IdentityMismatch);
    assert_eq!(memory.read_to_string(&path).expect("target unchanged"), "60");
}
'''
count = text.count(old)
if count != 1:
    raise SystemExit(f"expected one path-reuse test, found {count}")
path.write_text(text.replace(old, new, 1), encoding="utf-8")
