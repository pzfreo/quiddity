# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Shared pin for a declared family that routes through a writer-enabled private core.

Every such family asserts the same things about its core, and the assertion was hand-copied per
family until it drifted: three separate migrations (#639, #645, #646) produced a weaker variant
that an independent review had to catch. The properties are here once, and a family's own suite
names its own values.

What this does **not** prove is that the core is the only way to reach the capability. Three
routes defeat a syntactic sweep, and only one of them is caught elsewhere:

* ``globals()["_discover_x"](part, writer=...)`` inside a module that already holds a sanctioned
  caller. No gate rejects this -- the call sweep does not recognise the callee, and the sweep for
  outside modules skips those files by construction.
* a positional argument. Every core takes the capability keyword-only, so the capability itself
  cannot go this way, but the part the declaration passes is unpinned.
* ``getattr(module, "_discover_x")``, which ``ruff`` rejects as B009.

A ``**``-splat is *not* on that list: every sanctioned call must spell its keywords, because a
splat at a sanctioned call site would otherwise hide the capability from the withheld set.
``mypy`` does not catch it -- an entry point can splat a real ``EvidenceWriter`` into the core
with no ``type: ignore`` -- so the pin rejects it directly.
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


def _module_level_defs(name: str) -> list[tuple[str, ast.FunctionDef]]:
    return [
        (path.name, node)
        for path in sorted(PACKAGE.glob("*.py"))
        for node in ast.parse(path.read_text(encoding="utf-8"), filename=str(path)).body
        if isinstance(node, ast.FunctionDef) and node.name == name
    ]


def _locate(name: str) -> tuple[str, ast.FunctionDef]:
    """The one module-level ``def name`` in the package, with the file holding it.

    Locating rather than being told the file is what lets a sanctioned caller live in a module
    other than the declaration's -- the recess families declare in ``slots.py`` and keep both
    core and entry point in ``_recess_features.py``. Two definitions of the name is an error
    rather than a pick, because the pin would otherwise silently check the wrong one.
    """
    found = _module_level_defs(name)
    assert len(found) == 1, f"{name} has {len(found)} module-level definitions: {found}"
    return found[0]


def _the_core_call(where: tuple[str, ast.FunctionDef], core: str) -> ast.Call:
    filename, function = where
    calls = [
        node
        for node in ast.walk(function)
        if isinstance(node, ast.Call) and _callee_name(node.func) == core
    ]
    assert len(calls) == 1, f"{filename}::{function.name} calls {core} {len(calls)} times"
    return calls[0]


def _spelled_keywords(call: ast.Call, where: str) -> dict[str, str]:
    assert all(keyword.arg is not None for keyword in call.keywords), (
        f"{where} reaches the core through a ** splat, which hides what it hands over"
    )
    return {
        keyword.arg: ast.unparse(keyword.value)
        for keyword in call.keywords
        if keyword.arg is not None
    }


def _sites_in(node: ast.AST, core: str, filename: str, enclosing: str) -> list[tuple[str, str]]:
    sites: list[tuple[str, str]] = []
    for child in ast.iter_child_nodes(node):
        if isinstance(child, ast.FunctionDef | ast.AsyncFunctionDef):
            sites.extend(_sites_in(child, core, filename, child.name))
            continue
        if isinstance(child, ast.Call) and _callee_name(child.func) == core:
            sites.append((filename, enclosing))
        sites.extend(_sites_in(child, core, filename, enclosing))
    return sites


def _call_sites(core: str) -> list[tuple[str, str]]:
    """Every call to *core* in the package, as (file, enclosing function) pairs."""
    return [
        site
        for path in sorted(PACKAGE.glob("*.py"))
        for site in _sites_in(
            ast.parse(path.read_text(encoding="utf-8"), filename=str(path)),
            core,
            path.name,
            "<module>",
        )
    ]


def assert_core_route_is_closed(
    *,
    module: str,
    core: str,
    declaration: str = "_discover",
    handed_over: dict[str, str],
    also_reached_from: dict[str, tuple[str, ...]],
) -> None:
    """Assert *module*'s *declaration* is the only route that hands *core* the run's capabilities.

    *handed_over* maps every keyword the declaration passes to the ``ast.unparse`` form of the
    expression it must pass -- normal form, not the source text, so ``0.30`` is pinned as ``0.3``
    and reformatting or parenthesising an expression does not fail. It is exhaustive: a handle
    added later without being pinned fails here. That matters in both directions -- dropping
    ``face_surfaces`` from the pads declaration is type-valid and silently rebuilds a surface
    graph the run already has.

    *also_reached_from* is the exhaustive roster of the **other** functions allowed to call the
    core, each mapped to the keywords it must not pass. It is required and exhaustive in both
    directions: a call site it does not name fails, and a function it names that does not call
    the core fails too. Most families map their public entry point to the capability it may not
    forward -- ``{"recognise_flats": ("writer",)}`` -- because an entry point may legitimately
    forward its own parameters (``recognise_flats`` passes the ``cyls`` and ``face_edges`` it was
    given) but never the writer the declaration mints.

    ``also_reached_from={}`` is a stronger claim, not a skipped one: it says the declaration is
    the *only* caller in the package, which is what the call-site sweep below then checks. Both
    ``levels`` families are in that shape -- ``recognise_face_levels`` and ``recognise_risers``
    compute a deliberately different thing from their cores rather than calling them.

    Each roster entry's keywords must be non-empty and must name keywords the declaration hands
    over, so that a typo cannot quietly assert nothing. There is no default: it would be wrong
    the moment a family spells the capability ``ledger`` or ``sink``, both of which this package
    uses, and it would be wrong *silently*.
    """

    for caller, withheld in also_reached_from.items():
        # A `withheld` naming nothing the declaration passes would assert nothing at all, which is
        # how this pin was wrong before: the check stays green and the route stays open.
        assert withheld and set(withheld) <= set(handed_over), (
            f"{caller}'s withheld={withheld} names nothing {declaration} hands over"
        )

    declaring_file = f"{module}.py"
    declared = (declaring_file, _function(declaring_file, declaration))
    sanctioned = {name: _locate(name) for name in also_reached_from}
    core_file, _core_def = _locate(core)

    # No module outside the core's own home, the declaration's home and the sanctioned callers'
    # homes names the core at all -- not by import, not as an attribute, since either can be
    # rebound and called under a name the call sweep would not recognise.
    core_module = core_file.removesuffix(".py")
    allowed = {core_file, declaring_file} | {path for path, _node in sanctioned.values()}
    outsiders = sorted(
        path.name
        for path in PACKAGE.glob("*.py")
        if path.name not in allowed
        for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"), filename=str(path)))
        if (isinstance(node, ast.Attribute) and node.attr == core)
        or (
            isinstance(node, ast.ImportFrom)
            # A relative `from .flats import ...` carries the bare module name and a level.
            and node.module in (f"quiddity.{core_module}", core_module)
            and any(alias.name == core for alias in node.names)
        )
    )
    assert outsiders == [], f"{core} is named outside {sorted(allowed)} by {outsiders}"

    # The sanctioned sites are derived from the functions located above rather than counted, so
    # a second call inside a sanctioned function, or a first call from any other function in an
    # allowed file, both fail -- neither is visible to a count, and the sweep above skips those
    # files by construction.
    expected = sorted(
        [(declaring_file, declaration)] + [(path, node.name) for path, node in sanctioned.values()]
    )
    assert sorted(_call_sites(core)) == expected, (
        f"{core} call sites: {sorted(_call_sites(core))}, sanctioned {expected}"
    )

    # Which call is which is decided by the function holding it, not by the keywords it carries:
    # a declaration and an entry point that swapped keywords would otherwise both look right.
    passed = _spelled_keywords(_the_core_call(declared, core), declaration)
    assert passed == handed_over, f"{declaration} hands over {passed}, pinned as {handed_over}"

    for caller, withheld in also_reached_from.items():
        spelled = _spelled_keywords(_the_core_call(sanctioned[caller], core), caller)
        handed = sorted(keyword for keyword in spelled if keyword in withheld)
        assert handed == [], f"{caller} hands the core {handed}, which only the declaration may"


def _function(filename: str, name: str) -> ast.FunctionDef:
    tree = ast.parse((PACKAGE / filename).read_text(encoding="utf-8"), filename=filename)
    function = next(
        (node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == name),
        None,
    )
    assert function is not None, f"{filename} has no module-level `def {name}`"
    return function
