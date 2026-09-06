"""Check accepted recess evidence survives the ADR-0019 public projection.

This is a migration check, not an effectiveness scorer. Exact-region correspondence does not
prove geometric equivalence; reconstruction tests supply that evidence independently. Pocket's
extent-only records are reported separately and never counted as exact geometry projections.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from quiddity import import_step_geometry
from quiddity._candidates import FamilyId
from quiddity.result import InventoryProduct, _take_inventory

EXACT_FAMILIES = (
    FamilyId.PRISMATIC_POCKETS,
    FamilyId.PASSAGES,
    FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
    FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
    FamilyId.RECTANGULAR_BLIND_SLOTS,
    FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
)


def audit_product(product: InventoryProduct) -> dict:
    """Account for every accepted legacy recess without rerunning discovery."""

    rows = []
    accepted = product.accepted
    for family in (*EXACT_FAMILIES, FamilyId.POCKETS, FamilyId.CHANNELS):
        for ordinal, candidate in enumerate(accepted.candidate_set(family).candidates):
            defining = product.evidence.defining_of(candidate.record)
            constituent = product.evidence.constituent_of(candidate.record)
            owner = product.context.graph.common_valid_solid(defining)
            defining_indices = {node.index for node in defining}
            constituent_indices = {node.index for node in constituent}
            matches = [
                record
                for record in product.result.section_recesses
                if owner is not None
                and record.body == owner.ordinal
                and defining_indices
                and defining_indices <= set(record.evidence.constituent_faces)
                and constituent_indices <= set(record.evidence.constituent_faces)
                and (
                    family in {FamilyId.POCKETS, FamilyId.CHANNELS}
                    or constituent_indices == set(record.evidence.constituent_faces)
                )
            ]
            refused = any(
                owner is not None
                and record.body == owner.ordinal
                and defining_indices == set(record.evidence.defining_faces)
                and constituent_indices == set(record.evidence.constituent_faces)
                for record in product.result.section_recess_refusals
            )
            rows.append(
                {
                    "family": family.value,
                    "candidate": ordinal,
                    "status": (
                        ("explicit_refusal" if refused else "unrepresented")
                        if not matches
                        else "evidence_only"
                        if family in {FamilyId.POCKETS, FamilyId.CHANNELS}
                        else "exact_region"
                    ),
                    "section_recess_indices": [record.index for record in matches],
                }
            )
    return {
        "section_recesses": len(product.result.section_recesses),
        "counts": {
            status: sum(row["status"] == status for row in rows)
            for status in ("exact_region", "evidence_only", "explicit_refusal", "unrepresented")
        },
        "candidates": rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--step",
        type=Path,
        action="append",
        default=[],
        help="development STEP file; repeat for several inputs",
    )
    args = parser.parse_args()
    models = {}
    if args.step:
        for path in args.step:
            models[str(path)] = audit_product(_take_inventory(import_step_geometry(path)))
    else:
        from tests.golden._common import load_fixture

        root = Path(__file__).resolve().parents[1] / "tests" / "golden"
        for path in sorted(root.glob("*/fixture.py")):
            models[path.parent.name] = audit_product(
                _take_inventory(load_fixture(path).build_fixture())
            )
    print(json.dumps({"format_version": 1, "models": models}, indent=2, sort_keys=True))
    return int(
        any(
            row["status"] == "unrepresented"
            for model in models.values()
            for row in model["candidates"]
        )
    )


if __name__ == "__main__":
    raise SystemExit(main())
