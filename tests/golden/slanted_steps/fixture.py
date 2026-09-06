from build123d import Align, Box, BuildPart, BuildSketch, Plane, Polygon, Pos, extrude

from tests.golden._common import provenance

PROVENANCE = provenance("tests/test_slanted_blind_step.py")


def build_fixture():
    with BuildPart() as part:
        with BuildSketch(Plane.XZ):
            Polygon((0, 0), (50, 0), (50, 14), (40, 14), (40, 19), (32, 19), (24, 25), (0, 25))
        extrude(amount=50)
    solid = part.part
    bounds = solid.bounding_box()
    align_min = (Align.MIN, Align.MIN, Align.MIN)
    low = Pos(bounds.min.X, bounds.min.Y, bounds.min.Z) * Box(8, 12, 5, align=align_min)
    high = Pos(bounds.min.X, bounds.max.Y - 12, bounds.max.Z - 5) * Box(8, 12, 5, align=align_min)
    return solid - low - high
