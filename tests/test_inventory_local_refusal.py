"""MFCAD++ inputs with unprovable holes must keep the aggregate available.

Test-split models 13975 and 14052 are vendored under CC BY. Their uncompressed source
SHA-256 values are 20226562f507df001a322a3eccde6826690598ed97798d5e81be2dc2b058dd8c
and 91456bd801b9ebc6f8f13c8b19aba71e9dea2b7a7426f86fa56bafc17c13afef.
"""

import gzip
from pathlib import Path

import pytest
from build123d import Box
from OCP.BRepCheck import BRepCheck_Analyzer

from quiddity import feature_census, import_step_geometry
from quiddity._ocp_compat import brepcheck_status_ok
from quiddity.document import build_recognition_document
from quiddity.result import _take_inventory

_CORPUS = Path(__file__).parent / "corpus" / "inventory_refusal"
_BOX = Box(1, 1, 1).solids()[0].wrapped
# OCP 8.0.1.1 cannot read BRepCheck statuses, so local degradation has no proof there.
_STATUS_READABLE = brepcheck_status_ok(BRepCheck_Analyzer(_BOX), _BOX) is not None


def _part(model_id: str, tmp_path: Path):
    source = tmp_path / f"{model_id}.step"
    source.write_bytes(gzip.decompress((_CORPUS / f"{model_id}.step.gz").read_bytes()))
    return import_step_geometry(source)


@pytest.mark.parametrize("model_id", ("14052", "13975"))
def test_inventory_contains_unproved_hole_on_invalid_solid_or_open_shell(
    model_id: str, tmp_path: Path
) -> None:
    if not (_CORPUS / f"{model_id}.step.gz").is_file():
        pytest.skip("the optional MFCAD++ corpus is unavailable")
    part = _part(model_id, tmp_path)
    with pytest.raises(
        ValueError, match="Hole cylindrical evidence does not prove one valid solid"
    ):
        _take_inventory(part, local_degradation=False)

    product = _take_inventory(part)
    assert product.result == _take_inventory(part, local_degradation=True).result
    counts = feature_census(part)
    assert counts["hole"] == len(product.result.holes)
    if model_id == "14052" and _STATUS_READABLE:
        assert len(product.result.holes) == 1
        assert build_recognition_document(part)["proof"] == "local_degradation"
    elif model_id == "14052":
        assert not product.result.holes
        document = build_recognition_document(part)
        assert document["proof"] == "local_degradation"
        assert {face["proof"] for face in document["faces"]} == {"not_proven"}
    else:
        assert not part.solids()
        assert not product.result.holes
