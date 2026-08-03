#!/usr/bin/env python3
from pathlib import Path

path = Path("crates/optid/src/reconciler/tests/systemd.rs")
text = path.read_text(encoding="utf-8")
old = '''    let outcome = reconciler
        .apply_action(&mut actuator, &action)
        .expect("apply");
    assert!(matches!(
        outcome.targets[0].readback,
        ReadbackOutcome::Mismatch { .. }
    ));
'''
new = '''    let error = reconciler
        .apply_action(&mut actuator, &action)
        .expect_err("unverified compensation must fail closed");
    let detail = error.to_string();
    assert!(detail.contains("S2D finalization failed"), "{detail}");
    assert!(detail.contains("compensation failed"), "{detail}");
'''
count = text.count(old)
if count != 1:
    raise SystemExit(f"expected one F4 mismatch assertion, found {count}")
path.write_text(text.replace(old, new, 1), encoding="utf-8")
