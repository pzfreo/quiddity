"""Private tooling imports for frozen detector-baseline measurements, not consumer API."""

from quiddity.edge_open_circular_recesses import (
    EdgeOpenCircularPocket,
    OpenCircularSection,
    OpenCircularSectionSegment,
    recognise_edge_open_circular_pockets,
)
from quiddity.edge_open_prismatic_recesses import (
    EdgeOpenPrismaticRecess,
    OpenPolygonalSection,
    OpenSectionOpening,
    recognise_edge_open_prismatic_recesses,
)
from quiddity.passages import (
    Passage,
    PassageCompatibilityError,
    PassageEnds,
    SectionPassage,
    recognise_passages,
    recognise_section_passages,
)
from quiddity.prismatic_pockets import (
    PrismaticPocket,
    recognise_prismatic_pockets,
)
from quiddity.rectangular_blind_slots import (
    RectangularBlindSlot,
    recognise_rectangular_blind_slots,
)
from quiddity.round_bottom_slots import (
    RoundBottomBlindSlot,
    recognise_round_bottom_blind_slots,
)
from quiddity.slots import (
    Channel,
    Pocket,
    PocketArray,
    PocketGrid,
    recognise_channels,
    recognise_pocket_patterns,
    recognise_pockets,
)

__all__ = [
    "Pocket",
    "PocketArray",
    "PocketGrid",
    "PrismaticPocket",
    "Channel",
    "RectangularBlindSlot",
    "RoundBottomBlindSlot",
    "EdgeOpenCircularPocket",
    "OpenCircularSection",
    "OpenCircularSectionSegment",
    "EdgeOpenPrismaticRecess",
    "OpenPolygonalSection",
    "OpenSectionOpening",
    "Passage",
    "PassageEnds",
    "SectionPassage",
    "PassageCompatibilityError",
    "recognise_pockets",
    "recognise_pocket_patterns",
    "recognise_channels",
    "recognise_prismatic_pockets",
    "recognise_rectangular_blind_slots",
    "recognise_round_bottom_blind_slots",
    "recognise_edge_open_circular_pockets",
    "recognise_edge_open_prismatic_recesses",
    "recognise_passages",
    "recognise_section_passages",
]


def build_raw_recognition_result(part, *, cylinders=None, rotational=False):
    from quiddity.result import _take_inventory

    return _take_inventory(part, cylinders=cylinders, rotational=rotational)._legacy_result


build_recognition_result = build_raw_recognition_result


def detector_outputs_equal(left, right, *, excluding=()):
    """Compare detector fields, not their derived unified API projections."""
    from dataclasses import fields

    excluded = {
        "section_recesses",
        "section_recess_refusals",
        "section_recess_patterns",
        *excluding,
    }
    return all(
        getattr(left, field.name) == getattr(right, field.name)
        for field in fields(left)
        if field.name not in excluded
    )


def feature_census(part):
    from quiddity.census import _LEGACY_CENSUS_BINDINGS
    from quiddity.result import _take_inventory

    product = _take_inventory(part)
    return {
        key: len(product.distinct_steps.candidates)
        if key == "step"
        else len(getattr(product._legacy_result, field))
        for key, field in _LEGACY_CENSUS_BINDINGS
    }


def namespace():
    """Frozen detector-view adapter used only by the historic golden snapshot tools."""
    from types import SimpleNamespace

    import quiddity as public

    values = {name: getattr(public, name) for name in public.__all__}
    values.update({name: globals()[name] for name in __all__})
    values.update(
        build_recognition_result=build_recognition_result,
        build_raw_recognition_result=build_raw_recognition_result,
    )
    values["__all__"] = sorted(set(public.__all__) | set(__all__))
    return SimpleNamespace(**values)
