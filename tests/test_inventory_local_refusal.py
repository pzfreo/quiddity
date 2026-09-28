"""MFCAD++ inputs with unprovable holes must keep the aggregate available.

Test-split models 13975 and 14052 are vendored under CC BY. Their uncompressed source
SHA-256 values are 20226562f507df001a322a3eccde6826690598ed97798d5e81be2dc2b058dd8c
and 91456bd801b9ebc6f8f13c8b19aba71e9dea2b7a7426f86fa56bafc17c13afef.
"""

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
