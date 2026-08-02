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
    assert "hwmon:coretemp:coretemp:Package_id_0" in serialized


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
    assert len(names) == 14
    assert len(commands) == len(names)
    assert all(command[-1] == "--exact" for command in commands)
    assert all("thermal::tests::" in command[-3] for command in commands)
    assert "t1_production_pipeline_collect_to_render" in names
    assert "t1_production_pipeline_off_mode_zero_thermal_reads" in names
    assert "thermal_budget_derating_never_decreases_as_temperature_rises" in names


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
