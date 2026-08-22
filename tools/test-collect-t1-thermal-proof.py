#!/usr/bin/env python3
"""Tests for the read-only T1 thermal proof collector."""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

_TOOLS = Path(__file__).resolve().parent
_ROOT = _TOOLS.parent


def _load_collector():
    spec = importlib.util.spec_from_file_location(
        "collect_t1_thermal_proof", _TOOLS / "collect-t1-thermal-proof.py"
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


collector = _load_collector()


def _write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def test_collect_observation_is_sorted_and_privacy_safe(tmp_path: Path) -> None:
    hwmon = tmp_path / "sys/class/hwmon/hwmon7"
    _write(hwmon / "name", "coretemp\n")
    _write(hwmon / "temp2_input", "71000\n")
    _write(hwmon / "temp2_label", "Core 1\n")
    _write(hwmon / "temp1_input", "65000\n")
    _write(hwmon / "temp1_label", "Package id 0\n")
    _write(hwmon / "temp1_crit", "100000\n")
    _write(hwmon / "fan1_input", "3200\n")

    zone = tmp_path / "sys/class/thermal/thermal_zone3"
    _write(zone / "type", "x86_pkg_temp\n")
    _write(zone / "temp", "66000\n")

    observation = collector.collect_observation(tmp_path, 1)
    ids = [item["stable_id"] for item in observation["temperatures"]]
    assert ids == sorted(ids)
    assert observation["temperatures"][0]["plausible"] is True
    assert observation["fans"][0]["rpm"] == 3200
    serialized = json.dumps(observation)
    assert "/sys/" not in serialized
    assert "hwmon7" not in serialized
    assert "hwmon:coretemp:coretemp:temp1:Package_id_0" in serialized
    assert "thermal_zone:x86_pkg_temp:thermal_zone3" in serialized


def test_duplicate_labels_and_zone_types_keep_unique_identities(tmp_path: Path) -> None:
    hwmon = tmp_path / "sys/class/hwmon/hwmon0"
    _write(hwmon / "name", "coretemp\n")
    _write(hwmon / "temp1_input", "65000\n")
    _write(hwmon / "temp1_label", "Package\n")
    _write(hwmon / "temp2_input", "66000\n")
    _write(hwmon / "temp2_label", "Package\n")

    for index, value in ((1, 67000), (2, 68000)):
        zone = tmp_path / f"sys/class/thermal/thermal_zone{index}"
        _write(zone / "type", "x86_pkg_temp\n")
        _write(zone / "temp", f"{value}\n")

    temperatures, _ = collector.collect_hwmon(tmp_path)
    zones = collector.collect_thermal_zones(tmp_path)

    assert len({item["stable_id"] for item in temperatures}) == 2
    assert len({item["stable_id"] for item in zones}) == 2
    assert [item["instance"] for item in zones] == ["thermal_zone1", "thermal_zone2"]


def test_implausible_temperature_is_recorded_but_not_usable(tmp_path: Path) -> None:
    hwmon = tmp_path / "sys/class/hwmon/hwmon0"
    _write(hwmon / "name", "coretemp\n")
    _write(hwmon / "temp1_input", "9999000\n")

    temperatures, _ = collector.collect_hwmon(tmp_path)
    assert temperatures[0]["readable"] is True
    assert temperatures[0]["plausible"] is False


def test_extract_thermal_status_excludes_unrelated_runtime_data() -> None:
    status = """correlation_id=secret-machine-value
thermal_state=Derating
thermal_derating_ratio=0.40
thermal_die_sensor=hwmon:cpu:package
thermal_reasons:
- die temp is elevated
on_ac=true
thermal_max_fan_rpm=3200
"""
    extracted = collector.extract_thermal_status(status)
    assert "thermal_state=Derating" in extracted
    assert "- die temp is elevated" in extracted
    assert "thermal_max_fan_rpm=3200" in extracted
    assert "correlation_id" not in extracted
    assert "on_ac" not in extracted


def test_acceptance_commands_cover_canonical_t1_mapping() -> None:
    names = collector.t1_acceptance_test_names(_ROOT / "docs/plans/optid-package-status.toml")
    commands = collector.t1_acceptance_commands(
        _ROOT / "docs/plans/optid-package-status.toml"
    )
    assert len(names) == 37
    assert len(commands) == len(names)
    assert all(command[-1] == "--exact" for command in commands)
    assert all("thermal::tests::" in command[-3] for command in commands)
    assert "t1_production_pipeline_collect_to_render" in names
    assert "t1_production_pipeline_off_mode_zero_thermal_reads" in names
    assert "thermal_budget_derating_never_decreases_as_temperature_rises" in names
    # ADR 0026 conformance repair: every mapped test must remain addressable as
    # `thermal::tests::<name>` so the collector can run it with --exact.
    assert "t1_conformance_no_die_signal_is_unavailable_despite_other_temps" in names
    assert "t1_conformance_faulted_and_alarmed_readings_never_yield_cool" in names
    # ADR 0026 open-findings repair: the four gaps recorded at ratification.
    assert "t1_conformance_low_crit_fails_closed_instead_of_inventing_a_range" in names
    assert "t1_conformance_core_maximum_does_not_replace_package_provenance" in names
    assert "t1_conformance_skin_requires_a_labelled_channel_not_a_chip_name" in names
    assert "t1_conformance_alarm_survives_an_unreadable_temperature" in names
    # Independent verifier findings: the policy-facing temperature invariant is
    # the one that pins the cross-module leak, so assert it by name.
    assert (
        "t1_conformance_policy_facing_temperature_never_comes_from_a_non_die_source"
        in names
    )
    assert "t1_conformance_die_eligibility_is_driver_scoped_not_label_scoped" in names
    assert "t1_conformance_duplicate_collapse_keeps_the_maximum_and_the_alarm" in names
    assert "t1_conformance_hwmon_nodes_without_a_device_link_stay_distinct" in names
    assert "t1_conformance_status_records_the_effective_thresholds" in names
    assert "t1_conformance_collapse_records_where_a_raised_value_came_from" in names
    assert (
        "t1_conformance_same_type_zones_without_device_link_stay_distinct" in names
    )
    assert "t1_conformance_alarm_anywhere_defeats_a_full_headroom_claim" in names


def test_command_output_is_sanitized(tmp_path: Path) -> None:
    result = collector.CommandResult(
        ["example"], 1, f"cwd={tmp_path} /home/alice/private", "/Users/bob/secret"
    )
    recorded = collector.command_results_json([result], tmp_path)[0]
    assert str(tmp_path) not in recorded["stdout"]
    assert "/home/alice" not in recorded["stdout"]
    assert "/Users/bob" not in recorded["stderr"]
    assert "<repo>" in recorded["stdout"]
    assert "<home>" in recorded["stdout"]


def test_privacy_validator_rejects_home_paths(tmp_path: Path) -> None:
    _write(tmp_path / "manifest.json", '{"path":"/home/alice/private"}\n')
    assert collector.validate_privacy(tmp_path) == ["privacy rule matched in manifest.json"]


def test_completion_ready_rejects_fixture_sys_root(tmp_path: Path) -> None:
    fixture_root = tmp_path / "fixture"
    hwmon = fixture_root / "sys/class/hwmon/hwmon0"
    _write(hwmon / "name", "coretemp\n")
    _write(hwmon / "temp1_input", "65000\n")

    output = tmp_path / "bundle"
    returncode = collector.main(
        [
            "--output",
            str(output),
            "--samples",
            "1",
            "--interval-seconds",
            "0",
            "--sys-root",
            str(fixture_root),
            "--repo-root",
            str(tmp_path),
            "--skip-command-checks",
            "--require-completion-ready",
        ]
    )

    manifest = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
    assert returncode == 1
    assert manifest["live_sys_root"] is False
    assert "completion-ready collection requires the live / sysfs root" in manifest[
        "unresolved"
    ]
