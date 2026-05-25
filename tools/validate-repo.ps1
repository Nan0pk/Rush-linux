[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot

function Assert-File {
    param([string]$Path)
    $full = Join-Path $Root $Path
    if (-not (Test-Path -LiteralPath $full)) {
        throw "Missing required file: $Path"
    }
}

function Assert-Contains {
    param(
        [string]$Path,
        [string]$Pattern
    )
    $full = Join-Path $Root $Path
    $text = Get-Content -LiteralPath $full -Raw
    if ($text -notmatch $Pattern) {
        throw "Expected $Path to contain pattern: $Pattern"
    }
}

function Assert-NotContains {
    param(
        [string]$Path,
        [string]$Pattern
    )
    $full = Join-Path $Root $Path
    $text = Get-Content -LiteralPath $full -Raw
    if ($text -match $Pattern) {
        throw "Forbidden legacy/default pattern in ${Path}: $Pattern"
    }
}

$required = @(
    'Cargo.toml',
    'crates/optid/src/main.rs',
    'crates/optctl/src/main.rs',
    'config/optid/policy.toml',
    'packaging/systemd/optid.service',
    'distro/boot/cmdline.d/adaptive.conf',
    'distro/boot/uki.toml',
    'distro/kernel/default-adaptive.config',
    'distro/kernel/realtime.config',
    'distro/network/nftables.conf',
    'distro/systemd/user.slice.d/10-adaptive-accounting.conf',
    'distro/systemd/background.slice',
    'recipes/core/linux.toml',
    'recipes/core/linux-rt.toml',
    'recipes/core/optid.toml',
    'recipes/core/systemd.toml',
    'recipes/desktop/plasma-wayland.toml',
    'recipes/server/minimal.toml'
    'packaging/dbus/io.adaptive.Optid.xml'
    'packaging/dbus/io.adaptive.Optid.service'
    'distro/sysupdate/base.conf'
    'distro/sysupdate/uki.conf'
    'distro/editions/desktop.toml'
    'distro/editions/laptop.toml'
    'distro/editions/server.toml'
    'distro/editions/realtime-audio.toml'
    'benchmarks/manifest.toml'
    'tools/build-rootfs.sh'
    'tools/publish-github.ps1'
)

foreach ($file in $required) {
    Assert-File $file
}

Assert-Contains 'distro/boot/cmdline.d/adaptive.conf' 'cgroup_no_v1=all'
Assert-Contains 'distro/boot/cmdline.d/adaptive.conf' 'psi=1'
Assert-Contains 'distro/kernel/default-adaptive.config' 'CONFIG_PREEMPT_DYNAMIC=y'
Assert-Contains 'distro/kernel/default-adaptive.config' 'CONFIG_PSI=y'
Assert-Contains 'distro/kernel/default-adaptive.config' 'CONFIG_CGROUP_BPF=y'
Assert-Contains 'distro/kernel/default-adaptive.config' 'CONFIG_ZSWAP=y'
Assert-Contains 'distro/kernel/realtime.config' 'CONFIG_PREEMPT_RT=y'
Assert-Contains 'distro/network/nftables.conf' 'table inet adaptive_filter'
Assert-Contains 'packaging/systemd/optid.service' 'Conflicts=tlp.service power-profiles-daemon.service tuned.service'
Assert-Contains 'packaging/dbus/io.adaptive.Optid.xml' 'io.adaptive.Optid1'
Assert-Contains 'distro/sysupdate/uki.conf' 'Type=url-file'
Assert-Contains 'distro/editions/realtime-audio.toml' 'linux-adaptive-rt'
Assert-Contains 'benchmarks/manifest.toml' 'mixed-load-responsiveness'

$legacyChecks = @{
    'recipes/desktop/plasma-wayland.toml' = @('pulseaudio_default = true', 'legacy_x11_default = true')
    'recipes/server/minimal.toml' = @('iptables', 'cgroup_v1')
    'distro/editions/desktop.toml' = @('x11', 'pulseaudio', 'iptables')
    'distro/editions/laptop.toml' = @('x11', 'pulseaudio', 'iptables')
    'distro/editions/server.toml' = @('iptables', 'cgroup_v1')
    'distro/network/nftables.conf' = @('iptables', 'ip6tables', 'arptables', 'ebtables')
    'recipes/core/systemd.toml' = @('dual_init_supported = true', 'cgroup_v1_supported = true')
}

foreach ($entry in $legacyChecks.GetEnumerator()) {
    foreach ($pattern in $entry.Value) {
        Assert-NotContains $entry.Key ([regex]::Escape($pattern))
    }
}

Write-Host "Adaptive Linux repository validation passed."
