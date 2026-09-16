# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Every name a test patches on `_registry` must still be there to patch.

The declared-family migration moves recogniser entry points and private cores out of `_registry`
one family at a time, and a test that patches a moved name fails at `monkeypatch.setattr` with an
`AttributeError` -- loudly, but only if that test is in the set someone thought to run. #664 was
pushed red because six such patches sat in two files the change never opened.

This reads the patch targets out of the source instead, so the whole class is one cheap check
rather than a habit of remembering which suites to run.
"""

import ast
from pathlib import Path

import pytest

import quiddity._registry as registry_module

TESTS = Path(__file__).parent


def _registry_patch_targets(source: str) -> set[str]:
    """Names passed to `monkeypatch.setattr(<registry alias>, "<name>", ...)` in *source*."""

    tree = ast.parse(source)
    aliases = {
        alias.asname or alias.name
        for node in ast.walk(tree)
        if isinstance(node, ast.Import)
        for alias in node.names
        if alias.name == "quiddity._registry"
    }
    targets = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or len(node.args) < 2:
            continue
        if getattr(node.func, "attr", None) != "setattr":
            continue
        target, name = node.args[0], node.args[1]
        if (
            isinstance(target, ast.Name)
            and target.id in aliases
            and isinstance(name, ast.Constant)
            and isinstance(name.value, str)
        ):
            targets.add(name.value)
    return targets


@pytest.mark.parametrize("path", sorted(TESTS.glob("test_*.py")), ids=lambda path: path.name)
def test_every_registry_patch_target_still_exists(path: Path) -> None:
    missing = sorted(
        name
        for name in _registry_patch_targets(path.read_text(encoding="utf-8"))
        if not hasattr(registry_module, name)
    )
    assert missing == [], f"{path.name} patches {missing} on `_registry`, which no longer has them"
