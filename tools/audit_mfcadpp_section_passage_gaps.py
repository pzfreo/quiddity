#!/usr/bin/env python3
"""Classify MFCAD++ passage components against the production section-ring proofs.

Dataset labels select components to describe. They never participate in proposal discovery or
alter a geometry predicate. MFCAD++ has no native instance IDs, so the report calls connected
same-label faces component proxies rather than instances.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, cast

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._adjacency import FaceGraph, FaceNode  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity._geometry import dot  # noqa: E402
from quiddity._section_passages import (  # noqa: E402
    _DIRECTION_TOL,
    _INTERVAL_TOL,
    _canonical_run,
    _face_interval,
    _ordered_cycle,
    _pair_line,
    _parallel,
    _void_and_open,
    section_ring_proposals,
)
from quiddity._sections import (  # noqa: E402
    LocalFrame,
    PlanarSection,
    SectionVertex,
)
from quiddity.result import _take_inventory  # noqa: E402
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402
from tools.run_effectiveness_baseline import _KNOWN_MFCADPP_2500_INVALID  # noqa: E402

_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
_KNOWN_INVALID_REASON = "Hole cylindrical evidence does not prove one valid solid"
_GATES = (
    "no_linear_run",
    "nonplanar_or_nonwall",
    "no_collinear_junctions",
    "pair_interval_mismatch",
    "not_simple_cycle",
    "cross_solid_or_invalid_solid",
    "unequal_complete_spans",
    "invalid_section",
    "material_or_capped",
    "recognisable",
)

Vector3 = tuple[float, float, float]


@dataclass(frozen=True, slots=True)
class ComponentProbe:
    stage: int
    first_failed_gate: str
    run: Vector3 | None
    planar_walls: int
    collinear_pairs: int
    interval_pairs: int
    cycle_faces: int


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _selection_hash(ids: list[str]) -> str:
    return hashlib.sha256(("\n".join(ids) + "\n").encode()).hexdigest()


def _source_selection_hash(sources: list[tuple[str, str]]) -> str:
    value = "".join(f"{model_id}:{source_hash}\n" for model_id, source_hash in sources)
    return hashlib.sha256(value.encode()).hexdigest()


def _invalid_row(
    model_id: str,
    source_sha256: str,
    error: RuntimeError | ValueError,
    *,
    allow_invalid: bool,
) -> dict[str, str]:
    """Record only the exact invalid inputs covered by the published full-corpus policy."""

    if (
        not allow_invalid
        or model_id not in _KNOWN_MFCADPP_2500_INVALID
        or str(error) != _KNOWN_INVALID_REASON
    ):
        raise error
    return {
        "model_id": model_id,
        "source_sha256": source_sha256,
        "reason": str(error),
    }


def _failed(
    stage: int,
    *,
    run: Vector3 | None = None,
    planar_walls: int = 0,
    collinear_pairs: int = 0,
    interval_pairs: int = 0,
    cycle_faces: int = 0,
) -> ComponentProbe:
    return ComponentProbe(
        stage,
        _GATES[stage],
        run,
        planar_walls,
        collinear_pairs,
        interval_pairs,
        cycle_faces,
    )


def _run_candidates(graph: FaceGraph, members: tuple[FaceNode, ...]) -> tuple[Vector3, ...]:
    found: list[Vector3] = []
    for at, left in enumerate(members):
        for right in members[at + 1 :]:
            for edge in graph.shared_edges(left, right):
                run = _canonical_run(edge)
                if run is not None and not any(_parallel(run, known) for known in found):
                    found.append(run)
    return tuple(sorted(found))


def _probe_run(
    graph: FaceGraph,
    members: tuple[FaceNode, ...],
    run: Vector3,
) -> ComponentProbe:
    base = LocalFrame.canonical(run, (0.0, 0.0, 0.0))
    walls = tuple(
        node
        for node in members
        if graph.is_planar(node)
        and (normal := graph.normal(node)) is not None
        and abs(dot(normal, base.run)) <= _DIRECTION_TOL
    )
    if len(walls) != len(members):
        return _failed(1, run=base.run, planar_walls=len(walls))

    pair_lines: dict[frozenset[FaceNode], tuple[float, float, float, float]] = {}
    interval_lines: dict[frozenset[FaceNode], tuple[float, float, float, float]] = {}
    adjacency: dict[FaceNode, set[FaceNode]] = {node: set() for node in walls}
    for at, left in enumerate(walls):
        for right in walls[at + 1 :]:
            line = _pair_line(graph, left, right, base)
            if line is None:
                continue
            pair = frozenset((left, right))
            pair_lines[pair] = line
            left_span = _face_interval(graph, left, base.run)
            right_span = _face_interval(graph, right, base.run)
            if left_span is None or right_span is None:
                continue
            expected = (line[2], line[3], line[2], line[3])
            if any(
                abs(actual - wanted) > _INTERVAL_TOL
                for actual, wanted in zip((*left_span, *right_span), expected, strict=True)
            ):
                continue
            interval_lines[pair] = line
            adjacency[left].add(right)
            adjacency[right].add(left)
    if not pair_lines:
        return _failed(2, run=base.run, planar_walls=len(walls))
    if not interval_lines:
        return _failed(
            3,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
        )
    if len(walls) < 3 or any(len(adjacency[node]) != 2 for node in walls):
        return _failed(
            4,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
            interval_pairs=len(interval_lines),
        )
    solid = graph.common_valid_solid(walls)
    if solid is None:
        return _failed(
            5,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
            interval_pairs=len(interval_lines),
            cycle_faces=len(walls),
        )
    try:
        order = _ordered_cycle(walls, adjacency, interval_lines)
    except (KeyError, ValueError):
        return _failed(
            4,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
            interval_pairs=len(interval_lines),
        )
    spans = tuple(_face_interval(graph, node, base.run) for node in order)
    if any(span is None for span in spans):
        return _failed(
            6,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
            interval_pairs=len(interval_lines),
            cycle_faces=len(walls),
        )
    complete_spans = cast(tuple[tuple[float, float], ...], spans)
    low, high = complete_spans[0]
    if any(
        abs(span[0] - low) > _INTERVAL_TOL or abs(span[1] - high) > _INTERVAL_TOL
        for span in complete_spans
    ):
        return _failed(
            6,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
            interval_pairs=len(interval_lines),
            cycle_faces=len(walls),
        )
    lines = tuple(
        interval_lines[frozenset((node, order[(at + 1) % len(order)]))]
        for at, node in enumerate(order)
    )
    try:
        raw = PlanarSection(tuple(SectionVertex((line[0], line[1])) for line in lines))
        centre = raw.centroid
        frame = LocalFrame.canonical(
            base.run,
            tuple(centre[0] * base.u[index] + centre[1] * base.v[index] for index in range(3)),
        )
        section = PlanarSection(
            tuple(
                SectionVertex(
                    (vertex.point[0] - centre[0], vertex.point[1] - centre[1]),
                    vertex.bulge,
                )
                for vertex in raw.boundary
            )
        )
    except ValueError:
        return _failed(
            7,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
            interval_pairs=len(interval_lines),
            cycle_faces=len(walls),
        )
    if not _void_and_open(graph.solid_shape(solid), frame, (low, high), section):
        return _failed(
            8,
            run=base.run,
            planar_walls=len(walls),
            collinear_pairs=len(pair_lines),
            interval_pairs=len(interval_lines),
            cycle_faces=len(walls),
        )
    return _failed(
        9,
        run=base.run,
        planar_walls=len(walls),
        collinear_pairs=len(pair_lines),
        interval_pairs=len(interval_lines),
        cycle_faces=len(walls),
    )


def _probe_component(graph: FaceGraph, component: frozenset[FaceNode]) -> ComponentProbe:
    members = tuple(sorted(component, key=lambda node: node.index))
    runs = _run_candidates(graph, members)
    if not runs:
        return _failed(0)
    return max((_probe_run(graph, members, run) for run in runs), key=lambda probe: probe.stage)


def _accepted_claims(
    product: Any,
) -> tuple[tuple[str, frozenset[FaceNode], frozenset[FaceNode]], ...]:
    claims = []
    for family in FamilyId:
        if family is FamilyId.LEGACY:
            continue
        for disposition in product.reconciliation.for_family(family):
            if disposition.outcome is Outcome.ACCEPTED:
                candidate = disposition.candidate
                claims.append(
                    (
                        family.value,
                        product.evidence.defining_of(candidate),
                        product.evidence.constituent_of(candidate),
                    )
                )
    return tuple(claims)


def _relation(
    component: frozenset[FaceNode],
    claims: tuple[tuple[str, frozenset[FaceNode], frozenset[FaceNode]], ...],
    role: int,
) -> dict[str, Any]:
    touched = [claim for claim in claims if component & claim[role]]
    covered = set().union(*(component & claim[role] for claim in touched)) if touched else set()
    return {
        "covered_faces": len(covered),
        "full": component <= covered,
        "touching_families": sorted({claim[0] for claim in touched}),
    }


def _component_row(
    graph: FaceGraph,
    component: frozenset[FaceNode],
    claims: tuple[tuple[str, frozenset[FaceNode], frozenset[FaceNode]], ...],
    proposals: tuple[frozenset[FaceNode], ...],
    *,
    model_id: str,
    ordinal: int,
) -> dict[str, Any]:
    passage_claims = tuple(claim for claim in claims if claim[0] == FamilyId.PASSAGES.value)
    surface_counts = Counter(str(graph.surface(node)) for node in component)
    proposal_overlap = [
        overlap
        for proposal in proposals
        if (overlap := sorted(node.index for node in component & proposal))
    ]
    return {
        "model_id": model_id,
        "component": ordinal,
        "face_indices": sorted(node.index for node in component),
        "face_count": len(component),
        "surface_counts": dict(sorted(surface_counts.items())),
        "accepted": {
            "defining": _relation(component, claims, 1),
            "constituent": _relation(component, claims, 2),
        },
        "passages": {
            "defining": _relation(component, passage_claims, 1),
            "constituent": _relation(component, passage_claims, 2),
        },
        "production_proposal_overlaps": sorted(proposal_overlap),
        "probe": asdict(_probe_component(graph, component)),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--class-id", type=int, required=True)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--allow-invalid", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")
    selected_ids = [path.stem for path in paths]
    full_known_selection = (
        len(selected_ids) == 2500 and set(selected_ids) >= _KNOWN_MFCADPP_2500_INVALID
    )
    if full_known_selection and not args.allow_invalid:
        parser.error(
            "the known MFCAD++-2,500 selection contains seven invalid models; "
            "supply the documented --allow-invalid policy before recognition"
        )
    rows: list[dict[str, Any]] = []
    sources: list[tuple[str, str]] = []
    invalid: list[dict[str, str]] = []
    expected_invalid: set[str] = set()
    for path in paths:
        truth = load_mfcadpp_truth(path)
        sources.append((truth.model_id, truth.source_sha256))
        indices = {
            index for index, class_id in enumerate(truth.semantic) if class_id == args.class_id
        }
        if not indices:
            continue
        if truth.model_id in _KNOWN_MFCADPP_2500_INVALID:
            expected_invalid.add(truth.model_id)
        part = import_step(path)
        faces = tuple(part.faces())
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{path.stem}: imported face count does not match labels")
        try:
            product = _take_inventory(part)
        except (RuntimeError, ValueError) as error:
            invalid.append(
                _invalid_row(
                    truth.model_id,
                    truth.source_sha256,
                    error,
                    allow_invalid=args.allow_invalid,
                )
            )
            continue
        graph = product.context.graph
        nodes = {graph.require_node(faces[index]) for index in indices}
        claims = _accepted_claims(product)
        proposals = tuple(
            frozenset(proposal.nodes) for proposal in section_ring_proposals(part, graph)
        )
        rows.extend(
            _component_row(
                graph,
                component,
                claims,
                proposals,
                model_id=truth.model_id,
                ordinal=ordinal,
            )
            for ordinal, component in enumerate(_components(graph, nodes), start=1)
        )
    if full_known_selection and {item["model_id"] for item in invalid} != expected_invalid:
        parser.error("the full-corpus invalid-model set differs from the documented policy")
    gate_counts = Counter(row["probe"]["first_failed_gate"] for row in rows)
    untouched = [row for row in rows if row["accepted"]["constituent"]["covered_faces"] == 0]
    report = {
        "format": "b123d-recognisers-mfcadpp-section-passage-gap-audit",
        "format_version": 3,
        "implementation_commit": _commit(),
        "production_source": {
            "path": "src/quiddity/_section_passages.py",
            "sha256": _sha256(ROOT / "src/quiddity/_section_passages.py"),
        },
        "dataset": {"name": "mfcadpp", "version": _PUBLISHED_VERSION},
        "class_id": args.class_id,
        "derivation": "connected same-label original faces under shared-edge adjacency",
        "probe_semantics": (
            "Counterfactual application of the unchanged production proofs to every face in one "
            "label-component proxy. Exact production discovery is reported separately as proposal "
            "overlap; a probe failure is diagnostic anatomy, not proof that labels define a "
            "feature."
        ),
        "native_instance_labels": False,
        "selection": {
            "limit": args.limit,
            "allow_invalid": args.allow_invalid,
            "selected_ids_sha256": _selection_hash(selected_ids),
            "selected_sources_sha256": _source_selection_hash(sources),
        },
        "invalid_models": invalid,
        "invalid_policy": {
            "expected_ids_with_target_class": sorted(expected_invalid),
            "expected_reason": _KNOWN_INVALID_REASON,
        },
        "summary": {
            "models_with_class": len({row["model_id"] for row in rows}),
            "components": len(rows),
            "faces": sum(row["face_count"] for row in rows),
            "untouched_components": len(untouched),
            "untouched_faces": sum(row["face_count"] for row in untouched),
            "gate_counts": dict(sorted(gate_counts.items())),
            "untouched_gate_counts": dict(
                sorted(Counter(row["probe"]["first_failed_gate"] for row in untouched).items())
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
