"""MFCAD++ inputs with unprovable holes must keep the aggregate available.

Test-split models 13975 and 14052 are vendored under CC BY. Their uncompressed source
SHA-256 values are 20226562f507df001a322a3eccde6826690598ed97798d5e81be2dc2b058dd8c
and 91456bd801b9ebc6f8f13c8b19aba71e9dea2b7a7426f86fa56bafc17c13afef.
"""

import gzip
from pathlib import Path

import pytest
from build123d import Box, Compound, Cylinder, Pos

from quiddity import feature_census, import_step_geometry
from quiddity._ocp_compat import brepcheck_status_ok, brepcheck_statuses_readable
from quiddity.document import build_recognition_document
from quiddity.result import _take_inventory

_CORPUS = Path(__file__).parent / "corpus" / "inventory_refusal"
# OCP 8.0.1.1 cannot read BRepCheck statuses, so local degradation has no proof there.
_STATUS_READABLE = brepcheck_statuses_readable()


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


def test_unreadable_brepcheck_status_is_no_proof() -> None:
    class UnreadableResult:
        def Status(self):  # noqa: N802 - mirrors the OCP method
            raise TypeError("Unable to convert function return value to a Python type!")

    class UnreadableAnalyzer:
        def Result(self, _shape):  # noqa: N802 - mirrors the OCP method
            return UnreadableResult()

    assert brepcheck_status_ok(UnreadableAnalyzer(), object()) is None


def test_local_degradation_refuses_when_statuses_are_unreadable(monkeypatch, tmp_path) -> None:
    """The OCP 8.0.1.1 path, exercised on any OCP: no readable status means no degraded proof."""

    if not (_CORPUS / "14052.step.gz").is_file():
        pytest.skip("the optional MFCAD++ corpus is unavailable")
    import quiddity._adjacency as adjacency
    import quiddity.document as document

    monkeypatch.setattr(adjacency, "brepcheck_status_ok", lambda *_args: None)
    monkeypatch.setattr(document, "brepcheck_status_ok", lambda *_args: None)
    part = _part("14052", tmp_path)

    assert not _take_inventory(part).result.holes
    built = build_recognition_document(part)
    assert built["proof"] == "local_degradation"
    assert {face["proof"] for face in built["faces"]} == {"not_proven"}
    assert not built["features"]


def test_unreadable_statuses_do_not_taint_valid_bodies(monkeypatch, tmp_path) -> None:
    """A valid body next to an invalid one keeps local proof when statuses are unreadable."""

    if not (_CORPUS / "14052.step.gz").is_file():
        pytest.skip("the optional MFCAD++ corpus is unavailable")
    import quiddity._adjacency as adjacency
    import quiddity.document as document

    monkeypatch.setattr(adjacency, "brepcheck_status_ok", lambda *_args: None)
    monkeypatch.setattr(document, "brepcheck_status_ok", lambda *_args: None)
    invalid = _part("14052", tmp_path)
    valid = Pos(500, 0, 0) * (Box(20, 20, 10) - Cylinder(3, 10))
    part = Compound(children=[*invalid.solids(), *valid.solids()])

    built = build_recognition_document(part)
    assert built["proof"] == "local_degradation"
    holes = [feature for feature in built["features"] if feature["record_type"] == "HoleRecord"]
    assert len(holes) == 1
    proofs = {face["index"]: face["proof"] for face in built["faces"]}
    assert {proofs[index] for index in holes[0]["defining_faces"]} == {"local"}
    assert "not_proven" in proofs.values()
