from build123d import Box, Plane, Polygon, Pos, extrude

from tests.golden._common import originated_here

PROVENANCE = originated_here("tests/test_paired_ramp_steps.py")


def build_fixture():
    stock = Box(40, 40, 30)
    cutter = Pos(20, 20, 0) * extrude(Plane.XZ * Polygon((0, -8), (0, 8), (-10, 0)), 25)
    return stock - cutter
