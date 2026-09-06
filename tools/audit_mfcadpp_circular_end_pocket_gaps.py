#!/usr/bin/env python3
"""Classify MFCAD++ circular-end-pocket gaps against unchanged Pocket proofs."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._adjacency import FaceGraph, FaceNode  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity._geometry import length_tol  # noqa: E402
from quiddity._recess_core import _pocket_proposals_one  # noqa: E402
from quiddity._recess_faces import (  # noqa: E402
    _AXES,
    _MERGE_TOL,
    _cylinder_faces,
    _floor_end_faces,
    _has_side_walls,
    _planar_faces,
)
from quiddity._recess_obround import (  # noqa: E402
    _END_RADIUS_FRAC,
    _OBROUND_RATIO_TOL,
    _obround_end,
    _obround_ends,
)
from quiddity._recess_records import Slot  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402
from tools.run_effectiveness_baseline import _KNOWN_MFCADPP_2500_INVALID  # noqa: E402

_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
_KNOWN_INVALID_REASON = "Hole cylindrical evidence does not prove one valid solid"


@dataclass(frozen=True, slots=True)
class GapProbe:
    first_failed_gate: str
    cylinder_faces: int
    planar_faces: int
    principal_side_walls: int
    individually_supported_ends: int
    production_end_groups: int
    end_centerline_delta: float | None = None
    side_walls: bool | None = None
    floor_counts: tuple[int, int] | None = None
    cylinder_end_results: tuple[str, ...] = ()


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _selection_hash(values: list[str]) -> str:
    return hashlib.sha256(("\n".join(values) + "\n").encode()).hexdigest()


def _source_selection_hash(sources: list[tuple[str, str]]) -> str:
    value = "".join(f"{model_id}:{source_hash}\n" for model_id, source_hash in sources)
    return hashlib.sha256(value.encode()).hexdigest()


def _accepted_evidence(
    product: Any,
) -> tuple[tuple[str, frozenset[FaceNode], frozenset[FaceNode]], ...]:
    result = []
    for family in FamilyId:
        if family is FamilyId.LEGACY:
            continue
        for disposition in product.reconciliation.for_family(family):
            if disposition.outcome is Outcome.ACCEPTED:
                candidate = disposition.candidate
                result.append(
                    (
                        family.value,
                        product.evidence.defining_of(candidate),
                        product.evidence.constituent_of(candidate),
                    )
                )
    return tuple(result)


def _overlap(component: frozenset[FaceNode], groups: tuple[frozenset[FaceNode], ...]) -> int:
    return max((len(component & group) for group in groups), default=0)


def _cylinder_end_result(cap: tuple) -> str:
    """Explain one production ``_obround_end`` decision without changing its authority."""

    radius, axis, _location, bounds, concave, _node = cap
    if not concave:
        return "not_concave"
    if radius <= 0:
        return "non_positive_radius"
    other_axes = [candidate for candidate in "xyz" if candidate != axis]
    extents = {
        candidate: (
            getattr(bounds.max, "XYZ"[_AXES[candidate]])
            - getattr(bounds.min, "XYZ"[_AXES[candidate]])
        )
        / radius
        for candidate in other_axes
    }
    across = [
        candidate for candidate in other_axes if abs(extents[candidate] - 2.0) <= _OBROUND_RATIO_TOL
    ]
    bulge = [
        candidate for candidate in other_axes if abs(extents[candidate] - 1.0) <= _OBROUND_RATIO_TOL
    ]
    if len(across) != 1:
        return "not_one_diameter_extent"
    if len(bulge) != 1:
        return "not_one_radius_extent"
    if across[0] == bulge[0]:
        return "ambiguous_in_plane_axes"
    return "accepted"


def _probe_component(
    part: Any,
    graph: FaceGraph,
    component: frozenset[FaceNode],
    proposal_groups: tuple[frozenset[FaceNode], ...],
) -> GapProbe:
    cylinders = [cap for cap in _cylinder_faces(part, graph) if cap[5] in component]
    planes = [face for face in _planar_faces(part, graph=graph) if face.node in component]
    depth_axes = {cap[1] for cap in cylinders}
    depth_axis = next(iter(depth_axes)) if len(depth_axes) == 1 else None
    side_walls = [
        face for face in planes if face.wall and face.axis is not None and face.axis != depth_axis
    ]
    supported = [
        end for cap in cylinders if (end := _obround_end(cap, frozenset({cap[5]}))) is not None
    ]
    production_ends = [end for end in _obround_ends(part, graph) if component & end[9]]
    common: tuple[int, int, int, int, int] = (
        len(cylinders),
        len(planes),
        len(side_walls),
        len(supported),
        len(production_ends),
    )
    end_results = tuple(sorted(_cylinder_end_result(cap) for cap in cylinders))
    if len(cylinders) != 2 or len(planes) != 3:
        return GapProbe("fragmented_anatomy", *common, cylinder_end_results=end_results)
    if len(side_walls) != 2 or len({face.axis for face in side_walls}) != 1:
        return GapProbe("non_principal_side_walls", *common, cylinder_end_results=end_results)
    if len(supported) != 2 or len(production_ends) != 2:
        return GapProbe("not_two_semicircular_ends", *common, cylinder_end_results=end_results)

    low, high = sorted(production_ends, key=lambda end: end[5])
    compatible = (
        low[0:3] == high[0:3]
        and abs(low[3] - high[3]) <= length_tol(low[3], rel=_END_RADIUS_FRAC)
        and abs(low[7] - high[7]) <= _MERGE_TOL
        and abs(low[8] - high[8]) <= _MERGE_TOL
    )
    centerline_delta = abs(low[4] - high[4])
    if not compatible:
        return GapProbe(
            "incompatible_end_pair",
            *common,
            end_centerline_delta=centerline_delta,
            cylinder_end_results=end_results,
        )
    if not (low[6] == -1 and high[6] == 1 and high[5] - low[5] > _MERGE_TOL):
        return GapProbe(
            "ends_do_not_bound_void",
            *common,
            end_centerline_delta=centerline_delta,
            cylinder_end_results=end_results,
        )

    record = Slot(
        width_axis=low[0],
        long_axis=low[1],
        width=round(2 * low[3], 2),
        length=round(high[5] - low[5], 2),
        w_center=round((low[4] + high[4]) / 2, 2),
        lo=round(low[5], 2),
        hi=round(high[5], 2),
        d_lo=round((low[7] + high[7]) / 2, 2),
        d_hi=round((low[8] + high[8]) / 2, 2),
    )
    planar = _planar_faces(part, graph=graph)
    has_sides = _has_side_walls(planar, record)
    if not has_sides:
        return GapProbe(
            "missing_side_walls",
            *common,
            end_centerline_delta=centerline_delta,
            side_walls=False,
            cylinder_end_results=end_results,
        )
    floors = _floor_end_faces(planar, record)
    floor_counts = (len(floors[0]), len(floors[1]))
    if sum(bool(group) for group in floors) != 1:
        return GapProbe(
            "not_single_floor",
            *common,
            end_centerline_delta=centerline_delta,
            side_walls=True,
            floor_counts=floor_counts,
            cylinder_end_results=end_results,
        )
    if _overlap(component, proposal_groups):
        gate = "current_proposal"
    else:
        gate = "centerline_grouping_mismatch"
    return GapProbe(
        gate,
        *common,
        end_centerline_delta=centerline_delta,
        side_walls=True,
        floor_counts=floor_counts,
        cylinder_end_results=end_results,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--class-id", type=int, default=16)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--allow-invalid", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")
    rows: list[dict[str, Any]] = []
    invalid: list[dict[str, str]] = []
    class_model_ids: set[str] = set()
    sources: list[tuple[str, str]] = []
    for path in paths:
        truth = load_mfcadpp_truth(path)
        sources.append((truth.model_id, truth.source_sha256))
        labelled = {
            index for index, class_id in enumerate(truth.semantic) if class_id == args.class_id
        }
        if not labelled:
            continue
        class_model_ids.add(truth.model_id)
        part = import_step(path)
        faces = tuple(part.faces())
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{truth.model_id}: imported face count does not match labels")
        try:
            product = _take_inventory(part)
        except (RuntimeError, ValueError) as error:
            if (
                truth.model_id not in _KNOWN_MFCADPP_2500_INVALID
                or str(error) != _KNOWN_INVALID_REASON
            ):
                raise
            if not args.allow_invalid:
                parser.error(
                    f"{truth.model_id} is a documented invalid model; supply --allow-invalid"
                )
            invalid.append(
                {
                    "model_id": truth.model_id,
                    "source_sha256": truth.source_sha256,
                    "reason": str(error),
                }
            )
            continue
        graph = product.context.graph
        components = _components(graph, {graph.require_node(faces[index]) for index in labelled})
        accepted = _accepted_evidence(product)
        solids = list(part.solids()) or [part]
        proposals = tuple(
            proposal.planar
            | proposal.floors
            | frozenset(node for group in proposal.caps for node in group)
            for solid in solids
            for proposal in _pocket_proposals_one(solid, graph=graph)
        )
        for ordinal, component in enumerate(components, start=1):
            defining = set().union(*(component & claim[1] for claim in accepted))
            constituent = set().union(*(component & claim[2] for claim in accepted))
            families = sorted(
                {
                    family
                    for family, defining_claim, constituent_claim in accepted
                    if component & (defining_claim | constituent_claim)
                }
            )
            rows.append(
                {
                    "model_id": truth.model_id,
                    "component": ordinal,
                    "face_indices": sorted(node.index for node in component),
                    "face_count": len(component),
                    "source_sha256": truth.source_sha256,
                    "accepted_defining_faces": len(defining),
                    "accepted_constituent_faces": len(constituent),
                    "accepted_families": families,
                    "untouched": not constituent,
                    "pocket_proposal_overlap_faces": _overlap(component, proposals),
                    "probe": asdict(_probe_component(part, graph, component, proposals)),
                }
            )

    untouched = [row for row in rows if row["untouched"]]
    selected_ids = {path.stem for path in paths}
    full_known_selection = len(paths) == 2500 and selected_ids >= _KNOWN_MFCADPP_2500_INVALID
    expected_invalid = _KNOWN_MFCADPP_2500_INVALID & class_model_ids
    if full_known_selection and {item["model_id"] for item in invalid} != expected_invalid:
        parser.error("the full-corpus invalid-model set differs from the documented policy")
    gates = Counter(row["probe"]["first_failed_gate"] for row in untouched)
    anatomy_complete = [
        row
        for row in rows
        if row["probe"]["cylinder_faces"] == 2 and row["probe"]["planar_faces"] == 3
    ]
    anatomy_declined = [
        row for row in anatomy_complete if row["probe"]["first_failed_gate"] != "current_proposal"
    ]
    anatomy_declined_gates = Counter(row["probe"]["first_failed_gate"] for row in anatomy_declined)
    anatomy_end_results = Counter(
        result for row in anatomy_declined for result in row["probe"]["cylinder_end_results"]
    )
    report = {
        "format": "b123d-recognisers-mfcadpp-circular-end-pocket-gap-audit",
        "format_version": 2,
        "implementation_commit": _commit(),
        "production_sources": {
            path: _sha256(ROOT / path)
            for path in (
                "src/quiddity/_recess_core.py",
                "src/quiddity/_recess_faces.py",
                "src/quiddity/_recess_obround.py",
            )
        },
        "dataset": {"name": "mfcadpp", "version": _PUBLISHED_VERSION},
        "class_id": args.class_id,
        "derivation": "connected same-label original faces under shared-edge adjacency",
        "native_instance_labels": False,
        "selection": {
            "limit": args.limit,
            "selected_models": len(paths),
            "class_models": len(class_model_ids),
            "evaluated_class_models": len(class_model_ids) - len(invalid),
            "allow_invalid": args.allow_invalid,
            "selected_ids_sha256": _selection_hash([path.stem for path in paths]),
            "selected_sources_sha256": _source_selection_hash(sources),
        },
        "invalid_models": invalid,
        "invalid_policy": {
            "expected_ids": sorted(expected_invalid),
            "expected_reason": _KNOWN_INVALID_REASON,
        },
        "summary": {
            "models_with_class": len({row["model_id"] for row in rows}),
            "components": len(rows),
            "faces": sum(row["face_count"] for row in rows),
            "untouched_components": len(untouched),
            "untouched_faces": sum(row["face_count"] for row in untouched),
            "untouched_failure_gates": dict(sorted(gates.items())),
            "untouched_pocket_proposal_overlaps": sum(
                row["pocket_proposal_overlap_faces"] > 0 for row in untouched
            ),
            "anatomy_complete_components": len(anatomy_complete),
            "anatomy_complete_faces": sum(row["face_count"] for row in anatomy_complete),
            "anatomy_complete_current_proposals": len(anatomy_complete) - len(anatomy_declined),
            "anatomy_complete_declined_components": len(anatomy_declined),
            "anatomy_complete_declined_faces": sum(row["face_count"] for row in anatomy_declined),
            "anatomy_complete_declined_untouched_components": sum(
                row["untouched"] for row in anatomy_declined
            ),
            "anatomy_complete_declined_failure_gates": dict(sorted(anatomy_declined_gates.items())),
            "anatomy_complete_declined_cylinder_end_results": dict(
                sorted(anatomy_end_results.items())
            ),
        },
        "components": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
