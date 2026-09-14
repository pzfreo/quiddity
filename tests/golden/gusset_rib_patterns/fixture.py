from build123d import Align, Box, Location, Plane, Polygon, extrude

from tests.golden._common import originated_here

PROVENANCE = originated_here("tests/test_gussets.py")


def build_fixture():
    on_plane = (Align.CENTER, Align.CENTER, Align.MIN)
    plate = Box(80, 50, 8, align=on_plane)
    flange = Box(80, 8, 48, align=on_plane).moved(Location((0, 21, 0)))
    gusset = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    return plate + flange + gusset.moved(Location((-26, 0, 0))) + gusset.moved(Location((32, 0, 0)))
