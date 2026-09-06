# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Immutable ADR-0019 records and validation, without discovery or kernel operations.

The public facade re-exports these exact types. Kernel-backed section proofs and projection
live in _section_recess_geometry; orchestration and publication remain with their owners.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import cast

from quiddity._cylindrical_end_surface import CylindricalEndSurface as CylindricalEndSurface
from quiddity._record import Record
from quiddity._sections import (
    SectionVertex,
    _validate_simple,
    validate_section_end_separation,
)
from quiddity.passages import PassageFrame, PassageSection, PassageSectionVertex

_FEATURE_KINDS = frozenset({"pocket", "edge_open_recess", "passage", "channel"})
_SECTION_SHAPES = frozenset(
    {
        "rectangular",
        "circular",
        "obround",
        "triangular",
        "hexagonal",
        "polygonal",
        "general",
    }
)
Vector2 = tuple[float, float]
Vector3 = tuple[float, float, float]


def _numbers(value: object, size: int, *, name: str) -> tuple[float, ...]:
    if (
        not isinstance(value, tuple)
        or len(value) != size
        or any(isinstance(item, bool) or not isinstance(item, int | float) for item in value)
    ):
        raise ValueError(f"{name} must contain {size} finite numbers")
    result = tuple(float(item) for item in value)
    if not all(math.isfinite(item) for item in result):
        raise ValueError(f"{name} must contain {size} finite numbers")
    return result


@dataclass(frozen=True, order=True, slots=True)
class ClosedSectionProfile(Record):
    """One canonical physical closed line/arc boundary."""

    closure: str
    boundary: tuple[PassageSectionVertex, ...]

    def __post_init__(self) -> None:
        if self.closure != "closed":
            raise ValueError("closed section profile closure must be 'closed'")
        PassageSection(self.boundary)


@dataclass(frozen=True, order=True, slots=True)
class OpenSectionProfile(Record):
    """One canonical physical open line/arc chain plus its explicitly absent boundary."""

    closure: str
    boundary: tuple[PassageSectionVertex, ...]
    opening: tuple[Vector2, Vector2]

    def __post_init__(self) -> None:
        if self.closure != "open":
            raise ValueError("open section profile closure must be 'open'")
        if (
            not isinstance(self.boundary, tuple)
            or len(self.boundary) < 2
            or not all(isinstance(vertex, PassageSectionVertex) for vertex in self.boundary)
        ):
            raise ValueError("open section profile requires at least two physical vertices")
        if self.boundary[-1].bulge != 0.0:
            raise ValueError("the final open-profile vertex cannot imply a closing segment")
        if len({vertex.point for vertex in self.boundary}) != len(self.boundary):
            raise ValueError("open section profile vertices must be distinct")
        _validate_simple(
            tuple(SectionVertex(vertex.point, vertex.bulge) for vertex in self.boundary),
            closed=False,
        )
        opening = cast(
            tuple[Vector2, Vector2],
            tuple(
                cast(Vector2, _numbers(point, 2, name="opening endpoint")) for point in self.opening
            ),
        )
        if opening != (self.boundary[-1].point, self.boundary[0].point):
            raise ValueError("opening must run from the physical chain end to its start")
        reversed_boundary = tuple(
            PassageSectionVertex(
                self.boundary[-1 - index].point,
                -self.boundary[-2 - index].bulge if index < len(self.boundary) - 1 else 0.0,
            )
            for index in range(len(self.boundary))
        )
        if reversed_boundary < self.boundary:
            raise ValueError("open section profile must use its canonical direction")
        object.__setattr__(self, "opening", opening)


@dataclass(frozen=True, order=True, slots=True)
class PlanarEndSurface(Record):
    """Plane through the centroid end coordinate, with local section gradients."""

    type: str = "plane"
    gradient: tuple[float, float] = (0.0, 0.0)

    def __post_init__(self) -> None:
        if self.type != "plane":
            raise ValueError("planar end surface type must be 'plane'")
        gradient = cast(tuple[float, float], _numbers(self.gradient, 2, name="gradient"))
        if any(round(value, 6) != value for value in gradient):
            raise ValueError("section end gradient must serialize at six decimal places")
        object.__setattr__(self, "gradient", gradient)


@dataclass(frozen=True, order=True, slots=True)
class SectionEnd(Record):
    """Physical end condition and explicitly discriminated analytic surface."""

    condition: str
    surface: PlanarEndSurface | CylindricalEndSurface = PlanarEndSurface()

    def __post_init__(self) -> None:
        if self.condition not in {"open", "capped"}:
            raise ValueError("section end condition must be 'open' or 'capped'")
        if not isinstance(self.surface, PlanarEndSurface | CylindricalEndSurface):
            raise ValueError("section end requires an explicit analytic surface")


@dataclass(frozen=True, order=True, slots=True)
class SectionRecessEnds(Record):
    low: SectionEnd
    high: SectionEnd

    def __post_init__(self) -> None:
        if not isinstance(self.low, SectionEnd) or not isinstance(self.high, SectionEnd):
            raise ValueError("section recess ends must contain SectionEnd values")


@dataclass(frozen=True, order=True, slots=True)
class SectionRecessGeometry(Record):
    """The reconstructible constant-section geometry selected by ADR 0019."""

    type: str
    frame: PassageFrame
    run_interval: tuple[float, float]
    profile: ClosedSectionProfile | OpenSectionProfile
    ends: SectionRecessEnds

    def __post_init__(self) -> None:
        if self.type != "section_recess":
            raise ValueError("geometry type must be 'section_recess'")
        if not isinstance(self.frame, PassageFrame):
            raise ValueError("section recess requires a PassageFrame")
        interval = cast(tuple[float, float], _numbers(self.run_interval, 2, name="run_interval"))
        if interval[1] - interval[0] <= 1e-9 or any(round(value, 3) != value for value in interval):
            raise ValueError("run_interval must increase and serialize at three decimals")
        if not isinstance(self.profile, ClosedSectionProfile | OpenSectionProfile):
            raise ValueError("a section recess requires a closed or open section profile")
        if not isinstance(self.ends, SectionRecessEnds):
            raise ValueError("section recess requires explicit ends")
        low_surface, high_surface = self.ends.low.surface, self.ends.high.surface
        if isinstance(low_surface, PlanarEndSurface) and isinstance(high_surface, PlanarEndSurface):
            validate_section_end_separation(
                tuple(
                    SectionVertex(vertex.point, vertex.bulge) for vertex in self.profile.boundary
                ),
                interval[1] - interval[0],
                (
                    high_surface.gradient[0] - low_surface.gradient[0],
                    high_surface.gradient[1] - low_surface.gradient[1],
                ),
                closed=isinstance(self.profile, ClosedSectionProfile),
            )
        else:
            self._validate_cylindrical_end(interval)
        object.__setattr__(self, "run_interval", interval)

    def _validate_cylindrical_end(self, interval: tuple[float, float]) -> None:
        if not isinstance(self.profile, ClosedSectionProfile) or any(
            vertex.bulge != 0 for vertex in self.profile.boundary
        ):
            raise ValueError("cylindrical end currently requires a closed polygonal profile")
        ends = (self.ends.low, self.ends.high)
        curved_indices = [
            i for i, end in enumerate(ends) if isinstance(end.surface, CylindricalEndSurface)
        ]
        if len(curved_indices) != 1:
            raise ValueError("cylindrical pocket requires exactly one cylindrical end")
        index = curved_indices[0]
        curved = cast(CylindricalEndSurface, ends[index].surface)
        floor = ends[1 - index]
        if (
            ends[index].condition != "open"
            or floor.condition != "capped"
            or not isinstance(floor.surface, PlanarEndSurface)
            or floor.surface.gradient != (0.0, 0.0)
            or curved.branch != ("positive" if index == 1 else "negative")
        ):
            raise ValueError(
                "cylindrical pocket needs an outward open branch and flat capped floor"
            )
        points = tuple(vertex.point for vertex in self.profile.boundary)
        low, high = curved.polygon_height_bounds(points)
        separation = low - interval[0] if index == 1 else interval[1] - high
        if separation <= 1e-9:
            raise ValueError("cylindrical pocket ends must remain strictly separated")
        if round(curved.height((0.0, 0.0)), 3) != interval[index]:
            raise ValueError("run interval must agree with the cylindrical centroid intersection")


@dataclass(frozen=True, order=True, slots=True)
class SectionRecessClassification(Record):
    feature_kind: str
    section_shape: str

    def __post_init__(self) -> None:
        if self.feature_kind not in _FEATURE_KINDS:
            raise ValueError("unsupported section recess feature_kind")
        if self.section_shape not in _SECTION_SHAPES:
            raise ValueError("unsupported section recess section_shape")


@dataclass(frozen=True, order=True, slots=True)
class SectionRecessEvidence(Record):
    defining_faces: tuple[int, ...]
    constituent_faces: tuple[int, ...]

    def __post_init__(self) -> None:
        for name, values in (
            ("defining_faces", self.defining_faces),
            ("constituent_faces", self.constituent_faces),
        ):
            if (
                not isinstance(values, tuple)
                or any(type(value) is not int or value < 0 for value in values)
                or tuple(sorted(set(values))) != values
            ):
                raise ValueError(f"{name} must be sorted unique non-negative indices")
        if not set(self.defining_faces) <= set(self.constituent_faces):
            raise ValueError("defining faces must be constituent faces")


@dataclass(frozen=True, order=True, slots=True)
class SectionRecessBodyRef(Record):
    index: int

    def __post_init__(self) -> None:
        if type(self.index) is not int or self.index < 0:
            raise ValueError("body index must be a non-negative integer")


@dataclass(frozen=True, order=True, slots=True)
class SectionRecessFaceRef(Record):
    index: int

    def __post_init__(self) -> None:
        if type(self.index) is not int or self.index < 0:
            raise ValueError("face index must be a non-negative integer")


@dataclass(frozen=True, order=True, slots=True)
class SectionRecess(Record):
    index: int
    body: int
    geometry: SectionRecessGeometry
    classification: SectionRecessClassification
    evidence: SectionRecessEvidence

    def __post_init__(self) -> None:
        if type(self.index) is not int or self.index < 0:
            raise ValueError("occurrence index must be a non-negative integer")
        if type(self.body) is not int or self.body < 0:
            raise ValueError("occurrence body must be a non-negative integer")
        if not isinstance(self.geometry, SectionRecessGeometry):
            raise ValueError("occurrence requires section recess geometry")
        if not isinstance(self.classification, SectionRecessClassification):
            raise ValueError("occurrence requires authoritative classification")
        if not isinstance(self.evidence, SectionRecessEvidence):
            raise ValueError("occurrence requires face evidence")
        capped = sum(
            end.condition == "capped" for end in (self.geometry.ends.low, self.geometry.ends.high)
        )
        admitted = {
            "pocket": isinstance(self.geometry.profile, ClosedSectionProfile) and capped == 1,
            "edge_open_recess": (
                isinstance(self.geometry.profile, OpenSectionProfile) and capped == 1
            ),
            "passage": isinstance(self.geometry.profile, ClosedSectionProfile) and capped == 0,
            "channel": isinstance(self.geometry.profile, OpenSectionProfile) and capped == 0,
        }
        if not admitted[self.classification.feature_kind]:
            raise ValueError("classification, profile closure and end topology are inconsistent")


@dataclass(frozen=True, slots=True)
class SectionRecessRefusal(Record):
    """Source evidence for an internal candidate that cannot issue truthful unified geometry."""

    body: int
    reason: str
    evidence: SectionRecessEvidence

    def __post_init__(self) -> None:
        if type(self.body) is not int or self.body < 0:
            raise ValueError("refusal body must be a non-negative integer")
        if self.reason != "unsupported_support_geometry":
            raise ValueError("unsupported section projection refusal")
        if not isinstance(self.evidence, SectionRecessEvidence):
            raise ValueError("refusal requires source-face evidence")


@dataclass(frozen=True, slots=True)
class SectionRecessArray(Record):
    members: tuple[int, ...]
    pitch: float
    direction: Vector3

    def __post_init__(self) -> None:
        _pattern_members(self.members)
        (pitch,) = _numbers((self.pitch,), 1, name="array pitch")
        if pitch <= 0:
            raise ValueError("array pitch must be positive")
        direction = _numbers(self.direction, 3, name="array direction")
        if not math.isclose(sum(value * value for value in direction), 1.0, abs_tol=1e-6):
            raise ValueError("array direction must be unit length")


@dataclass(frozen=True, slots=True)
class SectionRecessGrid(Record):
    members: tuple[int, ...]
    rows: int
    cols: int
    row_pitch: float
    col_pitch: float
    row_direction: Vector3
    col_direction: Vector3
    center: Vector3

    def __post_init__(self) -> None:
        _pattern_members(self.members)
        if any(type(n) is not int or n < 2 for n in (self.rows, self.cols)):
            raise ValueError("grid requires at least two rows and columns")
        if self.rows * self.cols != len(self.members):
            raise ValueError("grid dimensions must match member count")
        pitches = _numbers((self.row_pitch, self.col_pitch), 2, name="grid pitches")
        if any(n <= 0 for n in pitches):
            raise ValueError("grid pitches must be positive")
        row = _numbers(self.row_direction, 3, name="grid row direction")
        col = _numbers(self.col_direction, 3, name="grid column direction")
        if any(
            not math.isclose(sum(v * v for v in direction), 1.0, abs_tol=1e-6)
            for direction in (row, col)
        ):
            raise ValueError("grid directions must be unit length")
        if abs(sum(a * b for a, b in zip(row, col, strict=True))) > 1e-6:
            raise ValueError("grid directions must be perpendicular")
        _numbers(self.center, 3, name="grid center")


def _pattern_members(members: tuple[int, ...]) -> None:
    if (
        not isinstance(members, tuple)
        or len(members) < 2
        or any(type(n) is not int or n < 0 for n in members)
        or len(set(members)) != len(members)
    ):
        raise ValueError("pattern requires distinct non-negative occurrence indices")


@dataclass(frozen=True, slots=True)
class SectionRecessDocument(Record):
    schema_version: int
    reference_scope: str
    bodies: tuple[SectionRecessBodyRef, ...]
    faces: tuple[SectionRecessFaceRef, ...]
    occurrences: tuple[SectionRecess, ...]
    refusals: tuple[SectionRecessRefusal, ...] = ()
    patterns: tuple[SectionRecessArray | SectionRecessGrid, ...] = ()

    def __post_init__(self) -> None:
        if type(self.schema_version) is not int or self.schema_version != 3:
            raise ValueError("unsupported section-recess document")
        if self.reference_scope != "result":
            raise ValueError("unsupported section-recess document")
        for roster, record_type in (
            (self.bodies, SectionRecessBodyRef),
            (self.faces, SectionRecessFaceRef),
            (self.occurrences, SectionRecess),
            (self.refusals, SectionRecessRefusal),
            (self.patterns, (SectionRecessArray, SectionRecessGrid)),
        ):
            if not isinstance(roster, tuple) or not all(
                isinstance(item, record_type) for item in roster
            ):
                raise ValueError("document rosters require immutable typed records")
        if tuple(item.index for item in self.bodies) != tuple(range(len(self.bodies))):
            raise ValueError("body roster must be dense and ordered")
        if tuple(item.index for item in self.faces) != tuple(range(len(self.faces))):
            raise ValueError("face roster must be dense and ordered")
        if tuple(item.index for item in self.occurrences) != tuple(range(len(self.occurrences))):
            raise ValueError("occurrence roster must be dense and ordered")
        referenced: tuple[SectionRecess | SectionRecessRefusal, ...] = (
            *self.occurrences,
            *self.refusals,
        )
        for occurrence in referenced:
            if occurrence.body >= len(self.bodies):
                raise ValueError("occurrence body index is outside the document roster")
            if any(
                face >= len(self.faces)
                for face in (
                    *occurrence.evidence.defining_faces,
                    *occurrence.evidence.constituent_faces,
                )
            ):
                raise ValueError("occurrence face index is outside the document roster")
        for pattern in self.patterns:
            if not isinstance(pattern, SectionRecessArray | SectionRecessGrid):
                raise ValueError("unsupported section pattern")
            if any(index >= len(self.occurrences) for index in pattern.members):
                raise ValueError("pattern member is outside the occurrence roster")
