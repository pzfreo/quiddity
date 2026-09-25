"""A local attribution refusal must not discard unrelated accepted geometry."""

from build123d import Box, Cylinder, Pos

from quiddity._adjacency import FaceGraph
from quiddity.result import _take_inventory


def test_one_unsafe_pocket_keeps_a_distant_pocket_and_hole(monkeypatch) -> None:
    part = Box(100, 60, 10)
    part -= Pos(-25, 0, 2) * Box(12, 10, 6)
    part -= Pos(25, 0, 2) * Box(12, 10, 6)
    part -= Pos(0, 20, 0) * Cylinder(3, 20)
    baseline = _take_inventory(part)
    assert len(baseline._legacy_result.pockets) == 2
    assert len(baseline.result.holes) == 1

    original = FaceGraph.common_valid_solid

    def with_one_unsafe_floor(self, nodes):
        members = tuple(nodes)
        if self.local_degradation and any(
            abs(self.face(node).center().X + 25) < 0.01
            and abs(self.face(node).center().Z + 1) < 0.01
            for node in members
        ):
            return None
        return original(self, members)

    monkeypatch.setattr(FaceGraph, "common_valid_solid", with_one_unsafe_floor)
    product = _take_inventory(part, local_degradation=True)
    assert len(product._legacy_result.pockets) == 1
    assert (product._legacy_result.pockets[0].lo, product._legacy_result.pockets[0].hi) == (
        19,
        31,
    )
    assert len(product.result.section_recesses) == 1
    assert len(product.result.holes) == 1
