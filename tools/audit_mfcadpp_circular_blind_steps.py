#!/usr/bin/env python3
"""Audit a geometry-only circular blind-step contract on MFCAD++ class 21.

Dataset labels select and score anatomy; they never participate in the candidate probe.  Instance
counts are explicitly non-native shared-edge same-class component proxies.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import subprocess
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any, cast

from build123d import Vector, extrude

from quiddity import import_step_geometry as import_step

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._adjacency import FaceGraph, FaceNode, axis_aligned_axis  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity._geometry import COORD_FLOOR  # noqa: E402
from quiddity._typing import CylinderEvidence  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

CLASS_ID = 21
_AXES = "xyz"
_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
#: Radians of OCCT cylindrical parameter noise admitted around an exact quarter turn. This is a
#: parameter-space read tolerance, not a geometric feature-size gate and not the dimensionless
#: normal-dot gap used for smoothness. 1e-7 rad displaces a unit-radius boundary by <0.1 micrometre.
_QUARTER_TURN_RAD_TOL = 1e-7
_GATES = (
    "missing_cylinder_terminal_pair",
    "nonprincipal_or_cross_axis_terminal",
    "not_concave",
    "not_quarter_cylinder",
    "external_cylinder",
    "not_blind_to_envelope",
    "incomplete_convex_sector_boundary",
    "material_in_swept_sector",
    "recognisable",
)


@dataclass(frozen=True, slots=True)
class PairProbe:
    """One label-independent cylindrical-wall/terminal geometry probe."""

    first_failed_gate: str
    axis: str | None
    radius: float | None
    length: float | None
    angular_span_radians: float | None
    exact_empty_sweep: bool | None


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _selection_hash(ids: list[str]) -> str:
    return hashlib.sha256(("\n".join(ids) + "\n").encode()).hexdigest()


def _axis_bounds(shape: Any, axis: int) -> tuple[float, float]:
    bounds = shape.bounding_box()
    return (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )[axis]


def _is_concave(graph: FaceGraph, left: FaceNode, right: FaceNode) -> bool:
    return graph.arc(left, right) == "concave"


def _is_convex(graph: FaceGraph, left: FaceNode, right: FaceNode) -> bool:
    return graph.arc(left, right) == "convex"


def _arc_name(graph: FaceGraph, left: FaceNode, right: FaceNode) -> str | None:
    kind = graph.arc(left, right)
    return None if kind is None else str(kind)


def _empty_terminal_sweep(
    graph: FaceGraph,
    cylinder: FaceNode,
    terminal: FaceNode,
    *,
    axis: int,
    direction: int,
    length: float,
) -> bool:
    solid_ref = graph.common_valid_solid((cylinder, terminal))
    if solid_ref is None:
        return False
    vector = [0.0, 0.0, 0.0]
    vector[axis] = float(direction)
    swept = extrude(graph.face(terminal), amount=length, dir=Vector(*vector))
    intersection = cast(Any, swept.intersect(graph.solid_shape(solid_ref)))
    if intersection is None:
        return True
    occupied = (
        intersection.volume
        if hasattr(intersection, "volume")
        else sum(shape.volume for shape in intersection)
    )
    return occupied == 0.0


def probe_pair(
    graph: FaceGraph,
    cylinder: FaceNode,
    terminal: FaceNode,
    evidence: CylinderEvidence | None,
) -> PairProbe:
    """Evaluate the proposed contract without consulting corpus labels."""

    plane = axis_aligned_axis(graph.face(terminal).wrapped)
    if evidence is None or plane is None or _AXES[plane[0]] != evidence["axis"]:
        return PairProbe(_GATES[1], None, None, None, None, None)
    axis = plane[0]
    axis_name = _AXES[axis]
    radius = evidence["diameter"] / 2
    angular_span = float(evidence["u_extent"])
    if not _is_concave(graph, cylinder, terminal):
        return PairProbe(_GATES[2], axis_name, radius, None, angular_span, None)
    if not math.isclose(angular_span, math.pi / 2, rel_tol=0.0, abs_tol=_QUARTER_TURN_RAD_TOL):
        return PairProbe(_GATES[3], axis_name, radius, None, angular_span, None)
    if evidence["external"]:
        return PairProbe(_GATES[4], axis_name, radius, None, angular_span, None)

    solid_ref = graph.common_valid_solid((cylinder, terminal))
    if solid_ref is None:
        return PairProbe(_GATES[5], axis_name, radius, None, angular_span, None)
    low, high = graph.bounds(cylinder)[axis]
    solid_low, solid_high = _axis_bounds(graph.solid_shape(solid_ref), axis)
    terminal_at = plane[1]
    if math.isclose(terminal_at, low, abs_tol=COORD_FLOOR) and math.isclose(
        high, solid_high, abs_tol=COORD_FLOOR
    ):
        direction = 1
    elif math.isclose(terminal_at, high, abs_tol=COORD_FLOOR) and math.isclose(
        low, solid_low, abs_tol=COORD_FLOOR
    ):
        direction = -1
    else:
        return PairProbe(_GATES[5], axis_name, radius, None, angular_span, None)
    length = high - low

    axial: list[FaceNode] = []
    sides: list[FaceNode] = []
    invalid: list[FaceNode] = []
    for neighbour in graph.neighbours(cylinder):
        if neighbour == terminal:
            continue
        neighbour_plane = axis_aligned_axis(graph.face(neighbour).wrapped)
        if not _is_convex(graph, cylinder, neighbour) or neighbour_plane is None:
            invalid.append(neighbour)
        elif neighbour_plane[0] == axis:
            axial.append(neighbour)
        else:
            sides.append(neighbour)
    side_axes = {
        plane[0]
        for node in sides
        if (plane := axis_aligned_axis(graph.face(node).wrapped)) is not None
    }
    if invalid or len(axial) != 1 or len(sides) != 2 or len(side_axes) != 2:
        return PairProbe(_GATES[6], axis_name, radius, length, angular_span, None)

    empty = _empty_terminal_sweep(
        graph,
        cylinder,
        terminal,
        axis=axis,
        direction=direction,
        length=length,
    )
    return PairProbe(
        _GATES[8] if empty else _GATES[7],
        axis_name,
        radius,
        length,
        angular_span,
        empty,
    )


def candidate_pairs(
    graph: FaceGraph, cylinder_evidence: list[CylinderEvidence]
) -> tuple[tuple[FaceNode, FaceNode, PairProbe], ...]:
    """Return every pair satisfying all proposed general geometry gates."""

    evidence_by_node = {graph.require_node(item["face"]): item for item in cylinder_evidence}
    found = []
    for cylinder, evidence in sorted(evidence_by_node.items(), key=lambda item: item[0].index):
        for terminal in graph.neighbours(cylinder):
            if not graph.is_planar(terminal):
                continue
            probe = probe_pair(graph, cylinder, terminal, evidence)
            if probe.first_failed_gate == "recognisable":
                found.append((cylinder, terminal, probe))
    return tuple(found)


def describe_component(
    graph: FaceGraph,
    nodes: tuple[FaceNode, ...],
    cylinder_evidence: list[CylinderEvidence],
) -> dict[str, Any]:
    """Return a traversal-neutral labelled-component anatomy descriptor."""

    ordered = tuple(sorted(nodes, key=lambda node: node.index))
    evidence_by_node = {graph.require_node(item["face"]): item for item in cylinder_evidence}
    probes = [
        probe_pair(graph, cylinder, terminal, evidence_by_node.get(cylinder))
        for cylinder in ordered
        if graph.face(cylinder).geom_type.name == "CYLINDER"
        for terminal in ordered
        if graph.is_planar(terminal)
    ]
    best = max(probes, key=lambda item: _GATES.index(item.first_failed_gate), default=None)
    return {
        "face_count": len(ordered),
        "surface_counts": sorted(
            Counter(graph.face(node).geom_type.name for node in ordered).items()
        ),
        "edge_counts": sorted(len(graph.face(node).edges()) for node in ordered),
        "internal_arc_counts": sorted(
            Counter(
                kind
                for at, left in enumerate(ordered)
                for right in ordered[at + 1 :]
                if (kind := _arc_name(graph, left, right)) is not None
            ).items()
        ),
        "first_failed_gate": (
            best.first_failed_gate if best is not None else "missing_cylinder_terminal_pair"
        ),
    }


def _motif_key(anatomy: dict[str, Any]) -> str:
    return json.dumps(anatomy, sort_keys=True, separators=(",", ":"))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")

    rows: list[dict[str, Any]] = []
    motif_counts: Counter[str] = Counter()
    gate_counts: Counter[str] = Counter()
    label_pairs: Counter[str] = Counter()
    overlap_families: Counter[str] = Counter()
    labelled_faces = component_total = recalled_components = prediction_total = true_predictions = 0
    defining_faces = true_defining_faces = 0
    for path in paths:
        truth = load_mfcadpp_truth(path)
        part = import_step(path)
        if not part.is_valid:
            raise RuntimeError(f"{path.stem}: imported shape is invalid")
        faces = tuple(part.faces())
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{path.stem}: imported face count does not match labels")
        product = _take_inventory(part)
        graph = product.context.graph
        node_indices = {graph.require_node(face): index for index, face in enumerate(faces)}
        cylinders = list(product.context.cylinders[0]) + list(product.context.cylinders[1])
        predictions = candidate_pairs(graph, cylinders)
        prediction_nodes = [
            frozenset((cylinder, terminal)) for cylinder, terminal, _ in predictions
        ]
        prediction_total += len(predictions)
        for defining in prediction_nodes:
            labels = tuple(sorted(truth.semantic[node_indices[node]] for node in defining))
            label_pairs[",".join(str(label) for label in labels)] += 1
            true_predictions += labels == (CLASS_ID, CLASS_ID)
            defining_faces += len(labels)
            true_defining_faces += sum(label == CLASS_ID for label in labels)
            for disposition in product.reconciliation.dispositions:
                if disposition.outcome is not Outcome.ACCEPTED:
                    continue
                if product.evidence.defining_of(disposition.candidate) & defining:
                    overlap_families[disposition.candidate.family.value] += 1

        labelled_indices = {
            index for index, class_id in enumerate(truth.semantic) if class_id == CLASS_ID
        }
        labelled_faces += len(labelled_indices)
        components = _components(
            graph, {graph.require_node(faces[index]) for index in labelled_indices}
        )
        component_total += len(components)
        recalled = sum(
            any(component & defining for defining in prediction_nodes) for component in components
        )
        recalled_components += recalled
        model_rows = []
        for component in components:
            anatomy = describe_component(graph, tuple(component), cylinders)
            key = _motif_key(anatomy)
            motif_counts[key] += 1
            gate_counts[anatomy["first_failed_gate"]] += 1
            model_rows.append(
                {
                    "face_indices": sorted(node_indices[node] for node in component),
                    "anatomy": anatomy,
                }
            )
        if components or predictions:
            rows.append(
                {
                    "model_id": path.stem,
                    "labelled_components": len(components),
                    "recalled_components": recalled,
                    "predictions": len(predictions),
                    "components": model_rows,
                }
            )

    motifs = [
        {"count": count, "anatomy": json.loads(key)}
        for key, count in sorted(motif_counts.items(), key=lambda item: (-item[1], item[0]))
    ]
    report = {
        "format": "b123d-recognisers-mfcadpp-circular-blind-step-audit",
        "format_version": 1,
        "implementation_commit": _commit(),
        "dataset": _PUBLISHED_VERSION,
        "class_id": CLASS_ID,
        "labels_used_for": "selection and scoring only; never candidate geometry",
        "component_derivation": "non-native shared-edge same-class connected components",
        "selection": {
            "rule": f"lexically first {args.limit} STEP files",
            "selected": len(paths),
            "selected_ids_sha256": _selection_hash([path.stem for path in paths]),
        },
        "models_loaded": len(paths),
        "models_invalid": 0,
        "models_evaluated": len(paths),
        "labelled_faces": labelled_faces,
        "derived_components": component_total,
        "recalled_components": recalled_components,
        "derived_component_recall": (
            recalled_components / component_total if component_total else None
        ),
        "predictions": prediction_total,
        "true_predictions": true_predictions,
        "prediction_precision": (true_predictions / prediction_total if prediction_total else None),
        "defining_faces": defining_faces,
        "true_defining_faces": true_defining_faces,
        "defining_face_precision": (
            true_defining_faces / defining_faces if defining_faces else None
        ),
        "prediction_label_pairs": dict(sorted(label_pairs.items())),
        "accepted_family_overlaps": dict(sorted(overlap_families.items())),
        "first_failed_gate_counts": {gate: gate_counts[gate] for gate in _GATES},
        "motifs": motifs,
        "models": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    summary = {key: value for key, value in report.items() if key not in {"models", "motifs"}}
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
