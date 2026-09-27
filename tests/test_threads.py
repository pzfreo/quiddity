"""The opt-in OCCT worker cap is set once, before a recognition run."""

from __future__ import annotations

import json
import os
import subprocess
import sys

import pytest

_PROBE = """
import json
from OCP.OSD import OSD_Parallel, OSD_ThreadPool

pool = OSD_ThreadPool.DefaultPool_s()
pool.Init(2)
pool.SetNbDefaultThreadsToLaunch(2)

from build123d import Box, Cylinder
part = Box(20, 20, 5) - Cylinder(2, 5)

from quiddity.document import build_recognition_document

document = build_recognition_document(part)
print(json.dumps({
    "pool": pool.NbThreads(),
    "launch": pool.NbDefaultThreadsToLaunch(),
    "backend": OSD_Parallel.ToUseOcctThreads_s(),
    "features": document["features"],
}, sort_keys=True))
"""


def _probe(setting: str | None) -> dict[str, object]:
    environment = os.environ.copy()
    environment.pop("QUIDDITY_THREADS", None)
    if setting is not None:
        environment["QUIDDITY_THREADS"] = setting
    result = subprocess.run(
        [sys.executable, "-c", _PROBE],
        env=environment,
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(result.stdout)


def test_thread_cap_resizes_existing_occt_pool_without_changing_recognition() -> None:
    default = _probe(None)
    capped = _probe("1")

    assert default["pool"] == default["launch"] == 2
    assert capped["pool"] == capped["launch"] == 1
    assert capped["backend"] is True
    assert capped["features"] == default["features"]


@pytest.mark.parametrize("setting", ("0", "-1", "many"))
def test_invalid_thread_cap_fails_clearly(setting: str) -> None:
    environment = os.environ.copy()
    environment["QUIDDITY_THREADS"] = setting
    result = subprocess.run(
        [sys.executable, "-c", "import quiddity"],
        env=environment,
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0
    assert "QUIDDITY_THREADS must be a positive integer" in result.stderr
