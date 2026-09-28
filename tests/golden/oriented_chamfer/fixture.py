from build123d import Box, Rot, chamfer

from tests.golden._common import originated_here

PROVENANCE = originated_here("tests/test_oriented_chamfers.py")


def build_fixture():
    box = Rot(0, 0, 30) * Box(40, 30, 20)
    edge = next(edge for edge in box.edges() if abs(edge.center().Z - 10) < 1e-6)
    return chamfer(edge, length=2)
