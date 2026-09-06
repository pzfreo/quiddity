"""Reproduce the residual E2 Fillet and Plate frame-transition diagnosis.

This is development evidence, not recognition policy. It traces the three signed-principal
Fillet transitions to an equidistant supporting-plane choice and records the internally oblique
Plate case separately so the two mechanisms cannot be conflated.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import sys
from pathlib import Path
from typing import Any, cast

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from OCP.BRepAdaptor import BRepAdaptor_Surface  # noqa: E402

from quiddity import import_step_geometry as import_step  # noqa: E402
from quiddity._adjacency import (  # noqa: E402
    axis_aligned_axis,
    edge_face_map,
    nearest_axis_aligned_planes,
    neighbours,
)
from quiddity._bevel import convex_bevel  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._geometry import AXIS_ALIGNED_COS, length_tol  # noqa: E402
from quiddity.frames import (  # noqa: E402
    PartFrame,
    RefusedPartFrame,
    _normalize_part,
    infer_part_frame,
)
from quiddity.plates import Plate  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

FILLET_TARGETS = {"1129": 11, "1149": 3, "11257": 16}
PLATE_MODEL = "10649"
CORRESPONDENCE_TOLERANCE = 1e-7


def _rounded(values) -> list[float]:
    return [round(float(value), 12) for value in values]


def _dot(left, right) -> float:
    return sum(float(a) * float(b) for a, b in zip(left, right, strict=True))


def _local_direction(frame: PartFrame, direction) -> tuple[float, float, float]:
    return cast(
        tuple[float, float, float],
        tuple(_dot(direction, axis) for axis in (frame.x, frame.y, frame.z)),
    )


def _line_deviation_from_principal(direction) -> float:
    component = max(abs(float(value)) for value in direction)
    return math.degrees(math.acos(min(1.0, component)))


def _frame(frame: PartFrame) -> dict[str, Any]:
    deviations = [_line_deviation_from_principal(axis) for axis in (frame.x, frame.y, frame.z)]
    return {
        "origin": _rounded(frame.origin),
        "x": _rounded(frame.x),
        "y": _rounded(frame.y),
        "z": _rounded(frame.z),
        "gauge": frame.gauge.value,
        "largest_basis_line_deviation_from_world_degrees": round(max(deviations), 9),
        "signed_principal_permutation": max(deviations) <= 1e-7,
    }


def _validate_face(raw_face, local_face, frame: PartFrame) -> dict[str, Any]:
    raw_center = tuple(raw_face.center())
    local_center = tuple(local_face.center())
    center_error = math.dist(local_center, frame.to_local(raw_center))
    raw_normal = tuple(raw_face.normal_at())
    local_normal = tuple(local_face.normal_at())
    normal_error = math.dist(local_normal, _local_direction(frame, raw_normal))
    if not (
        raw_face.geom_type == local_face.geom_type
        and len(raw_face.edges()) == len(local_face.edges())
        and abs(raw_face.area - local_face.area) <= CORRESPONDENCE_TOLERANCE
        and center_error <= CORRESPONDENCE_TOLERANCE
        and normal_error <= CORRESPONDENCE_TOLERANCE
    ):
        raise RuntimeError("normalization did not preserve target face identity")
    return {
        "validated": True,
        "raw_normal": _rounded(raw_normal),
        "local_normal": _rounded(local_normal),
        "center_error": round(center_error, 15),
        "normal_error": round(normal_error, 15),
    }


def _accepted_face_indices(product, family: FamilyId) -> set[int]:
    return {
        node.index
        for candidate in product.accepted.candidate_set(family).candidates
        for node in product.evidence.defining_of(candidate)
    }


def _fillet_plane_read(part, face) -> dict[str, Any]:
    surface = BRepAdaptor_Surface(face.wrapped)
    direction = surface.Cylinder().Axis().Direction()
    components = (abs(direction.X()), abs(direction.Y()), abs(direction.Z()))
    edge_axis = max(range(3), key=lambda axis: components[axis])
    bounds = face.bounding_box()
    spans = (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )
    centre = {axis: 0.5 * sum(spans[axis]) for axis in range(3)}
    face_edges = edge_face_map(part.faces())
    coordinates: dict[int, list[float]] = {}
    for other in neighbours(face, face_edges):
        aligned = axis_aligned_axis(other.wrapped)
        if aligned is not None and aligned[0] != edge_axis:
            coordinates.setdefault(aligned[0], []).append(aligned[1])
    axes = []
    for axis in sorted(coordinates):
        values = coordinates[axis]
        distances = [abs(value - centre[axis]) for value in values]
        nearest = min(distances)
        tied = sorted(
            value
            for value, distance in zip(values, distances, strict=True)
            if abs(distance - nearest) <= length_tol(max(distance, nearest), rel=1e-9)
        )
        plane_tol = length_tol(max(abs(value - centre[axis]) for value in tied), rel=1e-9)
        axes.append(
            {
                "axis": "xyz"[axis],
                "nearest_distance": round(nearest, 12),
                "tied_coordinates": _rounded(tied),
                "distinct_equidistant": max(tied) - min(tied) > plane_tol,
            }
        )
    legacy = nearest_axis_aligned_planes(face, face_edges, centre, exclude_axis=edge_axis)
    unambiguous = nearest_axis_aligned_planes(
        face,
        face_edges,
        centre,
        exclude_axis=edge_axis,
        refuse_equidistant=True,
    )
    required = {axis for axis in range(3) if axis != edge_axis}
    return {
        "run_axis": "xyz"[edge_axis],
        "axes": axes,
        "legacy_lower_coordinate_choice": {
            "coordinates": {"xyz"[axis]: round(value, 12) for axis, value in legacy.items()},
            "convex": required <= legacy.keys() and convex_bevel(part, centre, edge_axis, legacy),
        },
        "unambiguous_choice": {
            "coordinates": {"xyz"[axis]: round(value, 12) for axis, value in unambiguous.items()},
            "has_both_required_planes": required <= unambiguous.keys(),
        },
    }


def _fillet_models(dataset_root: Path) -> list[dict[str, Any]]:
    models = []
    for model_id, face_index in FILLET_TARGETS.items():
        path = dataset_root / f"{model_id}.step"
        truth = load_mfcadpp_truth(path)
        raw = import_step(path)
        frame = infer_part_frame(raw)
        if isinstance(frame, RefusedPartFrame):
            raise RuntimeError(f"{path.name}: frame refused: {frame.reason.value}")
        local = _normalize_part(raw, frame)
        raw_product = _take_inventory(raw)
        local_product = _take_inventory(local)
        raw_face = raw.faces()[face_index]
        local_face = local.faces()[face_index]
        models.append(
            {
                "file": path.name,
                "step_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "frame": _frame(frame),
                "target_face_index": face_index,
                "mfcadpp_class": truth.semantic[face_index],
                "correspondence": _validate_face(raw_face, local_face, frame),
                "raw": {
                    "accepted_target_fillet": face_index
                    in _accepted_face_indices(raw_product, FamilyId.FILLETS),
                    "plane_read": _fillet_plane_read(raw, raw_face),
                },
                "local": {
                    "accepted_target_fillet": face_index
                    in _accepted_face_indices(local_product, FamilyId.FILLETS),
                    "plane_read": _fillet_plane_read(local, local_face),
                },
            }
        )
    return models


def _plate_model(dataset_root: Path) -> dict[str, Any]:
    path = dataset_root / f"{PLATE_MODEL}.step"
    truth = load_mfcadpp_truth(path)
    raw = import_step(path)
    frame = infer_part_frame(raw)
    if isinstance(frame, RefusedPartFrame):
        raise RuntimeError(f"{path.name}: frame refused: {frame.reason.value}")
    local = _normalize_part(raw, frame)
    raw_product = _take_inventory(raw)
    local_product = _take_inventory(local)
    occurrences = []
    for candidate in raw_product.accepted.candidate_set(FamilyId.PLATES).candidates:
        record = cast(Plate, candidate.record)
        indices = sorted(node.index for node in raw_product.evidence.defining_of(candidate))
        local_faces = [local.faces()[index] for index in indices]
        raw_axis = "xyz".index(record.axis)
        world_axis = tuple(1.0 if index == raw_axis else 0.0 for index in range(3))
        local_direction = _local_direction(frame, world_axis)
        local_axis = max(range(3), key=lambda index: abs(local_direction[index]))

        def area_gate(part, faces, axis: int) -> dict[str, Any]:
            bounds = part.bounding_box()
            extents = (bounds.size.X, bounds.size.Y, bounds.size.Z)
            cross_area = math.prod(extents[index] for index in range(3) if index != axis)
            threshold = 0.4 * cross_area
            signed_areas = {-1: 0.0, 1: 0.0}
            for face in faces:
                component = tuple(face.normal_at())[axis]
                signed_areas[1 if component > 0.0 else -1] += face.area
            return {
                "axis": "xyz"[axis],
                "bbox_cross_area": round(cross_area, 12),
                "minimum_group_area": round(threshold, 12),
                "negative_group_area": round(signed_areas[-1], 12),
                "positive_group_area": round(signed_areas[1], 12),
                "both_groups_clear": all(area >= threshold for area in signed_areas.values()),
            }

        occurrences.append(
            {
                "raw_record": repr(record),
                "defining_face_indices": indices,
                "mfcadpp_classes": [truth.semantic[index] for index in indices],
                "correspondence": [
                    _validate_face(raw.faces()[index], local.faces()[index], frame)
                    for index in indices
                ],
                "local_absolute_max_normal_components": [
                    round(max(abs(float(value)) for value in face.normal_at()), 12)
                    for face in local_faces
                ],
                "plate_axis_alignment_threshold": AXIS_ALIGNED_COS,
                "all_defining_faces_locally_principal": all(
                    max(abs(float(value)) for value in face.normal_at()) >= AXIS_ALIGNED_COS
                    for face in local_faces
                ),
                "raw_area_gate": area_gate(
                    raw, [raw.faces()[index] for index in indices], raw_axis
                ),
                "local_area_gate": area_gate(local, local_faces, local_axis),
            }
        )
    return {
        "file": path.name,
        "step_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "frame": _frame(frame),
        "raw_aggregate_plates": len(raw_product.accepted.candidate_set(FamilyId.PLATES).candidates),
        "local_aggregate_plates": len(
            local_product.accepted.candidate_set(FamilyId.PLATES).candidates
        ),
        "first_boundary": "bbox-cross-area-threshold-changes-under-in-plane-roll",
        "occurrences": occurrences,
    }


def audit(dataset_root: Path) -> dict[str, Any]:
    models = _fillet_models(dataset_root)
    if any(model[side]["accepted_target_fillet"] for model in models for side in ("raw", "local")):
        raise RuntimeError("a target false Fillet remains accepted")
    plate = _plate_model(dataset_root)
    if plate["raw_aggregate_plates"] != 1 or plate["local_aggregate_plates"] != 0:
        raise RuntimeError("unexpected Plate transition inventory")
    return {
        "schema": 1,
        "dataset": "MFCAD++ published test split (development evidence)",
        "selection": "the three Fillet and one Plate transitions in issue #323",
        "face_identity": "zero-based build123d face order, validated across normalization",
        "fillet_disposition": "false positives removed by refusing distinct equidistant planes",
        "fillet_models": models,
        "plate_disposition": (
            "true covariance defect in the bbox-based area denominator; no Plate semantics "
            "changed by this Fillet increment"
        ),
        "plate_model": plate,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dataset_root", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    rendered = json.dumps(audit(args.dataset_root), indent=2, sort_keys=True)
    if args.output is None:
        print(rendered)
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
