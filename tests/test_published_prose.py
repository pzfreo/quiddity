"""Published runtime prose must make sense outside Draftwright's private history."""

from __future__ import annotations

import importlib
import re
from pathlib import Path

ROOT = Path(__file__).parents[1]
RUNTIME = ROOT / "src" / "quiddity"
README = ROOT / "README.md"
ADR_DIR = ROOT / "docs" / "adr"


def _runtime_sources() -> list[Path]:
    return sorted(RUNTIME.glob("*.py"))


def _package_adr_numbers() -> set[str]:
    """The ADR numbers this package publishes, read from the records themselves.

    Draftwright's historical ADRs share this numbering, so a citation is only checkable against
    what ``docs/adr`` actually contains. Deriving it keeps a new package ADR from being read as a
    reference to a private consumer record.
    """

    return {path.name[:4] for path in ADR_DIR.glob("0*.md")}


def test_runtime_prose_has_no_bare_issue_references() -> None:
    unresolved = {
        path.relative_to(ROOT).as_posix(): sorted(set(re.findall(r"#\d+[a-z]?", text)))
        for path in _runtime_sources()
        if (text := path.read_text(encoding="utf-8")) and re.search(r"#\d+[a-z]?", text)
    }
    assert unresolved == {}


def test_runtime_prose_only_uses_normative_package_adrs() -> None:
    package_adrs = _package_adr_numbers()
    assert package_adrs, "docs/adr must contain the records runtime prose is checked against"

    historical = {
        path.relative_to(ROOT).as_posix(): cited
        for path in _runtime_sources()
        if (text := path.read_text(encoding="utf-8"))
        and (cited := sorted(set(re.findall(r"\bADRs?\s+(\d{4})\b", text)) - package_adrs))
    }
    assert historical == {}


def test_the_normative_adr_check_still_rejects_a_consumer_record() -> None:
    """Deriving the allowed set must not make the check above vacuous.

    Draftwright ADRs 0015 and 0017 are historical inputs this package does not publish, so they
    must stay unciteable from runtime prose however many records ``docs/adr`` gains. Package ADR
    0013 is now the independently numbered public Blend decision.
    """

    assert _package_adr_numbers().isdisjoint({"0015", "0017"})


def test_runtime_prose_has_no_consumer_internal_paths() -> None:
    private_paths = {
        "model/declare",
        "model.declare",
        "model/detect.py",
        "sheet_emit",
        "staircase.step",
        "analysis._",
        "recognition/",
        "build_part_model",
        "_analyse",
        "detect._",
        "declare.",
        "analysis.py",
        "Draftwright",
        "draftwright",
    }
    # The replacement package's public JSON format ID is not a consumer-internal path.
    unresolved = {
        path.relative_to(ROOT).as_posix(): sorted(token for token in private_paths if token in text)
        for path in _runtime_sources()
        if (text := path.read_text(encoding="utf-8").replace("quiddity-rust/recognition/6", ""))
        and any(token in text for token in private_paths)
    }
    assert unresolved == {}


def test_readme_examples_only_use_api_the_package_actually_exports() -> None:
    """The README's code must run, which is the property its wording was standing in for.

    This replaces a set of assertions on particular sentences — ``"STEP editor" in readme`` and
    similar. Those failed on copy edits while passing on a README that documented a function
    that no longer existed, which is the wrong way round. Every name imported from the package
    in a fenced example is checked against the real export list instead.
    """

    readme = README.read_text(encoding="utf-8")
    exported = set(dir(importlib.import_module("quiddity")))

    imported = {
        name.strip()
        for match in re.findall(r"^from quiddity import (.+)$", readme, re.MULTILINE)
        for name in match.split(",")
    }

    assert imported, "the README must show how the package is imported"
    missing = sorted(imported - exported)
    assert not missing, f"README imports names the package does not export: {missing}"


def test_readme_shows_the_step_entry_point_the_package_is_for() -> None:
    """One claim worth pinning: the README demonstrates recognition from an imported file.

    Not a wording assertion — it checks the example calls the supported geometry-only STEP loader
    and feeds the result to a recogniser, which is the workflow the package exists to serve. A
    README that stopped showing it would be documenting a different library.
    """

    readme = README.read_text(encoding="utf-8")

    assert re.search(r"import_step_geometry\(", readme)
    assert re.search(r"build_framed_recognition_result\(\s*part\s*\)", readme)
