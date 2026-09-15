# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Shared pin for a declared family that routes through a writer-enabled private core.

Every such family asserts the same things about its core, and the assertion was hand-copied per
family until it drifted: three separate migrations (#639, #645, #646) produced a weaker variant
that an independent review had to catch. The properties are here once, and a family's own suite
names its own values.

What this does **not** prove is that the core is the only way to reach the capability. Three
routes defeat a syntactic sweep, and only one of them is caught elsewhere:

* ``globals()["_discover_x"](part, writer=...)`` inside the family module itself. No gate rejects
  this -- the call sweep does not recognise the callee, and the sweep for outside modules skips
  this file by construction.
* a positional argument. All seven cores take the capability keyword-only, so the capability
  itself cannot go this way, but the part the declaration passes is unpinned.
* ``getattr(module, "_discover_x")``, which ``ruff`` rejects as B009.

A ``**``-splat is *not* on that list: both calls must spell their keywords, because a splat at the
sanctioned call site would otherwise hide the capability from ``withheld``. ``mypy`` does not catch
it -- an entry point can splat a real ``EvidenceWriter`` into the core with no ``type: ignore`` --
so the pin rejects it directly.
"""

from __future__ import annotations

import ast
from pathlib import Path

PACKAGE = Path(__file__).parents[1] / "src" / "quiddity"


def _callee_name(func: ast.expr) -> str | None:
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return None


def _function(tree: ast.Module, name: str, filename: str) -> ast.FunctionDef:
    function = next(
        (node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == name),
        None,
    )
    assert function is not None, f"{filename} has no module-level `def {name}`"
    return function


def _core_call(function: ast.FunctionDef, core: str) -> ast.Call:
    call = next(
        (
            node
            for node in ast.walk(function)
            if isinstance(node, ast.Call) and _callee_name(node.func) == core
        ),
        None,
    )
    assert call is not None, f"{function.name} does not call {core}"
    return call


def _spelled_keywords(call: ast.Call, where: str) -> dict[str, str]:
    assert all(keyword.arg is not None for keyword in call.keywords), (
        f"{where} reaches the core through a ** splat, which hides what it hands over"
    )
    return {
        keyword.arg: ast.unparse(keyword.value)
        for keyword in call.keywords
        if keyword.arg is not None
    }


def assert_core_route_is_closed(
    *,
    module: str,
    core: str,
    declaration: str = "_discover",
    entrypoint: str,
    handed_over: dict[str, str],
    withheld: tuple[str, ...],
) -> None:
    """Assert only *module*'s *declaration* reaches *core* with the run's capabilities.

    *handed_over* maps every keyword the declaration passes to the source text of the expression
    it must pass, and it is exhaustive: a handle added later without being pinned fails here. That
    matters in both directions -- dropping ``face_surfaces`` from the pads declaration is
    type-valid and silently rebuilds a surface graph the run already has.

    *withheld* names what the public entry point must not pass, and has no default on purpose. It
    is narrower than everything the declaration hands over, because an entry point may legitimately
    forward its own parameters to the core -- ``recognise_flats`` passes ``cyls`` and ``face_edges``
    it was given. What it may never pass is the capability. A default would be wrong the moment a
    family spells that capability ``ledger`` or ``sink``, both of which this package uses, and it
    would be wrong *silently*: the check would pass while asserting nothing.
    """

    filename = f"{module}.py"

    # No module outside the family names the core at all -- not by import, not as an attribute,
    # since either can be rebound and called under a name this sweep would not recognise.
    outsiders = sorted(
        path.name
        for path in PACKAGE.glob("*.py")
        if path.name != filename
        for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"), filename=str(path)))
        if (isinstance(node, ast.Attribute) and node.attr == core)
        or (
            isinstance(node, ast.ImportFrom)
            # A relative `from .flats import ...` carries the bare module name and a level.
            and node.module in (f"quiddity.{module}", module)
            and any(alias.name == core for alias in node.names)
        )
    )
    assert outsiders == [], f"{core} is named outside {filename} by {outsiders}"

    # Exactly two call sites, both here. A third route into the writer-enabled core would be
    # invisible to the sweep above, which skips this file.
    call_sites = [
        path.name
        for path in PACKAGE.glob("*.py")
        for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"), filename=str(path)))
        if isinstance(node, ast.Call) and _callee_name(node.func) == core
    ]
    assert call_sites == [filename, filename], f"{core} call sites: {call_sites}"

    # Which call is which is decided by the function holding it, not by the keywords it carries:
    # a declaration and an entry point that swapped keywords would otherwise both look right.
    tree = ast.parse((PACKAGE / filename).read_text(encoding="utf-8"))
    declared_call = _core_call(_function(tree, declaration, filename), core)
    passed = _spelled_keywords(declared_call, declaration)
    assert passed == handed_over, f"{declaration} hands over {passed}, pinned as {handed_over}"

    public_call = _core_call(_function(tree, entrypoint, filename), core)
    handed = sorted(
        keyword for keyword in _spelled_keywords(public_call, entrypoint) if keyword in withheld
    )
    assert handed == [], f"{entrypoint} hands the core {handed}, which only the declaration may"
