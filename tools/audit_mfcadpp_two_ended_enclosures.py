#!/usr/bin/env python3
"""Measure the label-blind two-ended enclosure hypothesis on MFCAD++.

Geometry constructs every candidate before semantic labels are consulted.  MFCAD++ labels then
measure passage reach and negative-class contamination; they never select mouths or traversal.
Because MFCAD++ has no instance IDs, connected same-label components are explicitly reported as
component proxies rather than instances.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from tools.audit_mfcadpp_cavity_enclosures import _two_ended_regions  # noqa: E402
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

PASSAGE_CLASSES = (2, 3, 4)
POCKET_CLASSES = (13, 14, 15, 16)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _ratio(numerator: int, denominator: int) -> dict[str, int | float | None]:
    return {
        "numerator": numerator,
        "denominator": denominator,
        "value": numerator / denominator if denominator else None,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=2500)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")

    started = time.perf_counter()
    totals: Counter[str] = Counter()
    class_totals: dict[int, Counter[str]] = {
        class_id: Counter() for class_id in (*PASSAGE_CLASSES, *POCKET_CLASSES)
    }
    rows: list[dict[str, Any]] = []
    sources = []
    for path in paths:
        truth = load_mfcadpp_truth(path)
        sources.append((truth.model_id, truth.source_sha256))
        part = import_step(path)
        faces = tuple(part.faces())
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{path.stem}: imported face count does not match labels")

        from quiddity._adjacency import FaceGraph

        graph = FaceGraph(part)
        regions = _two_ended_regions(graph)
        reached: set[Any] = set()
        model_rows = []
        for ordinal, (region, mouths) in enumerate(regions, start=1):
            labels = Counter(truth.semantic[node.index] for node in region)
            passage_faces = sum(labels[class_id] for class_id in PASSAGE_CLASSES)
            pocket_faces = sum(labels[class_id] for class_id in POCKET_CLASSES)
            totals["candidates"] += 1
            totals["candidate_faces"] += len(region)
            totals["passage_faces_in_candidates"] += passage_faces
            totals["pocket_faces_in_candidates"] += pocket_faces
            totals["other_faces_in_candidates"] += len(region) - passage_faces - pocket_faces
            totals["passage_pure_candidates"] += passage_faces == len(region)
            reached.update(region)
            model_rows.append(
                {
                    "candidate": ordinal,
                    "face_indices": sorted(node.index for node in region),
                    "mouth_face_indices": [mouth.index for mouth in mouths],
                    "labels": dict(sorted(labels.items())),
                }
            )

        for class_id, summary in class_totals.items():
            nodes = {
                graph.require_node(faces[index])
                for index, label in enumerate(truth.semantic)
                if label == class_id
            }
            components = _components(graph, nodes) if nodes else ()
            summary["faces"] += len(nodes)
            summary["faces_reached"] += len(nodes & reached)
            summary["components"] += len(components)
            summary["components_reached"] += sum(
                bool(component & reached) for component in components
            )
            summary["components_fully_reached"] += sum(
                component <= reached for component in components
            )
        totals["models"] += 1
        totals["models_with_candidates"] += bool(regions)
        if model_rows:
            rows.append({"model_id": truth.model_id, "candidates": model_rows})

    passage_faces = sum(class_totals[class_id]["faces"] for class_id in PASSAGE_CLASSES)
    passage_reached = sum(class_totals[class_id]["faces_reached"] for class_id in PASSAGE_CLASSES)
    report = {
        "format": "b123d-recognisers-mfcadpp-two-ended-enclosure-audit",
        "format_version": 1,
        "implementation_commit": subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
        ).stdout.strip(),
        "labels_used_in_candidate_construction": False,
        "candidate_rule": (
            "one concave-or-smooth enclosure region independently seeded by exactly two "
            "complete convex-only inner-wire mouths on one proved solid"
        ),
        "selection": {
            "limit": args.limit,
            "selected_ids_sha256": hashlib.sha256(
                ("\n".join(path.stem for path in paths) + "\n").encode()
            ).hexdigest(),
            "selected_sources_sha256": hashlib.sha256(
                "".join(f"{model_id}:{digest}\n" for model_id, digest in sources).encode()
            ).hexdigest(),
        },
        "summary": dict(sorted(totals.items())),
        "rates": {
            "passage_face_reach": _ratio(passage_reached, passage_faces),
            "candidate_passage_purity": _ratio(
                totals["passage_faces_in_candidates"], totals["candidate_faces"]
            ),
            "passage_pure_candidate_rate": _ratio(
                totals["passage_pure_candidates"], totals["candidates"]
            ),
        },
        "class_summary": {
            str(class_id): dict(sorted(summary.items()))
            for class_id, summary in class_totals.items()
        },
        "runtime_seconds": time.perf_counter() - started,
        "models": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "models"}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
