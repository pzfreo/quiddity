# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""``scripts/update-recogniser-version`` moves every copy of the version, or none of them.

The script exists because a release that moves some copies and not others fails silently:
``__init__``'s ``PackageNotFoundError`` fallback sat at 0.2.2 through both the 0.2.3 and
0.2.4 releases, since nothing exercises it outside a bare source tree. So the property worth
testing is not that it can write a version -- it is that a partial write cannot survive.

``uv version`` is stubbed here rather than run. It owns ``pyproject.toml`` and ``uv.lock``
and is not this script's behaviour; what is, is what happens to the other four files around
it, including when it fails.
"""

from __future__ import annotations

import importlib.util
import json
import pathlib
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]
SCRIPT = ROOT / "scripts" / "update-recogniser-version"


def _load():
    spec = importlib.util.spec_from_loader(
        "update_recogniser_version",
        importlib.machinery.SourceFileLoader("update_recogniser_version", str(SCRIPT)),
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _project(root: Path, version: str = "0.2.5") -> None:
    (root / "src" / "quiddity").mkdir(parents=True)
    (root / "pyproject.toml").write_text(f'[project]\nversion = "{version}"\n', encoding="utf-8")
    (root / "uv.lock").write_text(f'version = "{version}"\n', encoding="utf-8")
    (root / "src/quiddity/capabilities.json").write_text(
        json.dumps(
            {"families": [], "package": {"name": "quiddity", "version": version}},
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (root / "src/quiddity/inspection_api.json").write_text(
        json.dumps(
            {"api": {}, "package": {"name": "quiddity", "version": version}},
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (root / "src/quiddity/evidence_api.json").write_text(
        json.dumps(
            {"api": {}, "package": {"name": "quiddity", "version": version}},
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (root / "src/quiddity/__init__.py").write_text(
        "try:\n"
        '    __version__ = version("quiddity")\n'
        "except PackageNotFoundError:\n"
        f'    __version__ = "{version}"\n',
        encoding="utf-8",
    )


def _versions(root: Path) -> dict[str, str]:
    manifest = json.loads((root / "src/quiddity/capabilities.json").read_text())
    inspection = json.loads((root / "src/quiddity/inspection_api.json").read_text())
    evidence = json.loads((root / "src/quiddity/evidence_api.json").read_text())
    init = (root / "src/quiddity/__init__.py").read_text(encoding="utf-8")
    return {
        "manifest": manifest["package"]["version"],
        "inspection": inspection["package"]["version"],
        "evidence": evidence["package"]["version"],
        "fallback": init.split('__version__ = "')[-1].split('"')[0],
    }


@pytest.mark.parametrize("target", ["0.2.6", "0.2.6.dev0", "1.0.0.dev12"])
def test_every_embedded_copy_moves_together(tmp_path, monkeypatch, target) -> None:
    """A release and a dev snapshot both move the manifest and the fallback."""

    module = _load()
    _project(tmp_path)
    monkeypatch.setattr(module.subprocess, "run", lambda *a, **k: None)

    module.update(tmp_path, target)

    assert _versions(tmp_path) == {
        "manifest": target,
        "inspection": target,
        "evidence": target,
        "fallback": target,
    }


@pytest.mark.parametrize(
    "target", ["0.2", "0.2.6.dev", "v0.2.6", "0.2.6-rc1", "01.2.3", "0.2.6rc1"]
)
def test_a_version_that_is_not_a_release_or_a_snapshot_is_refused(tmp_path, target) -> None:
    """Refused before anything is written, so a typo cannot half-apply.

    ``0.2.6rc1`` is in this list rather than the accepted one: ``capabilities.py`` rejects it,
    so a wheel built at that version raises from ``capability_manifest()``. Refusing it here
    stops that before anything is published -- see the script's own note on the regex.
    """

    module = _load()
    _project(tmp_path)

    with pytest.raises(ValueError):
        module.update(tmp_path, target)

    assert set(_versions(tmp_path).values()) == {"0.2.5"}


def test_a_failure_part_way_through_restores_every_file(tmp_path, monkeypatch) -> None:
    """The property the whole script is for: no half-moved version survives.

    ``uv version`` is made to fail *after* it would have rewritten ``pyproject.toml`` and
    ``uv.lock``, which is the worst case -- two files already moved, two not. All four must
    come back.
    """

    module = _load()
    _project(tmp_path)

    def explode(*_args, **_kwargs):
        (tmp_path / "pyproject.toml").write_text('[project]\nversion = "0.2.6"\n', encoding="utf-8")
        (tmp_path / "uv.lock").write_text('version = "0.2.6"\n', encoding="utf-8")
        raise RuntimeError("uv failed")

    monkeypatch.setattr(module.subprocess, "run", explode)

    with pytest.raises(RuntimeError):
        module.update(tmp_path, "0.2.6")

    assert set(_versions(tmp_path).values()) == {"0.2.5"}
    assert 'version = "0.2.5"' in (tmp_path / "pyproject.toml").read_text(encoding="utf-8")
    assert 'version = "0.2.5"' in (tmp_path / "uv.lock").read_text(encoding="utf-8")


def test_a_failure_after_the_manifest_is_written_still_restores_it(tmp_path, monkeypatch) -> None:
    """The rollback's real worst case, which the test above does not reach.

    That one raises inside the `uv version` stub, i.e. before the manifest and fallback are
    touched -- so a rollback restoring only `pyproject.toml` and `uv.lock` passed it. Failing
    at the *last* write is what actually requires all six snapshots to be honoured.
    """

    module = _load()
    _project(tmp_path)
    monkeypatch.setattr(module.subprocess, "run", lambda *a, **k: None)
    real_write = pathlib.Path.write_text

    def fail_on_the_fallback(self, data, **kwargs):
        if self.name == "__init__.py":
            raise RuntimeError("disk full")
        return real_write(self, data, **kwargs)

    monkeypatch.setattr(pathlib.Path, "write_text", fail_on_the_fallback)

    with pytest.raises(RuntimeError):
        module.update(tmp_path, "0.2.6")

    monkeypatch.undo()
    assert set(_versions(tmp_path).values()) == {"0.2.5"}


def test_a_source_tree_missing_the_fallback_is_refused_before_writing(tmp_path) -> None:
    """A moved or reworded fallback must stop the release, not be silently skipped.

    Silently skipping it is exactly how the fallback went stale for two releases, so this
    script fails closed on a shape it does not recognise rather than doing three quarters of
    the job.
    """

    module = _load()
    _project(tmp_path)
    (tmp_path / "src/quiddity/__init__.py").write_text(
        "__version__ = _resolved_elsewhere()\n", encoding="utf-8"
    )

    with pytest.raises(RuntimeError, match="fallback version"):
        module.update(tmp_path, "0.2.6")

    manifest = json.loads((tmp_path / "src/quiddity/capabilities.json").read_text())
    assert manifest["package"]["version"] == "0.2.5"
    inspection = json.loads((tmp_path / "src/quiddity/inspection_api.json").read_text())
    assert inspection["package"]["version"] == "0.2.5"
