#!/usr/bin/env python3
"""
collect-hardware-inventory.py — privacy-safe hardware inventory for Rush Linux.

Collects ONLY non-identifying hardware capabilities. Explicitly does NOT
collect: serial numbers, MAC addresses, UUIDs, hostnames, usernames, IP
addresses, Wi-Fi network names, or home-directory paths.

Usage:
    python3 tools/collect-hardware-inventory.py [--output inventory.json]

The output is a JSON file suitable for inclusion in a hardware evidence
bundle. All fields are reviewed for privacy before writing.

Exit codes:
    0  — inventory collected successfully
    1  — collection failed (missing /sys, /proc, or tools)
    2  — privacy violation detected (a field contained redactable data)
"""

import json
import os
import base64
import platform
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

# Fields that are NEVER collected. If any of these appear in a command's
# output, the collector refuses to write them and exits with code 2.
REDACTED_PATTERNS = [
    # Serial numbers (DMI, USB, PCI)
    r"(?i)serial[_=: ]+([A-Z0-9]{6,})",
    # MAC addresses
    r"(?i)([0-9a-f]{2}[:]){5}[0-9a-f]{2}",
    # UUIDs
    r"(?i)uuid[_=: ]+([0-9a-f-]{36})",
    # IP addresses (IPv4) — but not version numbers like 6.1.0-49
    r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
    # IPv6 — must have at least 3 colon-separated groups of 4 hex digits
    # and not be a time (HH:MM:SS only has 3 groups of 2 digits)
    r"(?i)\b(?:[0-9a-f]{1,4}:){3,7}[0-9a-f]{1,4}\b",
    # Hostnames (labels followed by domain-like suffix)
    r"(?i)hostname[_=: ]+\S+",
    # SSID
    r"(?i)ssid[_=: ]+\S+",
    # Home directory paths
    r"/home/[a-zA-Z0-9_]+",
    r"/Users/[a-zA-Z0-9_]+",
]


def run_cmd(cmd: list[str], timeout: int = 5) -> str:
    """Run a command and return stdout, or empty string on failure."""
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return r.stdout.strip() if r.returncode == 0 else ""
    except (OSError, subprocess.TimeoutExpired):
        return ""


def check_redaction(text: str) -> list[str]:
    """Return matched rule names without echoing sensitive matched values."""
    matches = []
    for index, pattern in enumerate(REDACTED_PATTERNS, start=1):
        if re.search(pattern, text):
            matches.append(f"privacy rule {index}")
    return matches


def safe_read(path: str) -> str:
    """Read a sysfs/procfs file safely. Returns empty string on failure."""
    try:
        return Path(path).read_text().strip()
    except (OSError, FileNotFoundError):
        return ""


def collect_cpu() -> dict:
    """Collect CPU model and topology (no serial numbers)."""
    info = {}
    # Model name — parse properly to avoid grabbing the next field
    cpuinfo = safe_read("/proc/cpuinfo")
    model = "unknown"
    for line in cpuinfo.split("\n"):
        if line.startswith("model name"):
            # "model name      : Intel(R) Xeon(R)..."
            model = line.split(":", 1)[1].strip()
            break
    info["model"] = model
    # Topology
    cpu_dirs = [d for d in os.listdir("/sys/devices/system/cpu/") if d.startswith("cpu") and d[3:].isdigit()]
    online = [d for d in cpu_dirs if safe_read(f"/sys/devices/system/cpu/{d}/online") != "0"]
    packages = set()
    cores = set()
    for cpu in online:
        topology = f"/sys/devices/system/cpu/{cpu}/topology"
        package = safe_read(f"{topology}/physical_package_id")
        core = safe_read(f"{topology}/core_id")
        if package:
            packages.add(package)
        if package and core:
            cores.add((package, core))
    info["online_cpus"] = len(online)
    info["sockets"] = len(packages) or "unknown"
    info["physical_cores"] = len(cores) or "unknown"
    info["threads_per_core"] = round(len(online) / len(cores), 1) if cores else "unknown"
    # Frequency range
    info["freq_min_khz"] = safe_read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq") or "unknown"
    info["freq_max_khz"] = safe_read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq") or "unknown"
    # EPP
    epp = safe_read("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
    info["epp_available"] = safe_read("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences") or "unknown"
    info["epp_current"] = epp or "unknown"
    info["epp_supported"] = bool(epp)
    return info


def collect_gpu() -> list:
    """Collect GPU models and drivers (no serials)."""
    gpus = []
    # Use lspci (filter to VGA/3D controllers)
    lspci = run_cmd(["lspci", "-nn", "-d", "::0300"]) + "\n" + run_cmd(["lspci", "-nn", "-d", "::0380"])
    for line in lspci.strip().split("\n"):
        if line.strip():
            # Extract just the device description, not the PCI address
            # (PCI address is not sensitive but is machine-specific)
            parts = line.split(":", 2)
            if len(parts) >= 3:
                desc = parts[2].strip()
                # Remove the [hex:hex] device IDs (keep description only)
                desc = re.sub(r"\[[0-9a-f]{4}:[0-9a-f]{4}\]", "", desc).strip()
                address = line.split()[0]
                if address.count(":") == 1:
                    address = f"0000:{address}"
                driver_link = Path("/sys/bus/pci/devices") / address / "driver"
                try:
                    driver = driver_link.resolve(strict=True).name
                except OSError:
                    driver = "unknown"
                gpus.append({"model": desc, "driver": driver})
    return gpus


def collect_ram() -> dict:
    """Collect RAM capacity only."""
    meminfo = safe_read("/proc/meminfo")
    total_kb = 0
    for line in meminfo.split("\n"):
        if line.startswith("MemTotal:"):
            total_kb = int(line.split()[1])
            break
    return {"total_gb": round(total_kb / 1024 / 1024, 1) if total_kb else "unknown"}


def collect_kernel_os() -> dict:
    """Collect kernel and OS version (no hostname, no build host)."""
    info = {}
    info["kernel"] = safe_read("/proc/sys/kernel/osrelease")
    # Do NOT collect /proc/version — it contains the kernel build hostname
    # (e.g., "root@i22d13141.eu95sqa") which is machine-unique.
    # OS release info
    os_release = {}
    for line in safe_read("/etc/os-release").split("\n"):
        if "=" in line:
            k, v = line.split("=", 1)
            os_release[k] = v.strip('"')
    info["os_name"] = os_release.get("NAME", "unknown")
    info["os_version"] = os_release.get("VERSION_ID", "unknown")
    info["os_pretty"] = os_release.get("PRETTY_NAME", "unknown")
    return info


def collect_dmi() -> dict:
    """Collect DMI board vendor/model ONLY (no serials, no UUIDs)."""
    info = {}
    # Only board vendor and board name — NOT board_serial, product_uuid, etc.
    info["board_vendor"] = safe_read("/sys/class/dmi/id/board_vendor") or "unknown"
    info["board_name"] = safe_read("/sys/class/dmi/id/board_name") or "unknown"
    info["board_version"] = safe_read("/sys/class/dmi/id/board_version") or "unknown"
    # sys_vendor is the OEM (e.g., "HP") — not unique
    info["sys_vendor"] = safe_read("/sys/class/dmi/id/sys_vendor") or "unknown"
    # product_name is the model (e.g., "Victus by HP 16 Gaming Laptop...")
    info["product_name"] = safe_read("/sys/class/dmi/id/product_name") or "unknown"
    return info


def collect_battery() -> dict:
    """Collect battery design/full capacity and status (no serials)."""
    info = {"present": False, "ac_online": None}
    bat_dir = Path("/sys/class/power_supply")
    if not bat_dir.exists():
        return info
    ac_online = None
    for supply in bat_dir.iterdir():
        if safe_read(str(supply / "type")) == "Mains":
            online = safe_read(str(supply / "online"))
            if online in ("0", "1"):
                ac_online = online == "1"
                break
    info["ac_online"] = ac_online
    for bat in bat_dir.iterdir():
        if not bat.name.startswith("BAT"):
            continue
        info["present"] = True
        info["name"] = bat.name
        info["status"] = safe_read(str(bat / "status")) or "unknown"
        design = safe_read(str(bat / "energy_full_design"))
        full = safe_read(str(bat / "energy_full"))
        now = safe_read(str(bat / "energy_now"))
        info["design_capacity_uwh"] = int(design) if design else 0
        info["full_capacity_uwh"] = int(full) if full else 0
        info["current_capacity_uwh"] = int(now) if now else 0
        info["health_pct"] = round(int(full) / int(design) * 100, 1) if design and full else 0
        info["technology"] = safe_read(str(bat / "technology")) or "unknown"
        break  # only first battery
    return info


def collect_platform_profile() -> dict:
    """Collect supported platform_profile values."""
    info = {"supported": False}
    pp = Path("/sys/firmware/acpi/platform_profile")
    if pp.exists():
        info["supported"] = True
        info["available"] = safe_read(str(pp.parent / "platform_profile_choices")) or "unknown"
        info["current"] = safe_read(str(pp)) or "unknown"
    return info


def collect_rapl() -> dict:
    """Collect RAPL/powercap availability (no measurement data)."""
    info = {"available": False}
    rapl = Path("/sys/class/powercap/intel-rapl")
    if rapl.exists():
        info["available"] = True
        packages = list(rapl.iterdir())
        info["package_count"] = len([p for p in packages if p.name.startswith("intel-rapl:")])
    return info


def collect_storage() -> dict:
    """Collect storage controller/device class (no serials)."""
    info = {"devices": []}
    # Use lsblk with --noheadings and filter to name/type/size only
    lsblk = run_cmd(["lsblk", "-d", "-o", "NAME,TYPE,SIZE,ROTA,TRAN", "--json"])
    if lsblk:
        try:
            data = json.loads(lsblk)
            for dev in data.get("blockdevices", []):
                info["devices"].append({
                    "name": dev.get("name", "unknown"),
                    "type": dev.get("type", "unknown"),
                    "size": dev.get("size", "unknown"),
                    "rotational": dev.get("rota", "unknown"),
                    "transport": dev.get("tran", "unknown"),
                })
        except json.JSONDecodeError:
            pass
    return info


def collect_pci_modaliases() -> list:
    """Collect relevant PCI modaliases (no serials)."""
    modaliases = []
    # Only collect modaliases for power-relevant device classes:
    # - Display/VGA (0300, 0380)
    # - Network (0200)
    # - Storage (0100, 0106, 0108)
    for class_id in ["0300", "0380", "0200", "0100", "0106", "0108"]:
        lspci = run_cmd(["lspci", "-nn", "-d", f"::{class_id}"])
        for line in lspci.strip().split("\n"):
            if line.strip():
                # Extract just the description, not the PCI address
                parts = line.split(":", 2)
                if len(parts) >= 3:
                    desc = parts[2].strip()
                    desc = re.sub(r"\[[0-9a-f]{4}:[0-9a-f]{4}\]", "", desc).strip()
                    modaliases.append({"class": class_id, "description": desc})
    return modaliases


def collect_pm_owners() -> dict:
    """Detect current power-management daemon owners."""
    owners = {}
    for daemon in ["power-profiles-daemon", "tlp", "tuned", "thermald"]:
        # Check if the service is active
        r = subprocess.run(
            ["systemctl", "is-active", daemon],
            capture_output=True, text=True, timeout=3
        )
        status = r.stdout.strip()
        if status == "active":
            owners[daemon] = "active"
        elif status == "inactive":
            owners[daemon] = "inactive"
        else:
            owners[daemon] = status or "unknown"
    return owners


def collect_power_profile() -> dict:
    """Collect the current distro power profile without changing it."""
    current = run_cmd(["powerprofilesctl", "get"])
    return {"available": bool(current), "current": current or "unknown"}


def _windows_inventory_from_payload(payload: dict) -> dict:
    """Map an allow-listed CIM payload into the cross-platform inventory shape."""
    cs = payload.get("computer_system") or {}
    cpu = payload.get("processor") or {}
    os_info = payload.get("operating_system") or {}
    battery = payload.get("battery") or {}
    design = payload.get("battery_design_capacity") or 0
    full = payload.get("battery_full_capacity") or 0
    try:
        design = int(design)
    except (TypeError, ValueError):
        design = 0
    try:
        full = int(full)
    except (TypeError, ValueError):
        full = 0
    return {
        "schema": "rush-hardware-inventory-v1",
        "collected_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "cpu": {
            "model": cpu.get("Name", "unknown"),
            "online_cpus": cpu.get("NumberOfLogicalProcessors", "unknown"),
            "physical_cores": cpu.get("NumberOfCores", "unknown"),
            "sockets": 1,
            "threads_per_core": (
                round(cpu.get("NumberOfLogicalProcessors", 0) / cpu.get("NumberOfCores", 1), 1)
                if cpu.get("NumberOfCores") else "unknown"
            ),
        },
        "gpu": [
            {"model": item.get("Name", "unknown"), "adapter_ram_bytes": item.get("AdapterRAM", 0)}
            for item in (payload.get("video_controllers") or [])
        ],
        "ram": {
            "total_gb": round(int(cs.get("TotalPhysicalMemory", 0)) / 1024**3, 1)
            if cs.get("TotalPhysicalMemory") else "unknown"
        },
        "kernel_os": {
            "kernel": os_info.get("Version", "unknown"),
            "os_name": os_info.get("Caption", "unknown"),
            "os_version": os_info.get("Version", "unknown"),
            "os_pretty": os_info.get("Caption", "unknown"),
            "architecture": os_info.get("OSArchitecture", "unknown"),
        },
        "dmi": {
            "sys_vendor": cs.get("Manufacturer", "unknown"),
            "product_name": cs.get("Model", "unknown"),
            "board_vendor": "not-collected-on-windows",
            "board_name": "not-collected-on-windows",
            "board_version": "not-collected-on-windows",
        },
        "battery": {
            "present": bool(battery),
            "status_code": battery.get("BatteryStatus", "unknown"),
            "charge_percent": battery.get("EstimatedChargeRemaining", "unknown"),
            "design_capacity_mwh": design,
            "full_capacity_mwh": full,
            "health_pct": round(full / design * 100, 1) if design and full else 0,
        },
        "platform_profile": {"supported": False},
        "rapl": {"available": False},
        "storage": {
            "devices": [
                {
                    "media_type": item.get("MediaType", "unknown"),
                    "interface_type": item.get("InterfaceType", "unknown"),
                    "size_bytes": item.get("Size", 0),
                }
                for item in (payload.get("disk_drives") or [])
            ]
        },
        "pci_modaliases": [],
        "pm_owners": {},
        "power_profile": {"available": False, "current": "unknown"},
        "initial_thermal": {"available": False, "sensor_count": 0, "maximum_celsius": None},
    }


def collect_windows_inventory() -> dict:
    """Collect only explicitly allow-listed, non-identifying CIM properties."""
    script = r'''
$ErrorActionPreference = 'Stop'
$cs = Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model,TotalPhysicalMemory
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1 Name,NumberOfCores,NumberOfLogicalProcessors
$gpu = @(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM)
$disk = @(Get-CimInstance Win32_DiskDrive | Select-Object MediaType,InterfaceType,Size)
$os = Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,OSArchitecture
$battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1 BatteryStatus,EstimatedChargeRemaining
$design = 0
$full = 0
try { $design = (Get-CimInstance -Namespace root\wmi -ClassName BatteryStaticData -ErrorAction Stop | Select-Object -First 1 -ExpandProperty DesignedCapacity) } catch {}
try { $full = (Get-CimInstance -Namespace root\wmi -ClassName BatteryFullChargedCapacity -ErrorAction Stop | Select-Object -First 1 -ExpandProperty FullChargedCapacity) } catch {}
[ordered]@{
  computer_system = $cs
  processor = $cpu
  video_controllers = $gpu
  disk_drives = $disk
  operating_system = $os
  battery = $battery
  battery_design_capacity = $design
  battery_full_capacity = $full
} | ConvertTo-Json -Depth 6 -Compress
'''
    encoded = base64.b64encode(script.encode("utf-16le")).decode("ascii")
    powershell = "powershell.exe" if platform.system().lower() == "windows" else "powershell"
    raw = run_cmd([powershell, "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], timeout=30)
    if not raw:
        raise RuntimeError("allow-listed Windows CIM inventory query failed")
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise RuntimeError("Windows CIM inventory returned invalid JSON") from exc
    return _windows_inventory_from_payload(payload)


def collect_thermal() -> dict:
    """Collect a coarse initial temperature summary, without device IDs."""
    temperatures = []
    for path in Path("/sys/class/hwmon").glob("hwmon*/temp*_input"):
        raw = safe_read(str(path))
        try:
            value = int(raw) / 1000
        except ValueError:
            continue
        if -20 <= value <= 150:
            temperatures.append(value)
    return {
        "available": bool(temperatures),
        "sensor_count": len(temperatures),
        "maximum_celsius": max(temperatures) if temperatures else None,
    }


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Privacy-safe hardware inventory")
    parser.add_argument("--output", "-o", default="hardware-inventory.json",
                        help="Output JSON file (default: hardware-inventory.json)")
    parser.add_argument("--check-only", action="store_true",
                        help="Run privacy checks on collection without writing output")
    args = parser.parse_args()

    print("Collecting privacy-safe hardware inventory...")
    print("This collector does NOT gather: serial numbers, MAC addresses,")
    print("UUIDs, hostnames, usernames, IP addresses, SSIDs, or home paths.")
    print()

    if platform.system().lower() == "windows":
        try:
            inventory = collect_windows_inventory()
        except RuntimeError as exc:
            print(f"Inventory collection failed: {exc}", file=sys.stderr)
            sys.exit(1)
    else:
        inventory = {
            "schema": "rush-hardware-inventory-v1",
            "collected_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "cpu": collect_cpu(),
            "gpu": collect_gpu(),
            "ram": collect_ram(),
            "kernel_os": collect_kernel_os(),
            "dmi": collect_dmi(),
            "battery": collect_battery(),
            "platform_profile": collect_platform_profile(),
            "rapl": collect_rapl(),
            "storage": collect_storage(),
            "pci_modaliases": collect_pci_modaliases(),
            "pm_owners": collect_pm_owners(),
            "power_profile": collect_power_profile(),
            "initial_thermal": collect_thermal(),
        }

    # Privacy scan: check the entire JSON for redactable patterns
    inventory_str = json.dumps(inventory, indent=2)
    violations = check_redaction(inventory_str)
    if violations:
        print("PRIVACY VIOLATION DETECTED:")
        for v in violations:
            print(f"  {v}")
        print("\nRefusing to write inventory. Fix the collector or redact manually.")
        sys.exit(2)

    if args.check_only:
        print("Privacy check passed. No output written (--check-only).")
        return

    output_path = Path(args.output)
    output_path.write_text(inventory_str)
    print(f"Inventory written to: {output_path}")
    print(f"Size: {output_path.stat().st_size} bytes")
    print()
    print("Summary:")
    print(f"  CPU: {inventory['cpu']['model']}")
    print(f"  OS: {inventory['kernel_os']['os_pretty']}")
    print(f"  Battery: {'present' if inventory['battery']['present'] else 'absent'}")
    print(f"  Platform profile: {'supported' if inventory['platform_profile']['supported'] else 'not supported'}")
    print(f"  RAPL: {'available' if inventory['rapl']['available'] else 'not available'}")
    print(f"  PM owners: {inventory['pm_owners']}")


if __name__ == "__main__":
    main()
