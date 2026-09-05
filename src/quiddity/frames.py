# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Part-relative recognition with an explicit caller-space frame.

Existing recognition entry points remain caller-space and byte compatible. Framed recognition
pairs the unchanged local-frame ``RecognitionResult`` and exact working shape with the frame
needed to interpret them.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from enum import Enum
from typing import cast

from build123d import Location, Shape
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Cylinder, GeomAbs_Plane
from OCP.gp import gp_Trsf
from OCP.GProp import GProp_GProps
from OCP.TopoDS import TopoDS_Shape

from quiddity._cylinder_substrate import analyse_cylinders
from quiddity._typing import FaceLike, FrozenCylinderInventory, Part, Vector3
from quiddity.evidence import (
    FaceRef,
    FramedEvidenceRefusalReason,
    FramedRecognitionEvidence,
    RefusedFramedEvidence,
    _issue_framed_recognition_evidence,
    _project_recognition_evidence,
)
from quiddity.explanations import RecognitionReport, build_raw_recognition_report
from quiddity.result import (
    RecognitionResult,
    _take_inventory,
    build_raw_recognition_result,
)

_PARALLEL_COS = 0.999
_ORTHOGONAL_COS = 1.0 - _PARALLEL_COS
_COMPONENT_EPS = 1e-12


class FrameGauge(Enum):
    """How much of the returned basis is established by the solid.

    ``FULL`` means the solid establishes a directed, ordered basis. ``ORTHOGONAL`` means it
    establishes at least two perpendicular direction lines, but a discrete sign or interchange
    remains unobservable. ``AXIAL`` means it establishes one direction line while roll about it
    is unobservable. In the latter two cases the returned axes are representatives of the gauge,
    not semantic material directions.
    """

    FULL = "full"
    ORTHOGONAL = "orthogonal"
    AXIAL = "axial"


class FrameRefusalReason(Enum):
    NO_MATERIAL = "no-material"
    NO_ANALYTIC_DIRECTION = "no-analytic-direction"
    NONFINITE_GEOMETRY = "nonfinite-geometry"


@dataclass(frozen=True, slots=True)
class RefusedPartFrame:
    reason: FrameRefusalReason


@dataclass(frozen=True, slots=True)
class PartFrame:
    """Caller-space placement of the local recognition coordinate system.

    A caller-space point ``p`` maps to local coordinates
    ``(dot(p-origin, x), dot(p-origin, y), dot(p-origin, z))``.  The inverse is the corresponding
    linear combination of the three axes plus ``origin``.
    """

    origin: Vector3
    x: Vector3
    y: Vector3
    z: Vector3
    gauge: FrameGauge

    def __post_init__(self) -> None:
        vectors = (self.origin, self.x, self.y, self.z)
        if not all(len(vector) == 3 for vector in vectors):
            raise ValueError("frame values must be 3-vectors")
        if not all(math.isfinite(value) for vector in vectors for value in vector):
            raise ValueError("frame values must be finite")
        for axis in (self.x, self.y, self.z):
            if not math.isclose(_dot(axis, axis), 1.0, rel_tol=0.0, abs_tol=2e-9):
                raise ValueError("frame directions must be unit length")
        if any(
            abs(_dot(left, right)) > 2e-9
            for left, right in ((self.x, self.y), (self.x, self.z), (self.y, self.z))
        ):
            raise ValueError("frame directions must be orthogonal")
        if _dot(_cross(self.x, self.y), self.z) < 1.0 - 2e-9:
            raise ValueError("frame must be right handed")

    def to_local(self, point: Vector3) -> Vector3:
        relative = tuple(point[index] - self.origin[index] for index in range(3))
        return cast(Vector3, tuple(_dot(relative, axis) for axis in (self.x, self.y, self.z)))

    def to_world(self, point: Vector3) -> Vector3:
        return cast(
            Vector3,
            tuple(
                self.origin[index]
                + point[0] * self.x[index]
                + point[1] * self.y[index]
                + point[2] * self.z[index]
                for index in range(3)
            ),
        )


@dataclass(frozen=True, slots=True)
class FramedRecognitionResult:
    """The exact local working shape and records expressed in its accompanying frame."""

    frame: PartFrame
    part: Shape[TopoDS_Shape]
    result: RecognitionResult


@dataclass(frozen=True, slots=True)
class FramedRecognitionReport:
    """The exact local working shape and report expressed in its accompanying frame."""

    frame: PartFrame
    part: Shape[TopoDS_Shape]
    report: RecognitionReport


@dataclass(frozen=True, slots=True)
class PreparedFramedPart:
    """One normalized part and its reusable cylinder substrate, ready for one aggregate run.

    Consumers may derive their caller-owned classification from ``part`` and ``cylinders`` before
    calling :meth:`recognise`.  That method keeps the exact frame, working part and precomputed
    substrate paired; callers do not need a private normalization helper or a second scan.
    """

    frame: PartFrame
    part: Shape[TopoDS_Shape]
    cylinders: FrozenCylinderInventory
    _caller_part: Part | None = field(default=None, repr=False, compare=False)
    _placement: Location | None = field(default=None, repr=False, compare=False)

    def recognise(self, *, rotational: bool = False) -> FramedRecognitionResult:
        """Run the aggregate once using the supplied local-frame classification."""

        # `build_recognition_result` accepts the historical mutable outer lists. Copying the two
        # tiny containers does not repeat cylinder analysis or duplicate the evidence objects.
        cylinders = (list(self.cylinders[0]), list(self.cylinders[1]))
        return FramedRecognitionResult(
            self.frame,
            self.part,
            build_raw_recognition_result(
                self.part,
                cylinders=cylinders,
                rotational=rotational,
            ),
        )

    def recognise_report(self, *, rotational: bool = False) -> FramedRecognitionReport:
        """Run the aggregate once and return its bounded report in this local frame."""

        cylinders = (list(self.cylinders[0]), list(self.cylinders[1]))
        return FramedRecognitionReport(
            self.frame,
            self.part,
            build_raw_recognition_report(
                self.part,
                cylinders=cylinders,
                rotational=rotational,
            ),
        )

    def recognise_evidence(
        self, *, rotational: bool = False
    ) -> FramedRecognitionEvidence[PartFrame] | RefusedFramedEvidence[FramedRecognitionResult]:
        """Run the aggregate once and pair accepted evidence to local and caller faces."""

        return _build_prepared_framed_recognition_evidence(self, rotational=rotational)


FrameInference = PartFrame | RefusedPartFrame
FramedPreparation = PreparedFramedPart | RefusedPartFrame
FramedRecognition = FramedRecognitionResult | RefusedPartFrame
FramedReport = FramedRecognitionReport | RefusedPartFrame
FramedEvidence = (
    FramedRecognitionEvidence[PartFrame]
    | RefusedFramedEvidence[FramedRecognitionResult]
    | RefusedPartFrame
)


@dataclass(slots=True)
class _DirectionClass:
    direction: Vector3
    area: float
    face_areas: list[float]
    face_offsets: list[tuple[float, float]]

    @property
    def signature(self) -> tuple[float, tuple[float, ...], tuple[tuple[float, float], ...]]:
        # Rotation-invariant ordering.  Quantisation settles final-bit OCCT area differences;
        # a remaining exact tie is a geometric gauge, not permission to inspect world XYZ.
        return (
            round(self.area, 9),
            tuple(sorted((round(v, 9) for v in self.face_areas), reverse=True)),
            tuple(
                sorted(
                    ((round(abs(offset), 9), round(area, 9)) for offset, area in self.face_offsets),
                    reverse=True,
                )
            ),
        )

    def oriented(self) -> tuple[Vector3, bool]:
        """Return the geometry-directed representative and whether its sign is observable."""

        forward = tuple(
            sorted(
                ((round(offset, 9), round(area, 9)) for offset, area in self.face_offsets),
                reverse=True,
            )
        )
        reverse = tuple(
            sorted(
                ((round(-offset, 9), round(area, 9)) for offset, area in self.face_offsets),
                reverse=True,
            )
        )
        if forward == reverse:
            return self.direction, False
        sign = 1.0 if forward > reverse else -1.0
        return cast(Vector3, tuple(sign * value for value in self.direction)), True


def _dot(left, right) -> float:
    return sum(float(a) * float(b) for a, b in zip(left, right, strict=True))


def _cross(left: Vector3, right: Vector3) -> Vector3:
    return (
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    )


def _unit(vector) -> Vector3:
    values = tuple(float(value) for value in vector)
    norm = math.hypot(*values)
    if not math.isfinite(norm) or norm <= _COMPONENT_EPS:
        raise ValueError("direction is nonfinite or degenerate")
    return cast(Vector3, tuple(value / norm for value in values))


def _canonical_sign(vector: Vector3) -> Vector3:
    pivot = max(range(3), key=lambda index: (abs(vector[index]), index))
    sign = -1.0 if vector[pivot] < 0.0 else 1.0
    return cast(Vector3, tuple(sign * value for value in vector))


def _clean(vector: Vector3) -> Vector3:
    return cast(
        Vector3,
        tuple(
            0.0
            if abs(value) <= _COMPONENT_EPS
            else math.copysign(1.0, value)
            if abs(abs(value) - 1.0) <= _COMPONENT_EPS
            else value
            for value in vector
        ),
    )


def _material_origin(part: Part) -> Vector3 | RefusedPartFrame:
    if not part.solids():
        return RefusedPartFrame(FrameRefusalReason.NO_MATERIAL)
    props = GProp_GProps()
    BRepGProp.VolumeProperties_s(part.wrapped, props)
    mass = float(props.Mass())
    if not math.isfinite(mass):
        return RefusedPartFrame(FrameRefusalReason.NONFINITE_GEOMETRY)
    if mass <= _COMPONENT_EPS:
        return RefusedPartFrame(FrameRefusalReason.NO_MATERIAL)
    centre = props.CentreOfMass()
    origin = cast(Vector3, tuple(float(value) for value in centre.Coord()))
    if not all(math.isfinite(value) for value in origin):
        return RefusedPartFrame(FrameRefusalReason.NONFINITE_GEOMETRY)
    return origin


def infer_part_frame(part: Part) -> FrameInference:
    """Infer a rigid-equivariant material origin and analytic local direction frame."""

    origin = _material_origin(part)
    if isinstance(origin, RefusedPartFrame):
        return origin
    classes: list[_DirectionClass] = []
    try:
        for face in part.faces():
            surface = BRepAdaptor_Surface(face.wrapped)
            kind = surface.GetType()
            if kind == GeomAbs_Plane:
                raw = tuple(float(value) for value in face.normal_at())
            elif kind == GeomAbs_Cylinder:
                raw = tuple(float(value) for value in surface.Cylinder().Axis().Direction().Coord())
            else:
                continue
            direction = _canonical_sign(_unit(raw))
            props = GProp_GProps()
            BRepGProp.SurfaceProperties_s(face.wrapped, props)
            area = float(props.Mass())
            centre = props.CentreOfMass()
            face_centre = cast(Vector3, tuple(float(value) for value in centre.Coord()))
            if not math.isfinite(area) or not all(math.isfinite(value) for value in face_centre):
                return RefusedPartFrame(FrameRefusalReason.NONFINITE_GEOMETRY)
            offset = _dot(
                tuple(face_centre[index] - origin[index] for index in range(3)), direction
            )
            for direction_class in classes:
                if abs(_dot(direction, direction_class.direction)) >= _PARALLEL_COS:
                    if _dot(direction, direction_class.direction) < 0.0:
                        offset = -offset
                    direction_class.area += area
                    direction_class.face_areas.append(area)
                    direction_class.face_offsets.append((offset, area))
                    break
            else:
                classes.append(_DirectionClass(direction, area, [area], [(offset, area)]))
    except (RuntimeError, ValueError):
        return RefusedPartFrame(FrameRefusalReason.NONFINITE_GEOMETRY)

    ranked = sorted(classes, key=lambda item: item.signature, reverse=True)
    for first_index, first_class in enumerate(ranked):
        for second_class in ranked[first_index + 1 :]:
            first, first_signed = first_class.oriented()
            second, second_signed = second_class.oriented()
            if abs(_dot(first, second)) > _ORTHOGONAL_COS:
                continue
            y = _unit(tuple(second[i] - _dot(first, second) * first[i] for i in range(3)))
            x = _clean(first)
            y = _clean(y)
            z = _clean(_unit(_cross(x, y)))
            # Any equal-ranked direction class leaves a possible axis interchange. Be
            # conservative even when the tied class was not selected for this representative:
            # FULL promises that the complete ordered basis, not merely its first axis, is
            # geometry-established.
            ordering_distinct = len({item.signature for item in ranked}) == len(ranked)
            gauge = (
                FrameGauge.FULL
                if first_signed and second_signed and ordering_distinct
                else FrameGauge.ORTHOGONAL
            )
            return PartFrame(origin, x, y, z, gauge)
    if ranked:
        # One axis leaves roll unconstrained. World XYZ selects a deterministic *representative*
        # of the explicitly published AXIAL gauge; it does not claim a semantic material axis.
        x, _ = ranked[0].oriented()
        seed = min(
            ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)),
            key=lambda candidate: abs(_dot(x, candidate)),
        )
        y = _unit(tuple(seed[i] - _dot(x, seed) * x[i] for i in range(3)))
        x, y = _clean(x), _clean(y)
        z = _clean(_unit(_cross(x, y)))
        return PartFrame(origin, x, y, z, FrameGauge.AXIAL)
    return RefusedPartFrame(FrameRefusalReason.NO_ANALYTIC_DIRECTION)


def _normalization_location(frame: PartFrame) -> Location:
    transform = gp_Trsf()
    axes = (frame.x, frame.y, frame.z)
    values = tuple(component for axis in axes for component in axis)
    offsets = tuple(-_dot(axis, frame.origin) for axis in axes)
    transform.SetValues(
        values[0],
        values[1],
        values[2],
        offsets[0],
        values[3],
        values[4],
        values[5],
        offsets[1],
        values[6],
        values[7],
        values[8],
        offsets[2],
    )
    return Location(gp_trsf=transform)


def _normalize_part(
    part: Part, frame: PartFrame, *, placement: Location | None = None
) -> Shape[TopoDS_Shape]:
    if not part.solids():
        # Material-origin inference already excludes this, so reaching it means the caller
        # changed the shape concurrently between inference and normalization.
        raise ValueError("part has no solids at normalization")
    # A rigid TopLoc placement changes evaluated coordinates without rebuilding topology. A
    # BRepBuilderAPI copied transform perturbs body ancestry and threshold geometry differently
    # across OCCT platforms (most visibly Plate attribution on macOS).
    exact_placement = placement if placement is not None else _normalization_location(frame)
    return exact_placement * part


def _caller_face_bijection(
    caller_part: Part, working_part: Shape[TopoDS_Shape], placement: Location
) -> tuple[tuple[FaceLike, FaceLike], ...] | None:
    caller_faces = tuple(caller_part.faces())
    working_faces = tuple(working_part.faces())
    if len(caller_faces) != len(working_faces):
        return None
    placed_callers = tuple(placement * caller for caller in caller_faces)
    matched: list[int] = []
    for working in working_faces:
        exact = tuple(
            index
            for index, placed in enumerate(placed_callers)
            if working.wrapped.IsSame(placed.wrapped)
        )
        if len(exact) != 1:
            return None
        matched.append(exact[0])
    if len(set(matched)) != len(caller_faces):
        return None
    return tuple(
        (working, caller_faces[index])
        for working, index in zip(working_faces, matched, strict=True)
    )


def _build_prepared_framed_recognition_evidence(
    prepared: PreparedFramedPart,
    *,
    rotational: bool = False,
) -> FramedRecognitionEvidence[PartFrame] | RefusedFramedEvidence[FramedRecognitionResult]:
    caller_part = prepared._caller_part
    placement = prepared._placement
    if caller_part is None or placement is None:
        return RefusedFramedEvidence(FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE)
    bijection = _caller_face_bijection(caller_part, prepared.part, placement)
    if bijection is None:
        return RefusedFramedEvidence(FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE)
    cylinders = (list(prepared.cylinders[0]), list(prepared.cylinders[1]))
    product = _take_inventory(cast(Part, prepared.part), cylinders=cylinders, rotational=rotational)
    evidence = _project_recognition_evidence(product)
    completed = FramedRecognitionResult(prepared.frame, prepared.part, product.result)
    pairs: list[tuple[FaceRef, FaceLike]] = []
    matched: set[int] = set()
    for reference in evidence.faces:
        working = evidence.face(reference)
        exact = tuple(
            (index, caller)
            for index, (mapped_working, caller) in enumerate(bijection)
            if working.wrapped.IsSame(mapped_working.wrapped)
        )
        if len(exact) != 1 or exact[0][0] in matched:
            return RefusedFramedEvidence(
                FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE, completed
            )
        matched.add(exact[0][0])
        pairs.append((reference, exact[0][1]))
    if len(matched) != len(bijection):
        return RefusedFramedEvidence(
            FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE, completed
        )
    return _issue_framed_recognition_evidence(
        prepared.frame,
        prepared.part,
        caller_part,
        evidence,
        tuple(pairs),
    )


def build_framed_recognition_result(part: Part, *, rotational: bool = False) -> FramedRecognition:
    """Recognise once in an inferred local frame, or return a closed frame refusal."""

    prepared = prepare_framed_part(part)
    if isinstance(prepared, RefusedPartFrame):
        return prepared
    return prepared.recognise(rotational=rotational)


def build_framed_recognition_evidence(
    part: Part, *, rotational: bool = False
) -> FramedEvidence:
    """Recognise once in an inferred local frame with exact caller-face evidence.

    The caller must not mutate *part* while using a successful returned view.
    """

    prepared = prepare_framed_part(part)
    if isinstance(prepared, RefusedPartFrame):
        return prepared
    return prepared.recognise_evidence(rotational=rotational)


def build_framed_recognition_report(part: Part, *, rotational: bool = False) -> FramedReport:
    """Recognise once in an inferred local frame with bounded explanations."""

    prepared = prepare_framed_part(part)
    if isinstance(prepared, RefusedPartFrame):
        return prepared
    return prepared.recognise_report(rotational=rotational)


def prepare_framed_part(part: Part) -> FramedPreparation:
    """Normalize *part* and derive the cylinder substrate before caller classification."""

    frame = infer_part_frame(part)
    if isinstance(frame, RefusedPartFrame):
        return frame
    placement = _normalization_location(frame)
    normalized = _normalize_part(part, frame, placement=placement)
    cylinders = analyse_cylinders(normalized)
    return PreparedFramedPart(
        frame,
        normalized,
        (tuple(cylinders[0]), tuple(cylinders[1])),
        _caller_part=part,
        _placement=placement,
    )


__all__ = [
    "FrameGauge",
    "FrameInference",
    "FrameRefusalReason",
    "FramedPreparation",
    "FramedEvidence",
    "FramedEvidenceRefusalReason",
    "FramedRecognitionEvidence",
    "FramedRecognition",
    "FramedRecognitionResult",
    "FramedRecognitionReport",
    "FramedReport",
    "PartFrame",
    "PreparedFramedPart",
    "RefusedPartFrame",
    "RefusedFramedEvidence",
    "build_framed_recognition_result",
    "build_framed_recognition_evidence",
    "build_framed_recognition_report",
    "infer_part_frame",
    "prepare_framed_part",
]
