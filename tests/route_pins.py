# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Shared pin for a declared family that routes through a writer-enabled private core.

Every such family asserts the same three things about its core, and the assertion was hand-copied
per family until it drifted: three separate migrations (#639, #645, #646) produced a weaker variant
that an independent review had to catch. The properties are here once, and a family's own suite
names its own values.

What this does **not** prove is that the core is the only way to reach the capability -- two routes
defeat the sweep and are left to other gates, because closing them here would cost more than they
are worth:

* ``core(part, **{"writer": ...})`` hides the keyword, but needs a ``type: ignore[arg-type]``
  to clear ``mypy``.
* ``getattr(module, "_discover_x")`` hides the attribute, but ``ruff`` rejects it as B009.
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


def _function(tree: ast.Module, name: str) -> ast.FunctionDef:
    return next(
        node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == name
    )


def _core_call(function: ast.FunctionDef, core: str) -> ast.Call:
    return next(
        node
        for node in ast.walk(function)
        if isinstance(node, ast.Call) and _callee_name(node.func) == core
    )


def assert_core_route_is_closed(
    *,
    module: str,
    core: str,
    declaration: str = "_discover",
    entrypoint: str,
    handed_over: dict[str, str],
    withheld: tuple[str, ...] = ("writer",),
) -> None:
    """Assert only *module*'s *declaration* reaches *core* with the run's capabilities.

    *handed_over* maps each keyword the declaration must pass to the source text of the expression
    it must pass, so a family passing three run-scoped handles pins all three. Pinning only the
    writer leaves the rest free: dropping ``face_surfaces`` from the pads declaration is type-valid
    and silently rebuilds a graph the run already has.

    *withheld* names what the public entry point must not pass. That is narrower than everything
    the declaration hands over, because an entry point may legitimately forward its own parameters
    to the core -- ``recognise_flats`` passes ``cyls`` and ``face_edges`` it was given. What it may
    never pass is the capability.
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
            and node.module == f"quiddity.{module}"
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
    declared_call = _core_call(_function(tree, declaration), core)
    passed = {
        keyword.arg: ast.unparse(keyword.value)
        for keyword in declared_call.keywords
        if keyword.arg is not None
    }
    for keyword, expected in handed_over.items():
        assert passed.get(keyword) == expected, (
            f"{declaration} passes {keyword}={passed.get(keyword)!r}, expected {expected!r}"
        )

    public_call = _core_call(_function(tree, entrypoint), core)
    handed = sorted(
        keyword.arg
        for keyword in public_call.keywords
        if keyword.arg is not None and keyword.arg in withheld
    )
    assert handed == [], f"{entrypoint} hands the core {handed}, which only the declaration may"
