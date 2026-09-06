"""Reproduce the defining-face diagnosis for the two E2 Angled Step frame losses.

This is a focused measurement tool, not a recogniser.  It identifies each raw accepted
AngledStep by its defining face, proves the corresponding normalized face, and reports the
first production discovery gate that rejects that same face in the inferred local frame.
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

from quiddity import import_step_geometry as import_step  # noqa: E402
from quiddity._adjacency import (  # noqa: E402
    edge_face_map,
    nearest_axis_aligned_planes,
)
from quiddity._bevel import (  # noqa: E402
    BevelReject,
    classify_bevel,
    convex_bevel,
    material_beyond_corner,
)
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity.angled_steps import (  # noqa: E402
    _terminal_read,
    recognise_angled_steps,
)
from quiddity.frames import (  # noqa: E402
    PartFrame,
    RefusedPartFrame,
    _normalize_part,
    infer_part_frame,
)
from quiddity.result import _take_inventory  # noqa: E402

MODEL_IDS = ("10492", "10649")
TOLERANCE = 1e-7


def _rounded(values) -> list[float]:
    return [round(float(value), 12) for value in values]


def _dot(left, right) -> float:
    return sum(float(a) * float(b) for a, b in zip(left, right, strict=True))


def _local_direction(frame: PartFrame, direction) -> tuple[float, float, float]:
    return cast(
        tuple[float, float, float],
        tuple(_dot(direction, axis) for axis in (frame.x, frame.y, frame.z)),
    )


def _line_deviation_from_world_axis(direction) -> float:
    component = max(abs(float(value)) for value in direction)
    return math.degrees(math.acos(min(1.0, component)))


def _first_rejection(part, face) -> dict[str, Any]:
    faces = list(part.faces())
    edge_faces = edge_face_map(faces)
    try:
        edge_i, normal, span, _leg_hi, _leg_lo = classify_bevel(face)
    except BevelReject as exc:
        return {"gate": "classify_bevel", "reason": exc.reason}

    other_axes = [axis for axis in (0, 1, 2) if axis != edge_i]
    centre = {axis: 0.5 * sum(span[axis]) for axis in (0, 1, 2)}
    neighbours = nearest_axis_aligned_planes(face, edge_faces, centre, exclude_axis=edge_i)
    if any(axis not in neighbours for axis in other_axes):
        return {
            "gate": "nearest_axis_aligned_planes",
            "reason": "missing-both-bridged-principal-planes",
            "run_axis": "xyz"[edge_i],
            "slant_normal": _rounded(normal),
            "required_axes": ["xyz"[axis] for axis in other_axes],
            "found_axes": ["xyz"[axis] for axis in sorted(neighbours)],
        }
    if not convex_bevel(part, centre, edge_i, neighbours):
        return {"gate": "convex_bevel", "reason": "concave"}
    if material_beyond_corner(part, centre, edge_i, neighbours):
        return {"gate": "material_beyond_corner", "reason": "material-behind-corner"}
    terminals, _near_misses = _terminal_read(face, edge_faces)
    if not terminals:
        return {"gate": "triangular_terminal", "reason": "absent"}
    return {"gate": "accepted", "reason": None}


def _face_correspondence(raw_face, local_face, frame: PartFrame) -> dict[str, Any]:
    raw_centre = tuple(raw_face.center())
    local_centre = tuple(local_face.center())
    expected_centre = frame.to_local(raw_centre)
    raw_normal = tuple(raw_face.normal_at())
    local_normal = tuple(local_face.normal_at())
    expected_normal = _local_direction(frame, raw_normal)
    centre_error = math.dist(local_centre, expected_centre)
    normal_error = math.dist(local_normal, expected_normal)
    preserved = (
        raw_face.geom_type == local_face.geom_type
        and len(raw_face.edges()) == len(local_face.edges())
        and abs(raw_face.area - local_face.area) <= TOLERANCE
        and centre_error <= TOLERANCE
        and normal_error <= TOLERANCE
    )
    if not preserved:
        raise RuntimeError("normalization did not preserve the target face correspondence")
    return {
        "validated": True,
        "raw_center": _rounded(raw_centre),
        "local_center": _rounded(local_centre),
        "center_error": round(centre_error, 15),
        "raw_normal": _rounded(raw_normal),
        "local_normal": _rounded(local_normal),
        "normal_error": round(normal_error, 15),
    }


def audit(dataset_root: Path) -> dict[str, Any]:
    models: list[dict[str, Any]] = []
    for model_id in MODEL_IDS:
        path = dataset_root / f"{model_id}.step"
        raw = import_step(path)
        frame = infer_part_frame(raw)
        if isinstance(frame, RefusedPartFrame):
            raise RuntimeError(f"{path.name}: frame refused: {frame.reason.value}")
        local = _normalize_part(raw, frame)
        raw_product = _take_inventory(raw)
        candidates = raw_product.accepted.candidate_set(FamilyId.ANGLED_STEPS).candidates
        occurrences = []
        for candidate in candidates:
            defining = raw_product.evidence.defining_of(candidate)
            if len(defining) != 1:
                raise RuntimeError(f"{path.name}: AngledStep does not have one defining face")
            face_index = next(iter(defining)).index
            raw_face = raw_product.context.graph.face(next(iter(defining)))
            local_face = local.faces()[face_index]
            occurrences.append(
                {
                    "raw_record": repr(candidate.record),
                    "defining_face_index": face_index,
                    "correspondence": _face_correspondence(raw_face, local_face, frame),
                    "local_discovery": _first_rejection(local, local_face),
                }
            )
        models.append(
            {
                "file": path.name,
                "step_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "frame": {
                    "origin": _rounded(frame.origin),
                    "x": _rounded(frame.x),
                    "y": _rounded(frame.y),
                    "z": _rounded(frame.z),
                    "gauge": frame.gauge.value,
                    "largest_basis_line_deviation_from_world_degrees": round(
                        max(
                            _line_deviation_from_world_axis(axis)
                            for axis in (frame.x, frame.y, frame.z)
                        ),
                        6,
                    ),
                },
                "raw_aggregate_angled_steps": len(candidates),
                "local_direct_angled_steps": len(recognise_angled_steps(local)),
                "local_aggregate_angled_steps": len(
                    _take_inventory(local).accepted.candidate_set(FamilyId.ANGLED_STEPS).candidates
                ),
                "occurrences": sorted(occurrences, key=lambda item: item["defining_face_index"]),
            }
        )
    return {
        "schema": 1,
        "dataset": "MFCAD++ published test split (development evidence)",
        "selection": "exact model IDs 10492 and 10649 from the first-500 mismatch report",
        "face_identity": "zero-based build123d face order, validated across normalization",
        "models": models,
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
