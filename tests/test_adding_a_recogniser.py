"""The registration sites in ``docs/adding-a-recogniser.md`` are real, counted, and complete.

The guide's section 7 is a table of every file a new family must touch. This keeps that table
from drifting: each path exists, the number of files is pinned so that adding or removing a
site is a deliberate edit here, and every exported family is present at every site that can be
checked by text.
"""

from __future__ import annotations

import re
from pathlib import Path

import quiddity
from quiddity._registry import DERIVED_DEFINITIONS, PHYSICAL_DEFINITIONS, Counted

ROOT = Path(__file__).parents[1]
GUIDE = ROOT / "docs" / "adding-a-recogniser.md"

#: Files a new family must hand-edit, measured on the gussets family (#602) on 2026-09-15 as
#: sixteen. Changes only when a site is derived from the registry or a new one is found.
#: 16 -> 15: the snapshot tool's package-originated list is derived from the registry.
#: 15 -> 14: a family declares itself in its module; the manifest tool reads that declaration.
#: 14 -> 11: three test rosters that restated the registry now read it.
REGISTRATION_SITES = 11


def _registration_sites() -> list[Path]:
    """Every backticked path in the Site column of the section 7 table.

    Rows are ``| n | site | add | check |`` with single-space cell padding; the table is not
    reformatted by any tool configured in this repository.
    """

    text = GUIDE.read_text(encoding="utf-8")
    section = text[text.index("## 7. ") : text.index("## 8. ")]
    rows = [line for line in section.splitlines() if re.match(r"\| \d+ \|", line)]
    return [
        ROOT / path
        for row in rows
        for path in re.findall(r"`((?:src|tools|tests|docs)/[^`]+)`", row.split("|")[2])
    ]


def _exported_definitions():
    exported = set(quiddity.__all__)
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS):
        if definition.public_entrypoint in exported:
            yield definition


def _result_class_body() -> str:
    source = (ROOT / "src/quiddity/result.py").read_text(encoding="utf-8")
    start = source.index("\nclass RecognitionResult")
    return source[start : source.index("\nclass ", start + 1)]


def test_every_registration_site_the_guide_names_exists() -> None:
    missing = [path for path in _registration_sites() if not path.exists()]
    assert missing == []


def test_registration_site_count_is_pinned() -> None:
    assert len(_registration_sites()) == REGISTRATION_SITES


def test_exports_are_exactly_the_registry_entry_points_less_the_retired_ones() -> None:
    """Closes the loop the check below relies on, in both directions.

    A registry family dropped from ``__all__`` would otherwise silently drop out of every
    needle below; a name in ``__all__`` with no registry definition would be unreachable by
    the aggregate. The retired names are the converged recess detectors and the legacy
    passage entry, which stay in the registry but are no longer exported.
    """

    from tools._legacy_recognition import __all__ as retired

    exported = {name for name in quiddity.__all__ if name.startswith("recognise_")}
    entry_points = {
        definition.public_entrypoint
        for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
        if definition.public_entrypoint is not None
    }
    assert exported == entry_points - set(retired)


def test_every_exported_family_is_present_at_every_text_checkable_site() -> None:
    sources = {
        "src/quiddity/result.py": _result_class_body(),
        **{
            name: (ROOT / name).read_text(encoding="utf-8")
            for name in (
                "src/quiddity/census.py",
                "src/quiddity/capabilities.json",
                "docs/capabilities.md",
            )
        },
    }
    absent: list[tuple[str, str]] = []
    for definition in _exported_definitions():
        entrypoint = definition.public_entrypoint
        checks = {
            "src/quiddity/result.py": f"    {definition.result_field}: tuple[",
            "src/quiddity/capabilities.json": f'"quiddity.{entrypoint}"',
            "docs/capabilities.md": f"| `{entrypoint}` |",
        }
        if isinstance(definition.census, Counted):
            checks["src/quiddity/census.py"] = f'"{definition.census.key}"'
        absent.extend(
            (entrypoint, site) for site, needle in checks.items() if needle not in sources[site]
        )
    assert absent == []
