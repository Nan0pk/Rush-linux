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
    'VERSION',
    'Cargo.lock',
    'RELEASES.md',
    'PROJECT_BRIEF.md',
    'AI_CONTINUATION.md',
    'IMPLEMENTATION_STATUS.md',
    'ROADMAP.md',
    'CONTRIBUTING.md',
    'SECURITY.md',
    'README.md',
    'Cargo.toml',
    'crates/optid/src/main.rs',
    'crates/optctl/src/main.rs',
    'config/optid/policy.toml',
    'packaging/systemd/optid.service',
    'packaging/systemd/optid-apply.service',
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
    'recipes/server/minimal.toml',
    'packaging/dbus/io.adaptive.Optid.xml',
    'packaging/dbus/io.adaptive.Optid.service',
    'distro/sysupdate/base.conf',
    'distro/sysupdate/uki.conf',
    'distro/editions/desktop.toml',
    'distro/editions/laptop.toml',
    'distro/editions/server.toml',
    'distro/editions/realtime-audio.toml',
    'benchmarks/manifest.toml',
    'docs/architecture.md',
    'docs/adaptive-engine.md',
    'docs/kernel-policy.md',
    'docs/packaging-and-builds.md',
    'docs/boot-and-updates.md',
    'docs/hardware-support.md',
    'docs/testing-and-benchmarks.md',
    'docs/non-goals.md',
    'docs/validation.md',
    'docs/versioning.md',
    'docs/release-policy.md',
    'docs/release-checklist.md',
    'docs/release-plan-v1.md',
    'docs/documentation-policy.md',
    'docs/decisions/0001-systemd-cgroup-v2.md',
    'docs/decisions/0002-wayland-pipewire.md',
    'docs/decisions/0003-uki-rollback.md',
    'docs/decisions/0004-adaptive-optid.md',
    'docs/decisions/0005-avoid-obsolete-defaults.md',
    'tools/build-rootfs.sh',
    'tools/publish-github.ps1',
    'release/milestones.toml',
    'release/test-tiers.toml'
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
Assert-NotContains 'packaging/systemd/optid.service' '--apply'
Assert-Contains 'packaging/systemd/optid-apply.service' '--apply'
Assert-Contains 'packaging/dbus/io.adaptive.Optid.xml' 'io.adaptive.Optid1'
Assert-Contains 'distro/sysupdate/uki.conf' 'Type=url-file'
Assert-Contains 'distro/editions/realtime-audio.toml' 'linux-adaptive-rt'
Assert-Contains 'benchmarks/manifest.toml' 'mixed-load-responsiveness'
Assert-Contains 'VERSION' '^0\.1\.0-alpha\.0\s*$'
Assert-Contains 'RELEASES.md' '0\.1\.0-alpha\.1'
Assert-Contains 'AI_CONTINUATION.md' 'Forbidden Shortcuts'
Assert-Contains 'AI_CONTINUATION.md' 'Next Task'
Assert-Contains 'IMPLEMENTATION_STATUS.md' 'Not Yet Implemented'
Assert-Contains 'ROADMAP.md' 'v0\.1\.0-alpha\.1: Compile-Clean Core'
Assert-Contains 'CONTRIBUTING.md' 'Documentation Is Required'
Assert-Contains 'docs/testing-and-benchmarks.md' 'Docs are part of acceptance criteria'
Assert-Contains 'docs/validation.md' 'Docs are part of acceptance criteria'
Assert-Contains 'docs/versioning.md' 'MAJOR'
Assert-Contains 'docs/release-policy.md' 'Release Blockers'
Assert-Contains 'docs/release-checklist.md' 'Stable Release'
Assert-Contains 'docs/release-plan-v1.md' 'v1\.0\.0: Final Stable Release'
Assert-Contains 'docs/documentation-policy.md' 'Every non-trivial change must document'
Assert-Contains 'release/milestones.toml' 'version = "1\.0\.0"'
Assert-Contains 'release/test-tiers.toml' '\[tier\.T5\]'
Assert-Contains 'docs/decisions/0001-systemd-cgroup-v2.md' 'Status: accepted'
Assert-Contains 'docs/decisions/0004-adaptive-optid.md' 'only default runtime optimization policy owner'

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

Write-Host "Rush Linux repository validation passed."
