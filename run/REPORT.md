# Rush Linux hardware run — 2026-06-20T07:21:15Z
- user=victus root=no sudo=none apply=dry-run
## Environment
```
host=fedora kernel=7.0.12-201.fc44.x86_64 ncpu=24
virt=none
unknown
cpu=13th Gen Intel(R) Core(TM) i7-13700HX
uptime/load: 12:21:15 up 15:46,  1 user,  load average: 1.20, 0.97, 1.28
EPP_cpus=24
platform_profile=ABSENT
rapl=NA
psupply /sys/class/power_supply/ACAD=Mains
psupply /sys/class/power_supply/BAT1=Battery
psupply /sys/class/power_supply/ucsi-source-psy-USBC000:001=USB
PSI=yes
cpu_dma_latency=yes
competing_daemons:
  tuned=inactive
n/a
  power-profiles-daemon=inactive
n/a
  tlp=inactive
n/a
ambient_top:
  %CPU COMMAND
  12.5 chrome
  12.5 bash
  10.6 agy
   4.4 chrome
   3.5 chrome
```
## Environment awareness
```
env_kind=vm:none
unknown  virt=none
unknown  container=none  ci=no
real_writable_hw=no  actuation=dry-run
distro=Fedora Linux 44 (Workstation Edition)
NOTE: dry-run forced: env=vm:none
unknown, writable hardware lever=no → results NOT representative of real hardware
```
- env=vm:none
unknown real_hw=no actuation=dry-run — dry-run forced: env=vm:none
unknown, writable hardware lever=no → results NOT representative of real hardware
- RUN  deps via pacman
- FAIL deps via pacman (rc=1) — see logs/-S
### FAIL deps via pacman (rc=1)
```
error: you cannot perform this operation unless you are root.
```
- RUN  cargo build --release
- OK   cargo build --release
- RUN  cargo test
- OK   cargo test
- RUN  optid --once (smoke)
- FAIL optid --once (smoke) (rc=1) — see logs/--once
### FAIL optid --once (smoke) (rc=1)
```
optid: Permission denied (os error 13)
```
- RUN  optid apply pin sanity
- FAIL optid apply pin sanity (rc=1) — see logs/-c
### FAIL optid apply pin sanity (rc=1)
```
D-Bus server error: org.freedesktop.DBus.Error.AccessDenied: Request to own name refused by policy. Running without D-Bus.
optid: failed to read policy TOML from /usr/lib/optid/policy.toml: No such file or directory (os error 2). Using defaults.
optid: Permission denied (os error 13)
Pinned global workload class to latency-critical (offline)
{
  "timestamp": 1781940079,
  "mode": "battery",
  "workload_class": "idle",
  "workload_reason": "active usage: load=1.18, cpu_pressure=0.00, mem_pressure=0.00",
  "cpu_wakeup_latency": 100000,
  "device_resume_latency": 1000000,
  "on_ac": false,
  "battery_pct": 100,
  "thermal_c": 68.05,
  "loadavg_1": 1.18,
  "cpu_pressure": {"avg10":0.00,"avg60":0.00,"avg300":0.00,"total":413669864},
  "memory_pressure": {"avg10":0.00,"avg60":0.00,"avg300":0.00,"total":44},
  "io_pressure": {"avg10":0.00,"avg60":0.00,"avg300":0.00,"total":93396611},
  "reasons": [
    "system is on battery"
  ],
  "actions": [
    "cpu.epp=power (prefer battery life through CPU energy preference)",
    "platform.profile=low-power (request low-power platform profile)",
    "systemd.set-property background.slice CPUWeight=25 IOWeight=25 (deprioritize background services on battery)",
    "vm.sysctl /proc/sys/vm/swappiness=60 (adjust swappiness for current mode)",
    "vm.sysctl /proc/sys/vm/dirty_background_bytes=67108864 (adjust dirty background bytes for current mode)",
    "vm.sysctl /proc/sys/vm/dirty_bytes=134217728 (adjust dirty bytes for current mode)",
    "cpu_dma_latency=100000 (class=idle, floor=100000us, row=contracts.idle)"
  ]
}
```
- RUN  bench host-v2
- FAIL bench host-v2 (rc=1) — see logs//home/victus/Rush-linux/tools/bench-optid-host-v2.sh
### FAIL bench host-v2 (rc=1)
```
[1;31m[err][0m must run as root (writes sysfs, places cgroups). use sudo.
```
- RUN  bench matrix
- FAIL bench matrix (rc=1) — see logs//home/victus/Rush-linux/tools/bench-optid-matrix.sh
### FAIL bench matrix (rc=1)
```
[1;31m[err][0m must run as root. use sudo.
```
- RUN  rushbench
- FAIL rushbench (rc=1) — see logs/matrix
### FAIL rushbench (rc=1)
```
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=light, workload=psi-io ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=interactive, workload=foreground-launch ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=interactive, workload=cyclictest ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=interactive, workload=psi-cpu ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=interactive, workload=psi-io ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=latency-critical, workload=foreground-launch ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=latency-critical, workload=cyclictest ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=latency-critical, workload=psi-cpu ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=latency-critical, workload=psi-io ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=throughput, workload=foreground-launch ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=throughput, workload=cyclictest ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=throughput, workload=psi-cpu ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)

--- Running cell: class=throughput, workload=psi-io ---
Cell failed: Failed to execute optctl pin: No such file or directory (os error 2)
```
- RUN  build mkosi disk.raw
- FAIL build mkosi disk.raw (rc=127) — see logs//home/victus/Rush-linux/tools/build-mkosi-image.sh
### FAIL build mkosi disk.raw (rc=127)
```
bash: /home/victus/Rush-linux/tools/build-mkosi-image.sh: No such file or directory
```
- RUN  validate uefi boot
- OK   validate uefi boot

## Result summary
OK:   3
FAIL: 7
logs: /home/victus/Rush-linux/run/logs
evidence csv: 
- DONE — read /home/victus/Rush-linux/run/REPORT.md
