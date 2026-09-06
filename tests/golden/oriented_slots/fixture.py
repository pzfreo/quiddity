from build123d import Align, Box, Pos, Rot

from tests.golden._common import originated_here

PROVENANCE = originated_here("tests/test_oriented_slots.py")
LEGACY_SNAPSHOT = False


def build_fixture():
    part = Box(120, 90, 10)
    for x in (-30, 0, 30):
        part -= (
            Pos(x, 0, 0)
            * Rot(0, 0, 30)
            * Box(24, 6, 20, align=(Align.CENTER, Align.CENTER, Align.CENTER))
        )
    return part
