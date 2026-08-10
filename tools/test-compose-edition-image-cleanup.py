#!/usr/bin/env python3
"""Safety regressions for edition image mount cleanup."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from types import SimpleNamespace

import pytest

TOOL = Path(__file__).with_name("compose-edition-image.py")
SPEC = importlib.util.spec_from_file_location("compose_edition_image", TOOL)
assert SPEC is not None and SPEC.loader is not None
COMPOSER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPOSER)


def composer_args(tmp_path: Path) -> SimpleNamespace:
    deployer = tmp_path / "deploy.py"
    deployer.write_text("#!/usr/bin/env python3\n", encoding="utf-8")
    return SimpleNamespace(
        deployer=deployer,
        mount_parent=tmp_path / "mounts",
        systemd_dissect="systemd-dissect",
        unsigned_development=False,
    )


def invoke(tmp_path: Path, args: SimpleNamespace) -> None:
    COMPOSER.deploy_into_image(
        args=args,
        image=tmp_path / "image.raw",
        plan_path=tmp_path / "edition-plan.json",
        artifact=tmp_path / "extension.raw",
        receipt=tmp_path / "extension.build.json",
    )


def test_unmount_failure_preserves_mounted_tree_and_primary_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    args = composer_args(tmp_path)
    mounted_root: Path | None = None

    def fake_run(command: list[str], *, cwd: Path | None = None) -> None:
        nonlocal mounted_root
        if "--mount" in command:
            mounted_root = Path(command[-1])
            (mounted_root / "sentinel").write_text("mounted", encoding="utf-8")
            return
        if "--umount" in command:
            raise COMPOSER.ComposeError("simulated unmount failure")
        raise COMPOSER.ComposeError("simulated deployment failure")

    monkeypatch.setattr(COMPOSER, "run_checked", fake_run)

    with pytest.raises(COMPOSER.ComposeError) as raised:
        invoke(tmp_path, args)

    message = str(raised.value)
    assert "simulated deployment failure" in message
    assert "simulated unmount failure" in message
    assert mounted_root is not None
    assert (mounted_root / "sentinel").read_text(encoding="utf-8") == "mounted"


def test_partial_mount_failure_never_recursively_deletes_mount_tree(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    args = composer_args(tmp_path)
    mounted_root: Path | None = None

    def fake_run(command: list[str], *, cwd: Path | None = None) -> None:
        nonlocal mounted_root
        assert "--mount" in command
        mounted_root = Path(command[-1])
        (mounted_root / "sentinel").write_text("possibly mounted", encoding="utf-8")
        raise COMPOSER.ComposeError("simulated mount failure")

    monkeypatch.setattr(COMPOSER, "run_checked", fake_run)

    with pytest.raises(COMPOSER.ComposeError) as raised:
        invoke(tmp_path, args)

    message = str(raised.value)
    assert "simulated mount failure" in message
    assert "failed to remove mount directory" in message
    assert mounted_root is not None
    assert (mounted_root / "sentinel").read_text(encoding="utf-8") == "possibly mounted"


if __name__ == "__main__":
    raise SystemExit(pytest.main([__file__, "-v"]))
