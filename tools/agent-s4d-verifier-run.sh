#!/usr/bin/env bash
set -euo pipefail

: "${IMPLEMENTATION_HEAD:?}"
: "${PROOF_LOG:?}"

exec > >(tee -a "$PROOF_LOG") 2>&1

VERIFIER_HEAD=$(git rev-parse HEAD)
echo "VERIFIER_HEAD=$VERIFIER_HEAD" >> "$GITHUB_ENV"
echo "implementation_head=$IMPLEMENTATION_HEAD"
echo "verifier_head=$VERIFIER_HEAD"
git merge-base --is-ancestor "$IMPLEMENTATION_HEAD" "$VERIFIER_HEAD"

expected=$(mktemp)
actual=$(mktemp)
printf '%s\n' \
  '.github/workflows/agent-s4d-verifier.yml' \
  'tools/agent-s4d-verify.py' \
  'tools/agent-s4d-verifier-run.sh' | sort > "$expected"
git diff --name-only "$IMPLEMENTATION_HEAD".."$VERIFIER_HEAD" | sort > "$actual"
diff -u "$expected" "$actual"

python3 tools/validate-current-work.py
set +e
python3 tools/validate-optid-packages.py > /tmp/pre-s4d-validator.log 2>&1
validator_status=$?
set -e
cat /tmp/pre-s4d-validator.log
test "$validator_status" -ne 0
python3 - <<'PY'
import re
from pathlib import Path
text = Path('/tmp/pre-s4d-validator.log').read_text(encoding='utf-8')
stale = re.findall(
    r'(?m)^\s*(?:-\s*)?([A-Z][A-Z0-9]*): verification receipt is stale',
    text,
)
expected = ['F1', 'F2', 'F3', 'F4', 'D0', 'S2D', 'S3D']
assert sorted(stale) == sorted(expected), (stale, text)
assert text.count('verification receipt is stale') == len(expected), text
print('pre_transition_stale_receipts=' + ','.join(stale))
PY
python3 tools/render-frontpage.py --check
git diff --check
echo "immutable_source_binding=pass"

cargo fmt --all -- --check
cargo check -p optid --all-targets --all-features
cargo clippy -p optid --all-targets --all-features -- -D warnings
echo "compile_and_clippy=pass"

python3 - <<'PY' > /tmp/s4d-affected-acceptance.tsv
import tomllib
from pathlib import Path
ledger = tomllib.loads(Path('docs/plans/optid-package-status.toml').read_text())
packages = {item['id']: item for item in ledger['package']}
for package_id in ('F1', 'F2', 'F3', 'F4', 'D0', 'S2D', 'S3D'):
    tests = packages[package_id].get('acceptance_tests', {})
    for test_name in tests.values():
        print(f'{package_id}\t{test_name}')
for test_name in (
    's4d_operation_type_mismatch_is_rejected',
    's4d_preopened_descriptor_survives_permission_tightening',
    's4d_symlink_path_replacement_is_rejected',
    's4d_stale_identity_is_rejected',
    's4d_removed_device_fails_closed',
    's4d_capability_descriptors_are_cloexec',
    's4d_topology_change_is_debounced',
    's4d_cold_rebuild_opens_fresh_identity',
    's4d_apply_unit_restarts_only_through_supervised_recovery_graph',
    's4d_startup_seals_before_any_worker_or_dbus_input',
    's4d_topology_rebuild_hands_back_before_status_75',
):
    print(f'S4D\t{test_name}')
PY

while IFS=$'\t' read -r package_id test_name; do
  echo "acceptance_package=$package_id test=$test_name"
  output=$(mktemp)
  cargo test -p optid --color never "$test_name" -- --nocapture 2>&1 | tee "$output"
  grep -F "$test_name ... ok" "$output"
done < /tmp/s4d-affected-acceptance.tsv
echo "all_affected_acceptance_mappings=pass"

cargo test -p optid
cargo test --workspace --all-features
echo "full_regression=pass"

cargo test -p optid --features experimental-capability-sealing \
  --bin optid-capability-seal-test
cargo test -p optid --features experimental-capability-sealing \
  --test capability_sealing_cli
cargo build -p optid --features experimental-capability-sealing \
  --bin optid-capability-seal-test --bin optid --bin optid-recover
sudo install -D -m 0755 target/debug/optid-capability-seal-test \
  /usr/local/libexec/optid-capability-seal-test

KERNEL_RELEASE=$(uname -r)
echo "KERNEL_RELEASE=$KERNEL_RELEASE" >> "$GITHUB_ENV"
echo "kernel_uname=$(uname -a)"
echo "kernel_release=$KERNEL_RELEASE"
if [[ -r /sys/kernel/security/lsm ]]; then
  echo "active_lsms=$(cat /sys/kernel/security/lsm)"
else
  echo "active_lsms=unavailable"
fi
/usr/local/libexec/optid-capability-seal-test --probe

python3 - <<'PY'
import os
import re
from pathlib import Path
text = Path(os.environ['PROOF_LOG']).read_text(encoding='utf-8')
matches = re.findall(r'(?i)(?:landlock[_ ]abi)[\s"=:]+([0-9]+)', text)
if not matches:
    raise SystemExit('could not parse Landlock ABI from proof log')
with open(os.environ['GITHUB_ENV'], 'a', encoding='utf-8') as handle:
    handle.write(f'LANDLOCK_ABI={matches[-1]}\n')
print(f'parsed_landlock_abi={matches[-1]}')
PY

echo "RECERTIFIED_AT=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$GITHUB_ENV"
sudo install -d -m 0755 /usr/libexec /usr/share/man/man8
sudo install -m 0755 target/debug/optid /usr/libexec/optid
sudo install -m 0755 target/debug/optid-recover /usr/libexec/optid-recover
printf '.TH OPTID 8\n.SH NAME\noptid \\- Rush Linux optimization daemon\n' \
  | gzip -9 | sudo tee /usr/share/man/man8/optid.8.gz >/dev/null
systemd-analyze verify packaging/systemd/optid-apply.service
systemd-analyze verify packaging/systemd/optid-recover.service
systemd-analyze verify packaging/systemd/optid-capability-seal-test.service

# D0: prove status-75 cold rebuild and non-75 restart suppression.
sudo install -D -m 0644 packaging/systemd/optid-capability-seal-test.service \
  /etc/systemd/system/optid-capability-seal-test.service
sudo touch /run/optid-capability-seal-test-enabled
sudo rm -rf /run/optid-capability-seal-test
sudo systemctl daemon-reload
sudo systemctl reset-failed optid-capability-seal-test.service || true
d0_started=$(date --iso-8601=seconds)
sudo systemctl start optid-capability-seal-test.service
for _ in $(seq 1 30); do
  active=$(systemctl show optid-capability-seal-test.service -p ActiveState --value)
  result=$(systemctl show optid-capability-seal-test.service -p Result --value)
  [[ "$active" == inactive && "$result" == success ]] && break
  [[ "$active" == failed ]] && break
  sleep 1
done
sudo journalctl --sync
sudo journalctl -u optid-capability-seal-test.service \
  --since "$d0_started" --no-pager -o cat | tee /tmp/d0-supervisor-success.log
test "$(systemctl show optid-capability-seal-test.service -p ActiveState --value)" = inactive
test "$(systemctl show optid-capability-seal-test.service -p Result --value)" = success
test "$(systemctl show optid-capability-seal-test.service -p ExecMainStatus --value)" = 0
test "$(grep -cF 'Scheduled restart job, restart counter is at 1.' /tmp/d0-supervisor-success.log)" -eq 1
test "$(grep -cF 'recovery step completed:' /tmp/d0-supervisor-success.log)" -eq 2
test "$(grep -cF 'capability-sealing proof passed (8 checks)' /tmp/d0-supervisor-success.log)" -eq 2
echo "D0_status_75_cold_rebuild=pass"

sudo mkdir -p /run/systemd/system/optid-capability-seal-test.service.d
sudo tee /run/systemd/system/optid-capability-seal-test.service.d/non75.conf >/dev/null <<'EOF'
[Service]
ExecStart=
ExecStart=/bin/sh -c 'exit 1'
EOF
sudo systemctl daemon-reload
sudo systemctl reset-failed optid-capability-seal-test.service || true
d0_failure_started=$(date --iso-8601=seconds)
set +e
sudo systemctl start optid-capability-seal-test.service
set -e
sleep 3
sudo journalctl --sync
sudo journalctl -u optid-capability-seal-test.service \
  --since "$d0_failure_started" --no-pager -o cat | tee /tmp/d0-non75.log
test "$(systemctl show optid-capability-seal-test.service -p Result --value)" = exit-code
test "$(systemctl show optid-capability-seal-test.service -p ExecMainStatus --value)" = 1
test "$(systemctl show optid-capability-seal-test.service -p NRestarts --value)" = 0
! grep -qF 'Scheduled restart job' /tmp/d0-non75.log
echo "D0_non_75_restart_suppression=pass"
sudo rm -rf /run/systemd/system/optid-capability-seal-test.service.d
sudo systemctl daemon-reload

# S3D: use the real merged unit graph with test-only executables and hosted-VM
# sandbox drop-ins. Prove recovery runs before both daemon generations and a
# failed required recovery starts no daemon and creates no restart loop.
sudo install -D -m 0644 packaging/systemd/optid-apply.service \
  /etc/systemd/system/optid-apply.service
sudo install -D -m 0644 packaging/systemd/optid-recover.service \
  /etc/systemd/system/optid-recover.service
sudo install -d -m 0755 /usr/local/libexec
cat > /tmp/s4d-recert-recover <<'SH'
#!/usr/bin/env bash
set -euo pipefail
root=/var/lib/optid/s4d-recert
count=0
[[ -f "$root/recover-count" ]] && IFS= read -r count < "$root/recover-count"
count=$((count + 1))
printf '%s\n' "$count" > "$root/recover-count"
printf 'recover-%s\n' "$count" >> "$root/order.log"
[[ ! -e "$root/force-recovery-failure" ]] || exit 78
SH
cat > /tmp/s4d-recert-apply <<'SH'
#!/usr/bin/env bash
set -euo pipefail
root=/var/lib/optid/s4d-recert
count=0
[[ -f "$root/apply-count" ]] && IFS= read -r count < "$root/apply-count"
count=$((count + 1))
printf '%s\n' "$count" > "$root/apply-count"
printf 'apply-%s\n' "$count" >> "$root/order.log"
[[ "$count" -ne 1 ]] || exit 1
exec /usr/bin/sleep 30
SH
sudo install -m 0755 /tmp/s4d-recert-recover /usr/local/libexec/s4d-recert-recover
sudo install -m 0755 /tmp/s4d-recert-apply /usr/local/libexec/s4d-recert-apply
sudo install -d -m 0755 \
  /etc/systemd/system/optid-apply.service.d \
  /etc/systemd/system/optid-recover.service.d
sudo tee /etc/systemd/system/optid-apply.service.d/verification.conf >/dev/null <<'EOF'
[Service]
Type=simple
ExecStart=
ExecStart=/usr/local/libexec/s4d-recert-apply
WatchdogSec=0
TimeoutStartSec=10s
RestartSec=1s
ReadWritePaths=
ReadWritePaths=/var/lib/optid /run/optid
ProtectKernelTunables=no
ProcSubset=all
ProtectProc=default
SystemCallFilter=
EOF
sudo tee /etc/systemd/system/optid-recover.service.d/verification.conf >/dev/null <<'EOF'
[Service]
ExecStart=
ExecStart=/usr/local/libexec/s4d-recert-recover
ReadWritePaths=
ReadWritePaths=/var/lib/optid /run/optid
ProtectKernelTunables=no
ProcSubset=all
ProtectProc=default
SystemCallFilter=
EOF
sudo systemctl daemon-reload
sudo systemctl stop optid-apply.service optid-recover.service || true
sudo rm -rf /var/lib/optid/s4d-recert
sudo install -d -m 0755 /var/lib/optid/s4d-recert
sudo systemctl reset-failed optid-apply.service optid-recover.service || true
s3d_started=$(date --iso-8601=seconds)
sudo systemctl start optid-apply.service || true
for _ in $(seq 1 20); do
  apply_count=$(cat /var/lib/optid/s4d-recert/apply-count 2>/dev/null || echo 0)
  recover_count=$(cat /var/lib/optid/s4d-recert/recover-count 2>/dev/null || echo 0)
  active=$(systemctl show optid-apply.service -p ActiveState --value)
  [[ "$apply_count" -eq 2 && "$recover_count" -eq 2 && "$active" == active ]] && break
  sleep 1
done
test "$(cat /var/lib/optid/s4d-recert/apply-count)" -eq 2
test "$(cat /var/lib/optid/s4d-recert/recover-count)" -eq 2
diff -u <(printf 'recover-1\napply-1\nrecover-2\napply-2\n') \
  /var/lib/optid/s4d-recert/order.log
sudo journalctl --sync
sudo journalctl -u optid-recover.service -u optid-apply.service \
  --since "$s3d_started" --no-pager -o short-monotonic \
  | tee /tmp/s3d-recert-restart.log
test "$(grep -cF 'Scheduled restart job, restart counter is at 1.' /tmp/s3d-recert-restart.log)" -eq 1
echo "S3D_recovery_before_replacement=pass"

sudo systemctl stop optid-apply.service
sudo rm -rf /var/lib/optid/s4d-recert
sudo install -d -m 0755 /var/lib/optid/s4d-recert
sudo touch /var/lib/optid/s4d-recert/force-recovery-failure
sudo systemctl reset-failed optid-apply.service optid-recover.service || true
s3d_failure_started=$(date --iso-8601=seconds)
set +e
sudo systemctl start optid-apply.service
s3d_start_status=$?
set -e
sleep 4
sudo journalctl --sync
test "$s3d_start_status" -ne 0
test "$(cat /var/lib/optid/s4d-recert/recover-count)" -eq 1
test ! -e /var/lib/optid/s4d-recert/apply-count
test "$(systemctl show optid-apply.service -p NRestarts --value)" -eq 0
test "$(systemctl show optid-apply.service -p ActiveState --value)" = inactive
sudo journalctl -u optid-recover.service -u optid-apply.service \
  --since "$s3d_failure_started" --no-pager -o short-monotonic \
  | tee /tmp/s3d-recert-failure.log
! grep -qF 'Scheduled restart job' /tmp/s3d-recert-failure.log
echo "S3D_failed_recovery_blocks_daemon=pass"

cat /tmp/d0-supervisor-success.log >> "$PROOF_LOG"
cat /tmp/d0-non75.log >> "$PROOF_LOG"
cat /tmp/s3d-recert-restart.log >> "$PROOF_LOG"
cat /tmp/s3d-recert-failure.log >> "$PROOF_LOG"
echo "shared_surface_live_proofs=pass"
