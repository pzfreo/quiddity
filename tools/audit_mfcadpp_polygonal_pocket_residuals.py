#!/usr/bin/env python3
"""Measure current polygonal Pocket detection and membership residuals on MFCAD++.

Every model's aggregate Candidate inventory is constructed before its labels are read. Labels then
select connected component proxies and measure already-issued evidence; they never participate in
proposal discovery or a geometry predicate. MFCAD++ has no native feature-instance identity.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import multiprocessing
import os
import subprocess
import sys
from collections import Counter
from collections.abc import Iterable
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._recess_core import _pocket_proposals_one  # noqa: E402
from quiddity._rings import rings  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.audit_mfcadpp_prismatic_pocket_gaps import (  # noqa: E402
    _accepted_evidence,
    _overlap,
    _probe_component,
)
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402
from tools.run_effectiveness_baseline import _KNOWN_MFCADPP_2500_INVALID  # noqa: E402

_PUBLISHED_VERSION = (
    "MFCAD++ published test split; DOI 10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"
)
_KNOWN_INVALID_REASON = "Hole cylindrical evidence does not prove one valid solid"
_TARGET_CLASSES = (13, 14, 15)


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _selection_hash(values: list[str]) -> str:
    return hashlib.sha256(("\n".join(values) + "\n").encode()).hexdigest()


def _status(covered: int, total: int) -> str:
    if covered == 0:
        return "untouched"
    if covered < total:
        return "partial"
    return "complete"


def _audit_model(
    path: Path,
) -> tuple[str, list[dict[str, Any]], dict[str, str] | None]:
    """Evaluate one model; only serializable evidence crosses the worker boundary."""

    from quiddity import import_step_geometry

    part = import_step_geometry(path)
    try:
        product = _take_inventory(part)
    except (RuntimeError, ValueError) as error:
        if path.stem not in _KNOWN_MFCADPP_2500_INVALID or str(error) != _KNOWN_INVALID_REASON:
            raise
        truth = load_mfcadpp_truth(path)
        return (
            f"{truth.model_id}:{truth.source_sha256}",
            [],
            {
                "model_id": truth.model_id,
                "source_sha256": truth.source_sha256,
                "reason": str(error),
            },
        )

    # Deliberately after Candidate construction: labels measure, but cannot author, geometry.
    truth = load_mfcadpp_truth(path)
    faces = tuple(part.faces())
    if len(faces) != len(truth.semantic):
        raise RuntimeError(f"{truth.model_id}: imported face count does not match labels")
    graph = product.context.graph
    accepted = _accepted_evidence(product)
    ring_proposals = tuple(
        frozenset((*ring.nodes, *ring.cap_nodes[0], *ring.cap_nodes[1]))
        for ring in rings(part, graph)
    )
    solids = list(part.solids()) or [part]
    pocket_proposals = tuple(
        proposal.planar
        | proposal.floors
        | frozenset(node for group in proposal.caps for node in group)
        for solid in solids
        for proposal in _pocket_proposals_one(solid, graph=graph)
    )

    rows: list[dict[str, Any]] = []
    for class_id in _TARGET_CLASSES:
        labelled = {
            graph.require_node(faces[index])
            for index, label in enumerate(truth.semantic)
            if label == class_id
        }
        for ordinal, component in enumerate(_components(graph, labelled) if labelled else ()):
            defining = set().union(*(component & claim[1] for claim in accepted))
            constituent = set().union(*(component & claim[2] for claim in accepted))
            probe = _probe_component(part, graph, component)
            surfaces = Counter(str(graph.surface(node)).rsplit(".", 1)[-1] for node in component)
            internal_degrees = Counter(
                len(set(graph.neighbours(node)) & set(component)) for node in component
            )
            rows.append(
                {
                    "model_id": truth.model_id,
                    "source_sha256": truth.source_sha256,
                    "class_id": class_id,
                    "ordinal": ordinal,
                    "faces": len(component),
                    "face_indices": sorted(node.index for node in component),
                    "defining_faces": len(defining),
                    "constituent_faces": len(constituent),
                    "missing_constituent_faces": len(component - constituent),
                    "coverage_status": _status(len(constituent), len(component)),
                    "accepted_families": sorted(
                        {
                            family
                            for family, defining_claim, constituent_claim in accepted
                            if component & (defining_claim | constituent_claim)
                        }
                    ),
                    "surfaces": dict(sorted(surfaces.items())),
                    "internal_degrees": {
                        str(degree): count for degree, count in sorted(internal_degrees.items())
                    },
                    "one_valid_solid": graph.common_valid_solid(component) is not None,
                    "ring_overlap_faces": _overlap(component, ring_proposals),
                    "pocket_overlap_faces": _overlap(component, pocket_proposals),
                    "ring_probe": {
                        "first_failed_gate": probe.first_failed_gate,
                        "stage": probe.stage,
                        "axis": probe.axis,
                        "eligible_walls": probe.eligible_walls,
                        "span_members": probe.span_members,
                        "cap_counts": probe.cap_counts,
                    },
                }
            )
    return f"{truth.model_id}:{truth.source_sha256}", rows, None


def _summarise(rows: list[dict[str, Any]]) -> dict[str, Any]:
    summary: dict[str, Any] = {}
    for class_id in _TARGET_CLASSES:
        selected = [row for row in rows if row["class_id"] == class_id]
        status = Counter(row["coverage_status"] for row in selected)
        gates = Counter(
            row["ring_probe"]["first_failed_gate"]
            for row in selected
            if row["coverage_status"] != "complete"
        )
        faces = sum(row["faces"] for row in selected)
        covered = sum(row["constituent_faces"] for row in selected)
        summary[str(class_id)] = {
            "components": len(selected),
            "faces": faces,
            "constituent_faces": covered,
            "missing_constituent_faces": faces - covered,
            "coverage": covered / faces if faces else None,
            "untouched_components": status["untouched"],
            "partial_components": status["partial"],
            "complete_components": status["complete"],
            "incomplete_first_failed_gates": dict(sorted(gates.items())),
        }
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--workers", type=int, default=min(4, os.cpu_count() or 1))
    parser.add_argument("--allow-invalid", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")
    if args.workers < 1:
        parser.error("--workers must be positive")
    selected_ids = [path.stem for path in paths]
    full_known_selection = (
        len(selected_ids) == 2500 and set(selected_ids) >= _KNOWN_MFCADPP_2500_INVALID
    )
    if full_known_selection and not args.allow_invalid:
        parser.error(
            "the known MFCAD++-2,500 selection contains seven invalid models; "
            "supply the documented --allow-invalid policy before recognition"
        )

    work: Iterable[tuple[str, list[dict[str, Any]], dict[str, str] | None]]
    executor: concurrent.futures.ProcessPoolExecutor | None = None
    if args.workers == 1:
        work = map(_audit_model, paths)
    else:
        executor = concurrent.futures.ProcessPoolExecutor(
            max_workers=args.workers, mp_context=multiprocessing.get_context("spawn")
        )
        work = executor.map(_audit_model, paths)
    sources: list[str] = []
    rows: list[dict[str, Any]] = []
    invalid: list[dict[str, str]] = []
    try:
        for source, model_rows, model_invalid in work:
            sources.append(source)
            rows.extend(model_rows)
            if model_invalid is not None:
                invalid.append(model_invalid)
    finally:
        if executor is not None:
            executor.shutdown()

    if invalid and not args.allow_invalid:
        parser.error("audit encountered invalid models without --allow-invalid")
    if (
        full_known_selection
        and {item["model_id"] for item in invalid} != _KNOWN_MFCADPP_2500_INVALID
    ):
        parser.error("the full-corpus invalid-model set differs from the documented policy")
    report = {
        "format": "b123d-recognisers-mfcadpp-polygonal-pocket-residual-audit",
        "format_version": 1,
        "implementation_commit": _commit(),
        "dataset_version": _PUBLISHED_VERSION,
        "target_classes": list(_TARGET_CLASSES),
        "component_derivation": "same-label original faces connected by shared-edge adjacency",
        "native_instance_labels": False,
        "labels_read_after_candidate_construction": True,
        "selection": {
            "limit": args.limit,
            "selected_models": len(paths),
            "evaluated_models": len(paths) - len(invalid),
            "allow_invalid": args.allow_invalid,
            "workers": args.workers,
            "selected_ids_sha256": _selection_hash(selected_ids),
            "selected_sources_sha256": _selection_hash(sources),
        },
        "invalid_models": invalid,
        "invalid_policy": {
            "expected_ids": sorted(_KNOWN_MFCADPP_2500_INVALID),
            "expected_reason": _KNOWN_INVALID_REASON,
        },
        "class_summary": _summarise(rows),
        "rows": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report["class_summary"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
