from build123d import Align, Box, Cylinder, Location, Plane, Polygon, extrude

from tests.golden._common import originated_here

PROVENANCE = originated_here("tests/test_gussets.py")


def build_fixture():
    on_plane = (Align.CENTER, Align.CENTER, Align.MIN)
    plate = Box(80, 50, 8, align=on_plane)
    flange = Box(80, 8, 48, align=on_plane).moved(Location((0, 21, 0)))
    boss = Cylinder(10, 12, align=on_plane).moved(Location((0, 3, 8)))
    gusset = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    part = (
        plate
        + flange
        + boss
        + gusset.moved(Location((-32, 0, 0)))
        + gusset.moved(Location((26, 0, 0)))
    )
    slot = (
        Box(18, 20, 8)
        + Cylinder(4, 20, rotation=(90, 0, 0)).moved(Location((-9, 0, 0)))
        + Cylinder(4, 20, rotation=(90, 0, 0)).moved(Location((9, 0, 0)))
    ).moved(Location((0, 21, 32)))
    part -= slot
    part -= Cylinder(5, 40, align=on_plane).moved(Location((0, 3, -1)))
    for x in (-25, 25):
        part -= Cylinder(4, 20, align=on_plane).moved(Location((x, -13, -1)))
    return part
