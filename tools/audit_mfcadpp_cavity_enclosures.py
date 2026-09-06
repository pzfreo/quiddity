#!/usr/bin/env python3
"""Audit inner-wire cavity regions against MFCAD++ labels and accepted evidence.

This is label-aware evaluation tooling, not recognition.  Candidate regions are derived only
from same-run topology: an inner wire seeds its adjacent faces and expansion crosses proved
concave or smooth arcs.  Labels are consulted only after region construction.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))

from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.derive_mfcadpp_components import _components  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

TARGET_CLASSES = (2, 3, 4, 13, 14, 15, 16)
TARGET_FAMILIES = (
    FamilyId.PASSAGES,
    FamilyId.POCKETS,
    FamilyId.PRISMATIC_POCKETS,
)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _commit() -> str:
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def _selection_hash(ids: list[str]) -> str:
    return hashlib.sha256(("\n".join(ids) + "\n").encode()).hexdigest()


def _source_selection_hash(sources: list[tuple[str, str]]) -> str:
    value = "".join(f"{model_id}:{source_hash}\n" for model_id, source_hash in sources)
    return hashlib.sha256(value.encode()).hexdigest()


def _production_source() -> dict[str, Any]:
    paths = sorted((ROOT / "src" / "quiddity").rglob("*.py"))
    entries = [(path.relative_to(ROOT).as_posix(), _sha256(path)) for path in paths]
    value = "".join(f"{path}:{digest}\n" for path, digest in entries)
    return {
        "root": "src/quiddity",
        "python_files": len(entries),
        "sha256": hashlib.sha256(value.encode()).hexdigest(),
    }


def _dominant_target_class(labels: Counter[int]) -> tuple[int | None, bool]:
    target = {key: value for key, value in labels.items() if key in TARGET_CLASSES}
    if not target:
        return None, False
    greatest = max(target.values())
    leaders = sorted(key for key, value in target.items() if value == greatest)
    return (leaders[0], False) if len(leaders) == 1 else (None, True)


def _accepted(product: Any) -> tuple[dict[str, Any], ...]:
    found = []
    for family in TARGET_FAMILIES:
        for disposition in product.reconciliation.for_family(family):
            if disposition.outcome is not Outcome.ACCEPTED:
                continue
            candidate = disposition.candidate
            found.append(
                {
                    "family": family.value,
                    "candidate": candidate,
                    "defining": product.evidence.defining_of(candidate),
                    "constituent": product.evidence.constituent_of(candidate),
                }
            )
    return tuple(found)


def _wire_seed(graph: Any, owner: Any, wire: Any) -> frozenset[Any]:
    edges = tuple(wire.edges())
    return frozenset(
        neighbour
        for neighbour in graph.neighbours(owner)
        if any(
            occurrence.edge == edge
            for occurrence in graph.shared_occurrences(owner, neighbour)
            for edge in edges
        )
    )


def _expand(graph: Any, owner: Any, seed: frozenset[Any]) -> frozenset[Any]:
    region = set(seed)
    pending = list(seed)
    while pending:
        current = pending.pop()
        for neighbour in graph.neighbours(current):
            if neighbour is owner or neighbour in region:
                continue
            kind = graph.arc(current, neighbour)
            if not (kind == "concave" or kind == "smooth"):
                continue
            region.add(neighbour)
            pending.append(neighbour)
    return frozenset(region)


def _candidate_regions(graph: Any) -> tuple[tuple[frozenset[Any], frozenset[Any]], ...]:
    """Return deduplicated regions with every inner-wire opening face that seeded them."""

    raw: dict[frozenset[Any], set[Any]] = defaultdict(set)
    for owner in graph.nodes:
        for wire in graph.face(owner).inner_wires():
            seed = _wire_seed(graph, owner, wire)
            if seed:
                raw[_expand(graph, owner, seed)].add(owner)

    def key(item: tuple[frozenset[Any], set[Any]]) -> tuple[int, ...]:
        return tuple(node.index for node in sorted(item[0], key=lambda node: node.index))

    return tuple((region, frozenset(owners)) for region, owners in sorted(raw.items(), key=key))


def _convex_mouth(graph: Any, opening: Any, region: frozenset[Any]) -> bool:
    """Whether one opening contributes a complete convex-only inner-wire boundary.

    The exact wire occurrences, rather than all neighbours of the opening face, define the
    boundary.  Requiring its entire seed to belong to ``region`` prevents an unrelated inner
    loop on the same face from acting as a terminal.
    """

    return any(
        seed
        and seed <= region
        and all(graph.arc(opening, neighbour) == "convex" for neighbour in seed)
        for wire in graph.face(opening).inner_wires()
        if (seed := _wire_seed(graph, opening, wire))
    )


def _two_ended_regions(graph: Any) -> tuple[tuple[frozenset[Any], tuple[Any, Any]], ...]:
    """Return enclosure regions proved by exactly two convex-only inner-wire mouths."""

    found = []
    for region, owners in _candidate_regions(graph):
        mouths = tuple(
            sorted(
                (owner for owner in owners if _convex_mouth(graph, owner, region)),
                key=lambda node: node.index,
            )
        )
        if len(mouths) != 2 or graph.common_valid_solid(region | set(mouths)) is None:
            continue
        found.append((region, (mouths[0], mouths[1])))
    return tuple(found)


def _ratio(numerator: int, denominator: int) -> dict[str, int | float | None]:
    return {
        "numerator": numerator,
        "denominator": denominator,
        "value": numerator / denominator if denominator else None,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"))[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")

    sources = [(path.stem, _sha256(path)) for path in paths]
    source_hashes = dict(sources)
    started = time.perf_counter()
    totals: Counter[str] = Counter()
    confusion: Counter[tuple[int, int]] = Counter()
    class_totals: dict[int, Counter[str]] = {class_id: Counter() for class_id in TARGET_CLASSES}
    family_totals: dict[str, Counter[str]] = {family.value: Counter() for family in TARGET_FAMILIES}
    rows = []
    for path in paths:
        truth = load_mfcadpp_truth(path)
        labelled = {
            index for index, class_id in enumerate(truth.semantic) if class_id in TARGET_CLASSES
        }
        if not labelled:
            continue
        part = import_step(path)
        faces = tuple(part.faces())
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{path.stem}: imported face count does not match labels")
        product = _take_inventory(part)
        graph = product.context.graph
        accepted = _accepted(product)
        for occurrence in accepted:
            family_totals[occurrence["family"]]["accepted_occurrences"] += 1

        class_components: dict[int, tuple[frozenset[Any], ...]] = {}
        for class_id in TARGET_CLASSES:
            nodes = {
                graph.require_node(faces[index])
                for index, label in enumerate(truth.semantic)
                if label == class_id
            }
            class_components[class_id] = _components(graph, nodes) if nodes else ()

        raw_inner_wires = 0
        for owner in graph.nodes:
            for wire in graph.face(owner).inner_wires():
                seed = _wire_seed(graph, owner, wire)
                if not seed:
                    totals["empty_inner_wires"] += 1
                    continue
                raw_inner_wires += 1
        totals["inner_wires"] += raw_inner_wires

        regions = []
        component_region_hits: Counter[tuple[int, int]] = Counter()
        covered_target_nodes: set[Any] = set()
        for ordinal, (region, owners) in enumerate(
            _candidate_regions(graph),
            start=1,
        ):
            solid = graph.common_valid_solid(region | owners)
            solid_nodes = (
                {node for node in graph.nodes if graph.common_valid_solid((node,)) is solid}
                if solid is not None
                else set()
            )
            labels = Counter(truth.semantic[node.index] for node in region)
            target_labels = {key: value for key, value in labels.items() if key in TARGET_CLASSES}
            dominant, target_class_tie = _dominant_target_class(labels)
            target_faces = sum(target_labels.values())
            non_target_faces = len(region) - target_faces
            touched_components = []
            for class_id, components in class_components.items():
                for component_index, component in enumerate(components, start=1):
                    overlap = len(region & component)
                    if overlap:
                        component_region_hits[(class_id, component_index)] += 1
                        touched_components.append(
                            {
                                "class_id": class_id,
                                "component": component_index,
                                "overlap": overlap,
                                "component_faces": len(component),
                            }
                        )
            associations = []
            for accepted_index, occurrence in enumerate(accepted, start=1):
                defining_overlap = len(region & occurrence["defining"])
                constituent_overlap = len(region & occurrence["constituent"])
                if defining_overlap or constituent_overlap:
                    associations.append(
                        {
                            "accepted": accepted_index,
                            "family": occurrence["family"],
                            "defining_overlap": defining_overlap,
                            "defining_faces": len(occurrence["defining"]),
                            "constituent_overlap": constituent_overlap,
                            "constituent_faces": len(occurrence["constituent"]),
                        }
                    )
            unique_occurrence = len({item["accepted"] for item in associations}) == 1
            unique_component = len(touched_components) == 1
            whole_body = bool(solid_nodes) and region >= solid_nodes - set(owners)

            totals["regions"] += 1
            totals["region_faces"] += len(region)
            totals["target_region_faces"] += target_faces
            totals["non_target_region_faces"] += non_target_faces
            totals["regions_touching_target"] += bool(target_faces)
            totals["class_pure_target_regions"] += (
                bool(target_faces) and not non_target_faces and len(target_labels) == 1
            )
            totals["mixed_label_regions"] += len(labels) > 1
            totals["ambiguous_target_class_regions"] += target_class_tie
            totals["whole_body_regions"] += whole_body
            totals["same_solid_regions"] += solid is not None
            totals["unique_component_regions"] += unique_component
            totals["unique_accepted_occurrence_regions"] += unique_occurrence
            totals["unique_component_and_occurrence_regions"] += (
                unique_component and unique_occurrence
            )
            for actual, count in labels.items():
                predicted = -2 if target_class_tie else dominant if dominant is not None else -1
                confusion[(predicted, actual)] += count
            if dominant is not None:
                class_summary = class_totals[dominant]
                class_summary["regions"] += 1
                class_summary["target_faces_in_regions"] += target_faces
                class_summary["non_target_faces_in_regions"] += non_target_faces
                class_summary["unique_component_regions"] += unique_component
                class_summary["unique_accepted_occurrence_regions"] += unique_occurrence
                class_summary["unique_component_and_occurrence_regions"] += (
                    unique_component and unique_occurrence
                )
                class_summary["merged_component_regions"] += len(touched_components) > 1
            covered_target_nodes.update(
                node for node in region if truth.semantic[node.index] in TARGET_CLASSES
            )
            regions.append(
                {
                    "region": ordinal,
                    "face_indices": sorted(node.index for node in region),
                    "opening_face_indices": sorted(node.index for node in owners),
                    "solid_proved": solid is not None,
                    "whole_body": whole_body,
                    "labels": dict(sorted(labels.items())),
                    "dominant_target_class": dominant,
                    "target_class_tie": target_class_tie,
                    "target_faces": target_faces,
                    "non_target_faces": non_target_faces,
                    "components": touched_components,
                    "accepted_associations": associations,
                    "unique_component": unique_component,
                    "unique_accepted_occurrence": unique_occurrence,
                }
            )

        accepted_region_hits: Counter[int] = Counter(
            association["accepted"]
            for region in regions
            for association in region["accepted_associations"]
        )
        for accepted_index, occurrence in enumerate(accepted, start=1):
            family_summary = family_totals[occurrence["family"]]
            hit_count = accepted_region_hits[accepted_index]
            family_summary["touched_occurrences"] += hit_count > 0
            family_summary["touched_once_occurrences"] += hit_count == 1
            family_summary["multiply_touched_occurrences"] += hit_count > 1
        for region in regions:
            occurrence_ids = {item["accepted"] for item in region["accepted_associations"]}
            region["unique_bidirectional_accepted_occurrence"] = (
                len(occurrence_ids) == 1 and accepted_region_hits[next(iter(occurrence_ids))] == 1
            )
            totals["unique_bidirectional_accepted_regions"] += region[
                "unique_bidirectional_accepted_occurrence"
            ]

        component_count = sum(len(components) for components in class_components.values())
        fragmented = sum(hits > 1 for hits in component_region_hits.values())
        merged = sum(
            len({(item["class_id"], item["component"]) for item in region["components"]}) > 1
            for region in regions
        )
        totals["fragmented_components"] += fragmented
        totals["merged_component_regions"] += merged
        totals["target_components"] += component_count
        totals["touched_target_components"] += len(component_region_hits)
        totals["unique_target_faces_reached"] += len(covered_target_nodes)
        totals["accepted_occurrences"] += len(accepted)
        totals["accepted_occurrences_touched"] += len(accepted_region_hits)
        totals["accepted_occurrences_touched_once"] += sum(
            count == 1 for count in accepted_region_hits.values()
        )
        totals["models"] += 1
        totals["target_faces"] += len(labelled)
        for class_id, components in class_components.items():
            class_nodes = {
                graph.require_node(faces[index])
                for index, label in enumerate(truth.semantic)
                if label == class_id
            }
            summary = class_totals[class_id]
            summary["faces"] += len(class_nodes)
            summary["faces_reached"] += len(class_nodes & covered_target_nodes)
            summary["components"] += len(components)
            summary["components_reached"] += sum(
                (class_id, component_index) in component_region_hits
                for component_index in range(1, len(components) + 1)
            )
            summary["fragmented_components"] += sum(
                component_region_hits[(class_id, component_index)] > 1
                for component_index in range(1, len(components) + 1)
            )
        rows.append(
            {
                "model_id": path.stem,
                "source_sha256": source_hashes[path.stem],
                "target_faces": len(labelled),
                "accepted_occurrences": [item["family"] for item in accepted],
                "regions": regions,
            }
        )

    report = {
        "format": "b123d-recognisers-mfcadpp-cavity-enclosure-audit",
        "format_version": 1,
        "implementation_commit": _commit(),
        "production_source": _production_source(),
        "labels_used_in_region_construction": False,
        "candidate_rule": "inner-wire adjacent seeds expanded over concave or smooth arcs",
        "target_classes": list(TARGET_CLASSES),
        "target_families": sorted(family.value for family in TARGET_FAMILIES),
        "selection": {
            "limit": args.limit,
            "selected_ids_sha256": _selection_hash([path.stem for path in paths]),
            "selected_sources_sha256": _source_selection_hash(sources),
        },
        "summary": dict(sorted(totals.items())),
        "rates": {
            "target_face_purity": _ratio(totals["target_region_faces"], totals["region_faces"]),
            "target_region_reach": _ratio(totals["regions_touching_target"], totals["regions"]),
            "target_face_reach": _ratio(
                totals["unique_target_faces_reached"], totals["target_faces"]
            ),
            "target_component_reach": _ratio(
                totals["touched_target_components"], totals["target_components"]
            ),
            "accepted_occurrence_reach": _ratio(
                totals["accepted_occurrences_touched"], totals["accepted_occurrences"]
            ),
            "unique_component_and_occurrence": _ratio(
                totals["unique_component_and_occurrence_regions"], totals["regions"]
            ),
        },
        "confusion": [
            {
                "predicted_target_class": predicted if predicted >= 0 else None,
                "prediction_state": (
                    "class" if predicted >= 0 else "ambiguous" if predicted == -2 else "none"
                ),
                "actual_class": actual,
                "faces": count,
            }
            for (predicted, actual), count in sorted(confusion.items())
        ],
        "class_summary": {
            str(class_id): dict(sorted(summary.items()))
            for class_id, summary in class_totals.items()
        },
        "family_summary": {
            family: dict(sorted(summary.items()))
            for family, summary in sorted(family_totals.items())
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
