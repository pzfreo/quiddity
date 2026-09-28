from build123d import Align, Box, Face, Solid, Vector, Wire

from tests.golden._common import originated_here

PROVENANCE = originated_here("tests/test_oblique_through_steps.py")


def build_fixture():
    outline = Wire.make_polygon(
        [Vector(0, y, z) for y, z in ((0, 0), (40, 0), (20, 20), (0, 20))],
        close=True,
    )
    removal = Solid.extrude(Face(outline), Vector(20, 0, 0))
    return Box(40, 40, 20, align=(Align.MIN, Align.MIN, Align.MIN)) - removal
