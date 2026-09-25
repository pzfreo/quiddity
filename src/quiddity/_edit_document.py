# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Document-local edit relations over accepted evidence and its exact face roster."""

from __future__ import annotations

import math
from collections import defaultdict
from collections.abc import Sequence
from typing import Any

from quiddity._adjacency import FaceGraph
from quiddity._geometry import plane_axes
from quiddity.evidence import FramedRecognitionEvidence


def _body_of(feature: dict[str, Any], face_bodies: Sequence[tuple[int, ...]]) -> int | None:
    owners = {body for face in feature["defining_faces"] for body in face_bodies[face]}
    return next(iter(owners)) if len(owners) == 1 else None


def _dimensions(kind: str, record: dict[str, Any]) -> dict[str, Any]:
    """Named, measured values; conditional names state their geometric interpretation."""

    values: dict[str, Any] = {}
    if kind in {"BossRecord", "PolygonalBoss"}:
        height = record["height"] if kind == "BossRecord" else record["top"] - record["base"]
        values.update(height_above_base=height, axial_length=height)
    elif kind == "RaisedPad":
        axis = record["axis"]
        values["height_above_base"] = record[f"{axis}1"] - record[f"{axis}0"]
    elif kind == "Plate":
        values["thickness"] = record["hi"] - record["lo"]
    elif kind == "GussetRib":
        values["thickness"] = record["thickness_bounds"][1] - record["thickness_bounds"][0]
    elif kind in {"ThinWallBody", "SheetMetalBody"}:
        values["thickness"] = record["thickness"]
    elif kind == "TurnedStep":
        length = record["hi"] - record["lo"]
        values["axial_length"] = length
        if length <= record["diameter"] / 2:
            values["disc_thickness"] = {"value": length, "basis": "heuristic"}
    return values


def _pattern_dimensions(record: dict[str, Any]) -> dict[str, Any]:
    if "pitch" in record:
        return {"center_spacing": record["pitch"], "direction": record.get("direction")}
    if "row_pitch" in record and "col_pitch" in record:
        axis = record.get("holes", [{}])[0].get("axis")
        if axis is None:
            return {}
        u, v = plane_axes(axis)
        angle = math.radians(record["angle"])
        column = [math.cos(angle) * u[i] + math.sin(angle) * v[i] for i in range(3)]
        row = [-math.sin(angle) * u[i] + math.cos(angle) * v[i] for i in range(3)]
        return {
            "row_spacing": record["row_pitch"],
            "column_spacing": record["col_pitch"],
            "row_direction": row,
            "column_direction": column,
        }
    if "diameter" in record and "holes" in record:
        locations = [hole["location"] for hole in record["holes"]]
        spacing = min(
            math.dist(left, right)
            for at, left in enumerate(locations)
            for right in locations[at + 1 :]
        )
        return {
            "adjacent_center_spacing": spacing,
            "bolt_circle_diameter": record["diameter"],
            "direction": "circumferential",
        }
    return {}


def enrich_edit_document(
    view: FramedRecognitionEvidence[Any],
    faces: list[dict[str, Any]],
    features: list[dict[str, Any]],
    derived: dict[str, list[dict[str, Any]]],
    *,
    graph: FaceGraph | None = None,
) -> None:
    """Add bounded edit facts to document copies, without changing public records.

    A relation is reported only between accepted occurrences on one body, using either
    shared source faces, a source edge, or explicit pattern membership. The record's
    ``dependents`` are document-local feature references with the dependent's face ids.
    """

    graph = graph if graph is not None else FaceGraph(view.part)
    adjacent = {
        node.index: {other.index for other in graph.neighbours(node)} for node in graph.nodes
    }
    face_bodies = [tuple(face["body_indices"]) for face in faces]
    bodies = [_body_of(feature, face_bodies) for feature in features]
    records = [view.record(reference) for reference in view.features]
    by_identity = {id(record): index for index, record in enumerate(records)}
    links: list[set[tuple[int, str]]] = [set() for _ in features]

    def touch(left: int, right: int) -> bool:
        source = set(features[left]["constituent_faces"])
        target = set(features[right]["defining_faces"])
        return bool(source & target or any(adjacent[face] & target for face in source))

    def link(owner: int, dependent: int, relation: str) -> None:
        if owner != dependent and bodies[owner] is not None and bodies[owner] == bodies[dependent]:
            links[owner].add((dependent, relation))

    for owner, feature in enumerate(features):
        kind = feature["record_type"]
        for dependent, other in enumerate(features):
            other_kind = other["record_type"]
            if other_kind in {"Chamfer", "Fillet"} and kind not in {"Chamfer", "Fillet"}:
                if touch(owner, dependent):
                    link(owner, dependent, "edge_treatment")
            elif (
                (
                    kind in {"BossRecord", "PolygonalBoss", "Groove", "SectionRecess"}
                    or kind.endswith("Pocket")
                )
                and other_kind == "HoleRecord"
                and touch(owner, dependent)
            ):
                link(owner, dependent, "crossing_bore")

    for pattern_name, member_name in (
        ("hole_patterns", "holes"),
        ("slot_patterns", "slots"),
        ("oriented_slot_patterns", "slots"),
    ):
        for pattern in getattr(view.result, pattern_name):
            members = [
                by_identity[id(member)]
                for member in getattr(pattern, member_name)
                if id(member) in by_identity
            ]
            for owner in members:
                for sibling in members:
                    link(owner, sibling, "pattern_sibling")

    for pattern in view.result.gusset_rib_patterns:
        members = [by_identity[id(member)] for member in pattern.ribs if id(member) in by_identity]
        relation = (
            "mirror_sibling"
            if type(pattern).__name__ == "GussetRibMirrorPair"
            else "pattern_sibling"
        )
        for owner in members:
            for sibling in members:
                link(owner, sibling, relation)

    hole_groups: dict[tuple[Any, ...], list[int]] = defaultdict(list)
    for index, feature in enumerate(features):
        if feature["record_type"] != "HoleRecord" or bodies[index] is None:
            continue
        record = feature["record"]
        opening_plane = round(
            sum(
                value * direction
                for value, direction in zip(record["location"], record["axis"], strict=True)
            ),
            2,
        )
        key = (
            bodies[index],
            tuple(round(value, 4) for value in record["axis"]),
            opening_plane,
            record["diameter"],
            record["depth"],
            record["bottom"],
        )
        hole_groups[key].append(index)
    pairs: list[dict[str, Any]] = []
    for members in hole_groups.values():
        if len(members) != 2:
            continue
        first, second = members
        a = features[first]["record"]["location"]
        b = features[second]["record"]["location"]
        delta = [b[at] - a[at] for at in range(3)]
        spacing = math.dist(a, b)
        axis = features[first]["record"]["axis"]
        if spacing <= 0 or abs(sum(delta[at] * axis[at] for at in range(3))) > 0.01:
            continue
        direction = [value / spacing for value in delta]
        pairs.append(
            {
                "holes": [first, second],
                "center_spacing": spacing,
                "direction": direction,
                "body_index": bodies[first],
                "named_dimensions": {"center_spacing": spacing, "direction": direction},
            }
        )
        link(first, second, "pair_sibling")
        link(second, first, "pair_sibling")
    derived["hole_pairs"] = pairs

    for index, feature in enumerate(features):
        feature["record"] = {
            **feature["record"],
            "named_dimensions": _dimensions(feature["record_type"], feature["record"]),
            "dependents": [
                {
                    "feature_index": dependent,
                    "relation": relation,
                    "defining_faces": features[dependent]["defining_faces"],
                    "constituent_faces": features[dependent]["constituent_faces"],
                }
                for dependent, relation in sorted(links[index])
            ],
        }

    for patterns in derived.values():
        for pattern in patterns:
            if "named_dimensions" not in pattern:
                pattern["named_dimensions"] = _pattern_dimensions(pattern)
