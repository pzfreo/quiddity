#!/usr/bin/env python3
"""Measure covariant Plate area authorities without changing production behavior."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import platform
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

from build123d import Axis, Face
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Plane
from OCP.GProp import GProp_GProps

from quiddity import import_step_geometry as import_step

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._geometry import (  # noqa: E402
    AXIS_ALIGNED_COS,
    clears_threshold,
    cluster_coordinates,
)
from quiddity.frames import (  # noqa: E402
    RefusedPartFrame,
    _normalize_part,
    infer_part_frame,
)
from quiddity.plates import Plate, recognise_plates  # noqa: E402

AXES = {"x": 0, "y": 1, "z": 2}
MIN_AREA_FRAC = 0.4
MAX_THICK_FRAC = 0.5
TOL = 0.5


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _mass(shape: Any, *, volume: bool = False) -> float:
    props = GProp_GProps()
    if volume:
        BRepGProp.VolumeProperties_s(shape.wrapped, props)
    else:
        BRepGProp.SurfaceProperties_s(shape.wrapped, props)
    return props.Mass()


def _groups(part: Any, axis_index: int) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    sides: tuple[list[tuple[float, float, float, float, Face]], ...] = ([], [])
    other = [index for index in range(3) if index != axis_index]
    for face in part.faces():
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType() != GeomAbs_Plane:
            continue
        try:
            normal = tuple(face.normal_at())
        except Exception:  # noqa: BLE001 -- matches the production refusal
            continue
        component = normal[axis_index]
        if abs(component) < AXIS_ALIGNED_COS:
            continue
        props = GProp_GProps()
        BRepGProp.SurfaceProperties_s(face.wrapped, props)
        area = props.Mass()
        centre = props.CentreOfMass()
        point = (centre.X(), centre.Y(), centre.Z())
        plane_point = surface.Plane().Location()
        location = (plane_point.X(), plane_point.Y(), plane_point.Z())[axis_index]
        sides[component > 0].append(
            (location, area, point[other[0]] * area, point[other[1]] * area, face)
        )

    grouped: list[list[dict[str, Any]]] = []
    for side in sides:
        values = []
        for cluster in cluster_coordinates([entry[0] for entry in side], tol=TOL):
            values.append(
                {
                    "coordinate": min(side[index][0] for index in cluster),
                    "area": sum(side[index][1] for index in cluster),
                    "u_sum": sum(side[index][2] for index in cluster),
                    "v_sum": sum(side[index][3] for index in cluster),
                }
            )
        grouped.append(values)
    return grouped[0], grouped[1]


def _bbox_authority(part: Any, axis: str, groups: tuple[list[dict[str, Any]], ...]) -> float:
    del groups
    size = tuple(part.bounding_box().size)
    return math.prod(size[index] for index in range(3) if index != AXES[axis])


def _signed_planar_authority(
    part: Any, axis: str, groups: tuple[list[dict[str, Any]], ...]
) -> float:
    del part, axis
    return max(sum(group["area"] for group in side) for side in groups)


def _largest_group_authority(
    part: Any, axis: str, groups: tuple[list[dict[str, Any]], ...]
) -> float:
    del part, axis
    return max((group["area"] for side in groups for group in side), default=0.0)


def _mean_material_authority(
    part: Any, axis: str, groups: tuple[list[dict[str, Any]], ...]
) -> float:
    del groups
    extent = tuple(part.bounding_box().size)[AXES[axis]]
    return _mass(part, volume=True) / extent if extent > 0.0 else 0.0


def _oriented_cross_envelope_authority(
    part: Any, axis: str, groups: tuple[list[dict[str, Any]], ...]
) -> float:
    del groups
    axis_index = AXES[axis]
    other = [index for index in range(3) if index != axis_index]
    angles = set()
    for face in part.faces():
        surface = BRepAdaptor_Surface(face.wrapped)
        if surface.GetType() != GeomAbs_Plane:
            continue
        try:
            normal = tuple(face.normal_at())
        except Exception:  # noqa: BLE001 -- degenerate planes establish no direction
            continue
        if abs(normal[axis_index]) > 1.0 - AXIS_ALIGNED_COS:
            continue
        projected = math.hypot(normal[other[0]], normal[other[1]])
        if projected < AXIS_ALIGNED_COS:
            continue
        angle = math.degrees(math.atan2(normal[other[1]], normal[other[0]]))
        angles.add(round(angle % 90.0, 9))
    if not angles:
        return 0.0
    rotation_axis = (Axis.X, Axis.Y, Axis.Z)[axis_index]
    sign = 1.0 if axis == "y" else -1.0
    areas = []
    for angle in angles:
        size = tuple(part.rotate(rotation_axis, sign * angle).bounding_box().size)
        areas.append(size[other[0]] * size[other[1]])
    return min(areas)


AUTHORITIES: dict[str, Callable[[Any, str, tuple[list[dict[str, Any]], ...]], float]] = {
    "bbox_envelope": _bbox_authority,
    "signed_planar_boundary": _signed_planar_authority,
    "largest_principal_group": _largest_group_authority,
    "mean_material_section": _mean_material_authority,
    "oriented_cross_envelope": _oriented_cross_envelope_authority,
}


def _recognise(part: Any, authority_name: str) -> list[Plate]:
    authority = AUTHORITIES[authority_name]
    records = []
    for solid in list(part.solids()) or [part]:
        size = tuple(solid.bounding_box().size)
        for axis, axis_index in AXES.items():
            groups = _groups(solid, axis_index)
            denominator = authority(solid, axis, groups)
            if not math.isfinite(denominator) or denominator <= 0.0:
                continue
            threshold = MIN_AREA_FRAC * denominator
            maximum_thickness = MAX_THICK_FRAC * size[axis_index]
            events = [
                (group["coordinate"], sign, group)
                for sign, side in zip((-1, 1), groups, strict=True)
                for group in side
                if clears_threshold(group["area"], threshold)
            ]
            events.sort(key=lambda event: (event[0], event[1]))
            seen: set[tuple[str, float, float]] = set()
            for (low, low_sign, low_group), (high, high_sign, high_group) in zip(
                events, events[1:], strict=False
            ):
                thickness = high - low
                if (
                    low_sign != -1
                    or high_sign != 1
                    or thickness <= TOL
                    or thickness >= maximum_thickness
                ):
                    continue
                key = (axis, round(low, 3), round(high, 3))
                if key in seen:
                    continue
                seen.add(key)
                combined = low_group["area"] + high_group["area"]
                records.append(
                    Plate(
                        axis=axis,
                        lo=key[1],
                        hi=key[2],
                        u=(low_group["u_sum"] + high_group["u_sum"]) / combined,
                        v=(low_group["v_sum"] + high_group["v_sum"]) / combined,
                    )
                )
    return sorted(records, key=lambda item: (item.axis, item.lo, item.hi, item.u, item.v))


def _records(records: list[Plate]) -> list[dict[str, Any]]:
    return [record.to_dict() for record in records]


def _evaluate(model_id: str, path: Path) -> dict[str, Any]:
    raw = import_step(path)
    frame = infer_part_frame(raw)
    if isinstance(frame, RefusedPartFrame):
        return {"id": model_id, "status": "frame_refused", "reason": frame.reason.value}
    part = _normalize_part(raw, frame)
    production = recognise_plates(part)
    variants = {name: _recognise(part, name) for name in AUTHORITIES}
    return {
        "id": model_id,
        "status": "evaluated",
        "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "production": _records(production),
        "production_matches": {name: records == production for name, records in variants.items()},
        "authorities": {name: _records(records) for name, records in variants.items()},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("no STEP models found")
    models = [_evaluate(path.stem, path) for path in paths]
    evaluated = [model for model in models if model["status"] == "evaluated"]
    summary = {}
    for name in AUTHORITIES:
        changed = [
            model for model in evaluated if model["authorities"][name] != model["production"]
        ]
        summary[name] = {
            "records": sum(len(model["authorities"][name]) for model in evaluated),
            "changed_models": len(changed),
            "changed_ids": [model["id"] for model in changed],
            "introduced": sum(
                max(0, len(model["authorities"][name]) - len(model["production"]))
                for model in changed
            ),
            "removed": sum(
                max(0, len(model["production"]) - len(model["authorities"][name]))
                for model in changed
            ),
            "production_matches": sum(model["production_matches"][name] for model in evaluated),
        }
    report = {
        "format": "b123d-recognisers-plate-area-authority-measurement",
        "format_version": 1,
        "commit": _commit(),
        "dataset": (
            "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
        ),
        "selection": {
            "rule": "first STEP paths, lexical ascending",
            "selected_ids_sha256": hashlib.sha256(
                ("\n".join(path.stem for path in paths) + "\n").encode()
            ).hexdigest(),
        },
        "limit": args.limit,
        "environment": {"python": platform.python_version(), "platform": platform.platform()},
        "evaluated": len(evaluated),
        "frame_refused": len(models) - len(evaluated),
        "production_records": sum(len(model["production"]) for model in evaluated),
        "summary": summary,
        "models": models,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "models"}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
