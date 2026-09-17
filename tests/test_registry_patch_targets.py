# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Every `_registry` name a test reaches for must still be there to reach.

The declared-family migration moves recogniser entry points and private cores out of `_registry`
one family at a time. A test that names a moved one fails loudly -- but only if it is in the set
someone thought to run, and #664 was pushed red with six such references in two files the change
never opened.

So this reads the names out of the source rather than relying on that. It matches **references**,
not `monkeypatch.setattr` calls: two of those six were a plain attribute read saving the original
before patching it, and a sweep for patch calls alone would have found that file only because
`setattr` happened to appear in it too. The forms that reach a module attribute -- an aliased
import, a bare `import quiddity._registry`, `from quiddity import _registry`, a `setattr` or
`mock.patch.object` against any of them, and the `"quiddity._registry.X"` string target `pytest`
accepts -- all reduce to an attribute access or a dotted string, and both are read here.

What it cannot see is a name assembled at runtime, `monkeypatch.setattr(registry, name, ...)` with
a variable. Two files use that form against other modules, so it is house style and may arrive
here; nothing below would catch it.
"""

import ast
import re
from pathlib import Path

import pytest

import quiddity._registry as registry_module

TESTS = Path(__file__).parent

_STRING_TARGET = re.compile(r"^quiddity\._registry\.(\w+)$")


def _registry_aliases(tree: ast.AST) -> set[str]:
    """Local names that refer to the registry module, however it was imported."""

    aliases = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name == "quiddity._registry":
                    # `import quiddity._registry` binds `quiddity`; the attribute chain below
                    # reads `quiddity._registry.X` as an attribute of an attribute.
                    aliases.add(alias.asname or "quiddity._registry")
        elif isinstance(node, ast.ImportFrom) and node.module == "quiddity":
            aliases.update(
                alias.asname or alias.name for alias in node.names if alias.name == "_registry"
            )
    return aliases


def _callee(func: ast.expr) -> str:
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return ""


def _dotted(node: ast.expr) -> str:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return f"{_dotted(node.value)}.{node.attr}"
    return ""


def _registry_references(source: str) -> set[str]:
    """Every `_registry` attribute the source names, by any route."""

    tree = ast.parse(source)
    aliases = _registry_aliases(tree)
    found = set()
    for node in ast.walk(tree):
        # `registry_module.recognise_x`, however the module was bound.
        if isinstance(node, ast.Attribute) and _dotted(node.value) in aliases:
            found.add(node.attr)
        # `setattr(registry_module, "recognise_x", ...)`, `monkeypatch.setattr` and
        # `mock.patch.object` all pass the module first and the name as a string second.
        elif (
            isinstance(node, ast.Call)
            # `hasattr` is how a test asserts a name is *absent*, which is the opposite claim.
            and _callee(node.func) in {"setattr", "getattr", "delattr", "object"}
            and len(node.args) >= 2
            and _dotted(node.args[0]) in aliases
            and isinstance(node.args[1], ast.Constant)
            and isinstance(node.args[1].value, str)
        ):
            found.add(node.args[1].value)
        # `monkeypatch.setattr("quiddity._registry.recognise_x", ...)`.
        elif isinstance(node, ast.Constant) and isinstance(node.value, str):
            match = _STRING_TARGET.match(node.value)
            if match:
                found.add(match.group(1))
    return found


@pytest.mark.parametrize("path", sorted(TESTS.glob("*.py")), ids=lambda path: path.name)
def test_every_registry_reference_still_resolves(path: Path) -> None:
    missing = sorted(
        name
        for name in _registry_references(path.read_text(encoding="utf-8"))
        if not hasattr(registry_module, name)
    )
    assert missing == [], f"{path.name} names {missing} on `_registry`, which no longer has them"


#: The migration is finished, so no test names a migration-sensitive registry attribute any more
#: and the sweep above legitimately finds nothing across the corpus. That makes a corpus count
#: useless as a matcher guard -- zero reads the same whether the matcher works and there is
#: nothing left, or the matcher quietly stopped matching. So the guard runs the matcher over a
#: fixture carrying one of every form the module docstring claims to support, which pins all of
#: them rather than whichever happened to survive. The fixture is assembled from `_MODULE` rather
#: than written out: spelled literally, its string target and attribute chains would be picked up
#: by the sweep scanning this very file, and the resolution test above would then fail on names
#: that exist only inside it.
_MODULE = "quiddity._registry"
_PACKAGE, _PRIVATE = _MODULE.split(".")

_REFERENCE_FORMS = f"""
import {_MODULE}
import {_MODULE} as aliased
from {_PACKAGE} import {_PRIVATE}
from {_PACKAGE} import {_PRIVATE} as renamed

{_MODULE}.BARE_IMPORT
aliased.ALIASED_IMPORT
{_PRIVATE}.FROM_IMPORT
renamed.RENAMED_IMPORT
setattr(aliased, "SETATTR_TARGET", None)
getattr(aliased, "GETATTR_TARGET")
delattr(renamed, "DELATTR_TARGET")
monkeypatch.setattr(renamed, "MONKEYPATCH_TARGET", None)
mock.patch.object({_PRIVATE}, "PATCH_OBJECT_TARGET")
monkeypatch.setattr("{_MODULE}.STRING_TARGET", None)
hasattr({_PRIVATE}, "ABSENT_NAME")
"""


def test_the_sweep_still_finds_every_reference_form_it_claims_to() -> None:
    """Guards the matcher: a sweep that matched nothing would pass every case above."""

    assert _registry_references(_REFERENCE_FORMS) == {
        "BARE_IMPORT",
        "ALIASED_IMPORT",
        "FROM_IMPORT",
        "RENAMED_IMPORT",
        "SETATTR_TARGET",
        "GETATTR_TARGET",
        "DELATTR_TARGET",
        "MONKEYPATCH_TARGET",
        "PATCH_OBJECT_TARGET",
        "STRING_TARGET",
    }


def test_the_sweep_still_refuses_the_one_form_that_claims_absence() -> None:
    """`hasattr` is excluded on purpose, and the exclusion is as load-bearing as the matches.

    A test writes `assert not hasattr(registry_module, "X")` to claim a name is *gone*. Reading
    that as a reference would make the resolution test above fail on exactly the assertion that
    proves the migration worked, so widening the callee set is not the harmless fix it looks
    like. The fixture carries the form; this pins that it finds nothing.
    """

    assert "ABSENT_NAME" not in _registry_references(_REFERENCE_FORMS)
