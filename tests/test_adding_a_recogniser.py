"""The registration sites in ``docs/adding-a-recogniser.md`` are real, counted, and complete.

The guide's section 7 is a table of every file a new family must touch. This keeps that table
from drifting: each path exists, the number of rows is pinned so that removing a site is a
deliberate edit here, and every exported family is present at every site that can be checked
by text.
"""

from __future__ import annotations

import re
from pathlib import Path

import quiddity
from quiddity._registry import DERIVED_DEFINITIONS, PHYSICAL_DEFINITIONS, Counted

ROOT = Path(__file__).parents[1]
GUIDE = ROOT / "docs" / "adding-a-recogniser.md"

#: Hand-edit sites a new family needs, measured on the gussets family (#602) on 2026-09-15.
#: Lower this only when a site has been derived from the registry.
#: 10 -> 9: the snapshot tool's package-originated list is derived from the registry.
REGISTRATION_SITES = 9


def _registration_sites() -> list[Path]:
    text = GUIDE.read_text(encoding="utf-8")
    section = text[text.index("## 7. ") : text.index("## 8. ")]
    rows = [line for line in section.splitlines() if re.match(r"\| \d+ \|", line)]
    return [ROOT / re.search(r"`([^`]+)`", row).group(1) for row in rows]


def _exported_definitions():
    exported = set(quiddity.__all__)
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS):
        if definition.public_entrypoint in exported:
            yield definition


def test_every_registration_site_the_guide_names_exists() -> None:
    missing = [path for path in _registration_sites() if not path.exists()]
    assert missing == []


def test_registration_site_count_is_pinned() -> None:
    assert len(_registration_sites()) == REGISTRATION_SITES


def test_every_exported_family_is_present_at_every_text_checkable_site() -> None:
    sources = {
        name: (ROOT / name).read_text(encoding="utf-8")
        for name in (
            "src/quiddity/__init__.py",
            "src/quiddity/result.py",
            "src/quiddity/census.py",
            "src/quiddity/capabilities.json",
            "docs/capabilities.md",
        )
    }
    absent: list[tuple[str, str]] = []
    for definition in _exported_definitions():
        entrypoint = definition.public_entrypoint
        checks = {
            "src/quiddity/__init__.py": entrypoint,
            "src/quiddity/result.py": f"    {definition.result_field}: tuple[",
            "src/quiddity/capabilities.json": f'"quiddity.{entrypoint}"',
            "docs/capabilities.md": f"`{entrypoint}`",
        }
        if isinstance(definition.census, Counted):
            checks["src/quiddity/census.py"] = f'"{definition.census.key}"'
        absent.extend(
            (entrypoint, site) for site, needle in checks.items() if needle not in sources[site]
        )
    assert absent == []
