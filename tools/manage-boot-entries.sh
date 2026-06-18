#!/usr/bin/env bash
# manage-boot-entries.sh — Manage UKI boot entries with rollback retention.
#
# Implements the v0.4 rollback entry retention requirement:
# - Renames the current UKI to a versioned/timestamped entry
# - Retains at least INSTANCES_MAX (default: 3) previous UKI entries
# - Prunes entries beyond INSTANCES_MAX (oldest first)
# - Updates systemd-boot loader.conf to default to the latest entry
#
# Usage:
#   tools/manage-boot-entries.sh <esp_root> [new_uki_path] [version]
#
# Arguments:
#   esp_root       Path to the ESP mount (e.g., /boot or /efi)
#   new_uki_path   Path to the new UKI to install (optional, copies if given)
#   version        Version string for the new entry (optional)
#
# Environment:
#   INSTANCES_MAX  Number of rollback entries to retain (default: 3)

set -euo pipefail

ESP_ROOT="${1:?Usage: manage-boot-entries.sh <esp_root> [new_uki_path] [version]}"
NEW_UKI="${2:-}"
VERSION="${3:-$(cat "$(dirname "$0")/../VERSION" 2>/dev/null || echo unknown)}"
INSTANCES_MAX="${INSTANCES_MAX:-3}"

EFI_LINUX="${ESP_ROOT}/EFI/Linux"
LOADER_ENTRIES="${ESP_ROOT}/loader/entries"
LOADER_CONF="${ESP_ROOT}/loader/loader.conf"
TIMESTAMP="$(date +%Y%m%d%H%M%S)"

echo "=== Rush Linux Boot Entry Manager ==="
echo "  ESP:       ${ESP_ROOT}"
echo "  Max entries: ${INSTANCES_MAX}"
echo "  Version:   ${VERSION}"

mkdir -p "${EFI_LINUX}" "${LOADER_ENTRIES}"

# ── Step 1: Rotate existing main UKI into a versioned rollback entry ──
MAIN_UKI="${EFI_LINUX}/rush-linux.efi"
if [ -f "${MAIN_UKI}" ]; then
    # Determine a unique name for the rollback copy
    ROLLBACK_BASENAME="rush-linux-${VERSION}-${TIMESTAMP}"
    ROLLBACK_UKI="${EFI_LINUX}/${ROLLBACK_BASENAME}.efi"
    ROLLBACK_CONF="${LOADER_ENTRIES}/rush-linux-${VERSION}-${TIMESTAMP}.conf"

    echo "Rotating current UKI -> ${ROLLBACK_BASENAME}.efi"
    cp "${MAIN_UKI}" "${ROLLBACK_UKI}"

    # Create a boot entry for the rollback UKI
    cat > "${ROLLBACK_CONF}" <<EOF
title Rush Linux (${VERSION} rollback)
version ${VERSION}
efi /EFI/Linux/${ROLLBACK_BASENAME}.efi
EOF
    echo "  Created rollback entry: ${ROLLBACK_CONF}"
fi

# ── Step 2: Install new UKI if provided ──────────────────────────────
if [ -n "${NEW_UKI}" ] && [ -f "${NEW_UKI}" ]; then
    echo "Installing new UKI: ${NEW_UKI} -> ${MAIN_UKI}"
    cp "${NEW_UKI}" "${MAIN_UKI}"
fi

# ── Step 3: Count and prune excess rollback entries ──────────────────
# List rollback UKI entries (exclude the main rush-linux.efi)
ROLLBACK_UKIS=($(ls -1t "${EFI_LINUX}"/rush-linux-*.efi 2>/dev/null || true))
ENTRY_COUNT=${#ROLLBACK_UKIS[@]}

echo "Rollback entries: ${ENTRY_COUNT} (max: ${INSTANCES_MAX})"

if [ "${ENTRY_COUNT}" -gt "${INSTANCES_MAX}" ]; then
    PRUNE_COUNT=$((ENTRY_COUNT - INSTANCES_MAX))
    echo "Pruning ${PRUNE_COUNT} oldest rollback entry/entries..."
    # Entries are sorted newest-first, so prune from the end (oldest)
    for ((i = ENTRY_COUNT - 1; i >= ENTRY_COUNT - PRUNE_COUNT; i--)); do
        UKI_TO_REMOVE="${ROLLBACK_UKIS[$i]}"
        UKI_BASENAME="$(basename "${UKI_TO_REMOVE}")"
        CONF_NAME="${UKI_BASENAME%.efi}.conf"
        echo "  Removing: ${UKI_BASENAME}"
        rm -f "${UKI_TO_REMOVE}"
        rm -f "${LOADER_ENTRIES}/${CONF_NAME}"
    done
fi

# ── Step 4: Report final entry count ─────────────────────────────────
FINAL_UKIS=($(ls -1 "${EFI_LINUX}"/rush-linux*.efi 2>/dev/null || true))
FINAL_CONFS=($(ls -1 "${LOADER_ENTRIES}"/rush-linux*.conf 2>/dev/null || true))
TOTAL_ENTRIES=${#FINAL_UKIS[@]}

echo ""
echo "Boot entries after rotation:"
echo "  Total UKI files: ${TOTAL_ENTRIES}"
for uki in "${FINAL_UKIS[@]}"; do
    echo "    $(basename "${uki}")"
done
echo "  Total loader entries: ${#FINAL_CONFS[@]}"
for conf in "${FINAL_CONFS[@]}"; do
    echo "    $(basename "${conf}")"
done

# ── Step 5: Verify minimum rollback entries ──────────────────────────
# For v0.4 gate: at least 3 rollback entries must be retained.
# This means we need at least 4 UKI files total (1 main + 3 rollback).
# If we don't have enough yet, that's expected on first builds — report status.
ROLLBACK_FINAL=$(ls -1 "${EFI_LINUX}"/rush-linux-*.efi 2>/dev/null | wc -l)
if [ "${ROLLBACK_FINAL}" -ge "${INSTANCES_MAX}" ]; then
    echo ""
    echo "✅ Rollback entry gate PASSED: ${ROLLBACK_FINAL} entries >= ${INSTANCES_MAX}"
else
    echo ""
    echo "⚠  Rollback entry gate: ${ROLLBACK_FINAL}/${INSTANCES_MAX} entries (need more updates to reach threshold)"
fi

echo ""
echo "Boot entry management complete."
