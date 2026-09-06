"""Release checks for evidence-preserving projection, independent of effectiveness scores."""

from pathlib import Path

import pytest
from build123d import import_step

from quiddity._candidates import FamilyId
from quiddity.result import _take_inventory
from tests.golden._common import load_fixture
from tools.audit_section_recess_migration import audit_product

GOLDEN = Path(__file__).parent / "golden"
CORPUS = Path(__file__).parent / "corpus" / "mfcadpp"
pytestmark = pytest.mark.slow


def _missing_exact(report):
    return [
        row
        for row in report["candidates"]
        if row["status"] == "unrepresented" and row["family"] != FamilyId.POCKETS.value
    ]


@pytest.mark.parametrize("name", sorted(path.parent.name for path in GOLDEN.glob("*/fixture.py")))
def test_every_exact_accepted_golden_recess_has_a_unified_region(name):
    part = load_fixture(GOLDEN / name / "fixture.py").build_fixture()
    report = audit_product(_take_inventory(part))
    assert _missing_exact(report) == []
    assert report["counts"]["unrepresented"] == 0
    assert report["counts"]["explicit_refusal"] == 0


@pytest.mark.parametrize("path", sorted(CORPUS.glob("*.step")), ids=lambda path: path.stem)
def test_every_exact_accepted_development_recess_has_a_unified_region(path):
    report = audit_product(_take_inventory(import_step(path)))
    assert _missing_exact(report) == []
    assert report["counts"]["unrepresented"] == 0
