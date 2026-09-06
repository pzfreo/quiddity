#!/usr/bin/env python3
"""Audit neutral blend-chain reach against MFCAD++ Round labels.

Labels are evaluation-only. Blend discovery uses the production graph and effective-surface
authority exactly as issued by one aggregate recognition run.
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

from quiddity._blend_view import (  # noqa: E402
    BlendChain,
    BlendCollapseIndex,
    RefusedBlendComponent,
)
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._dispositions import Outcome  # noqa: E402
from quiddity._run import start  # noqa: E402
from quiddity.result import _take_inventory  # noqa: E402
from tools.effectiveness_report import load_mfcadpp_truth  # noqa: E402

ROUND_CLASS = 23


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


def _selection_hash(values: list[str]) -> str:
    return hashlib.sha256(("\n".join(values) + "\n").encode()).hexdigest()


def _production_source() -> dict[str, Any]:
    paths = sorted((ROOT / "src" / "quiddity").rglob("*.py"))
    entries = [(path.relative_to(ROOT).as_posix(), _sha256(path)) for path in paths]
    value = "".join(f"{path}:{digest}\n" for path, digest in entries)
    return {
        "root": "src/quiddity",
        "python_files": len(entries),
        "sha256": hashlib.sha256(value.encode()).hexdigest(),
    }


def _accepted(product: Any) -> tuple[dict[str, Any], ...]:
    found = []
    for family in FamilyId:
        if family is FamilyId.LEGACY:
            continue
        for disposition in product.reconciliation.for_family(family):
            if disposition.outcome is not Outcome.ACCEPTED:
                continue
            candidate = disposition.candidate
            found.append(
                {
                    "family": family.value,
                    "defining": product.evidence.defining_of(candidate),
                    "constituent": product.evidence.constituent_of(candidate),
                }
            )
    return tuple(found)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--limit", type=int, default=500)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    from OCP.BRepAdaptor import BRepAdaptor_Surface

    from quiddity import import_step_geometry as import_step

    paths = sorted(args.root.glob("*.st*p"), key=lambda path: path.name)[: args.limit]
    if not paths:
        parser.error("the selected workload contains no STEP files")

    sources = [(path.stem, _sha256(path)) for path in paths]
    source_hash = hashlib.sha256(
        "".join(f"{model_id}:{digest}\n" for model_id, digest in sources).encode()
    ).hexdigest()
    totals: Counter[str] = Counter()
    sides: Counter[str] = Counter()
    refusal_reasons: Counter[str] = Counter()
    accepted_families: Counter[str] = Counter()
    chain_label_profiles: Counter[str] = Counter()
    chain_side_label_profiles: Counter[str] = Counter()
    outside_surface_kinds: Counter[str] = Counter()
    rows = []
    source_hashes = dict(sources)
    started = time.perf_counter()
    for path in paths:
        part = import_step(path)
        faces = tuple(part.faces())
        context = start(part)
        graph = context.graph
        results = BlendCollapseIndex(graph, context.surfaces).results()
        chains = tuple(result for result in results if isinstance(result, BlendChain))
        refusals = tuple(result for result in results if isinstance(result, RefusedBlendComponent))

        # Labels enter only after the complete selected model's neutral discovery has finished.
        truth = load_mfcadpp_truth(path)
        if len(faces) != len(truth.semantic):
            raise RuntimeError(f"{path.stem}: imported face count does not match labels")
        labelled_indices = {
            index for index, class_id in enumerate(truth.semantic) if class_id == ROUND_CLASS
        }
        labelled = {graph.require_node(faces[index]) for index in labelled_indices}
        chain_nodes = {node for chain in chains for node in chain.blend_nodes}
        convex_nodes = {
            node for chain in chains if chain.side == "convex" for node in chain.blend_nodes
        }
        concave_nodes = {
            node for chain in chains if chain.side == "concave" for node in chain.blend_nodes
        }
        refused_nodes = {node for refusal in refusals for node in refusal.nodes}

        accepted: tuple[dict[str, Any], ...] = ()
        accepted_constituent_indices: set[int] = set()
        accepted_defining_indices: set[int] = set()
        fillet_constituent_indices: set[int] = set()
        if labelled_indices:
            product = _take_inventory(part)
            accepted = _accepted(product)
            accepted_constituent_indices = {
                node.index for occurrence in accepted for node in occurrence["constituent"]
            }
            accepted_defining_indices = {
                node.index for occurrence in accepted for node in occurrence["defining"]
            }
            fillet_constituent_indices = {
                node.index
                for occurrence in accepted
                if occurrence["family"] == "fillets"
                for node in occurrence["constituent"]
            }

        for chain in chains:
            sides[chain.side] += 1
            labels = Counter(truth.semantic[node.index] for node in chain.blend_nodes)
            target = labels[ROUND_CLASS]
            if target == len(chain.blend_nodes):
                profile = "pure_round"
            elif target:
                profile = "mixed_round"
            else:
                profile = "no_round"
            chain_label_profiles[profile] += 1
            chain_side_label_profiles[f"{chain.side}:{profile}"] += 1
        for refusal in refusals:
            refusal_reasons[refusal.reason.value] += len(refusal.nodes & labelled)
        for family in sorted({occurrence["family"] for occurrence in accepted}):
            family_nodes = {
                node.index
                for occurrence in accepted
                if occurrence["family"] == family
                for node in occurrence["constituent"]
            }
            accepted_families[family] += len(family_nodes & labelled_indices)

        touched_indices = labelled_indices & accepted_constituent_indices
        touched = {node for node in labelled if node.index in touched_indices}
        untouched = {node for node in labelled if node.index not in touched_indices}
        outside = untouched - chain_nodes - refused_nodes
        for node in outside:
            kind = BRepAdaptor_Surface(graph.face(node).wrapped).GetType().name
            outside_surface_kinds[kind] += 1
        row = {
            "model_id": path.stem,
            "source_sha256": source_hashes[path.stem],
            "round_faces": len(labelled),
            "accepted_defining_round_faces": len(labelled_indices & accepted_defining_indices),
            "accepted_covered_round_faces": len(touched),
            "fillet_covered_round_faces": len(labelled_indices & fillet_constituent_indices),
            "chain_faces": len(chain_nodes),
            "chain_round_faces": len(labelled & chain_nodes),
            "convex_chain_faces": len(convex_nodes),
            "convex_chain_round_faces": len(labelled & convex_nodes),
            "concave_chain_faces": len(concave_nodes),
            "concave_chain_round_faces": len(labelled & concave_nodes),
            "refused_faces": len(refused_nodes),
            "refused_round_faces": len(labelled & refused_nodes),
            "untouched_round_faces": len(untouched),
            "untouched_reached_by_chain": len(untouched & chain_nodes),
            "untouched_reached_by_convex_chain": len(untouched & convex_nodes),
            "untouched_reached_by_concave_chain": len(untouched & concave_nodes),
            "untouched_in_refusal": len(untouched & refused_nodes),
            "untouched_outside_index": len(outside),
            "chains": len(chains),
            "refusals": len(refusals),
        }
        rows.append(row)
        totals["models"] += 1
        totals.update({key: value for key, value in row.items() if isinstance(value, int)})

    report = {
        "format": "b123d-recognisers-mfcadpp-blend-chain-audit",
        "format_version": 1,
        "implementation_commit": _commit(),
        "production_source": _production_source(),
        "labels_used_in_discovery": False,
        "target_class": ROUND_CLASS,
        "selection": {
            "limit": args.limit,
            "selected_ids_sha256": _selection_hash([path.stem for path in paths]),
            "selected_sources_sha256": source_hash,
        },
        "summary": dict(sorted(totals.items())),
        "chain_sides": dict(sorted(sides.items())),
        "chain_label_profiles": dict(sorted(chain_label_profiles.items())),
        "chain_side_label_profiles": dict(sorted(chain_side_label_profiles.items())),
        "refused_round_faces_by_reason": dict(sorted(refusal_reasons.items())),
        "accepted_round_constituents_by_family": dict(sorted(accepted_families.items())),
        "untouched_outside_index_by_kernel_surface": dict(sorted(outside_surface_kinds.items())),
        "runtime_seconds": time.perf_counter() - started,
        "models": rows,
    }
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({key: report[key] for key in report if key != "models"}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
