"""MFCAD++ inputs with unprovable holes must keep the aggregate available."""

import gzip
from pathlib import Path

import pytest

from quiddity import feature_census, import_step_geometry
from quiddity.document import build_recognition_document
from quiddity.result import _take_inventory

_CORPUS = Path(__file__).parent / "corpus" / "inventory_refusal"


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
    if model_id == "14052":
        assert len(product.result.holes) == 1
        assert build_recognition_document(part)["proof"] == "local_degradation"
    else:
        assert not part.solids()
        assert not product.result.holes
