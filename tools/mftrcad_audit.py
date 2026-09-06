#!/usr/bin/env python3
"""Audit an external MFTRCAD v1 checkout without making its labels policy.

MFTRCAD is too large to vendor.  Point this tool at a directory containing the upstream
``steps/`` and ``labels/`` directories.  It validates STEP/annotation identity, reports the
dataset's semantic and instance populations, expands its feature-relationship groups, and
optionally joins accepted package Candidates back to the annotated faces they define.

The JSON output is deterministic: paths and mappings are sorted, floating-point recognition
records are never serialized, and development/holdout membership follows the checked-in
SHA-256 rule rather than observed recognition outcomes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from dataclasses import dataclass
from itertools import combinations
from pathlib import Path, PurePath
from typing import Any, Final, Literal, TypeAlias, cast

DATASET_REF: Final = "xmy2000/mftrcad"
DATASET_VERSION: Final = 1
DATASET_ID: Final = 4_762_595
DATASET_CREATED_AT: Final = "2024-04-09T08:52:51.09Z"
DATASET_LICENCE: Final = "MIT"
GENERATOR_COMMIT: Final = "3ae17a43dde22e4f27c5d7df179838d92551c28c"
SELECTION_NAMESPACE: Final = "b123d-recognisers:mftrcad:v1"
SELECTION_MODULUS: Final = 1000
DEVELOPMENT_BUCKETS: Final = frozenset(range(0, 500))
HOLDOUT_BUCKETS: Final = frozenset(range(500, 1000))
F5_FLATS_H1: Final = "F5-FLATS-H1"
F5_FILLETS_H1: Final = "F5-FILLETS-H1"
F5_COUNTERSINKS_H1: Final = "F5-COUNTERSINKS-H1"
F5_BOSSES_H1: Final = "F5-BOSSES-H1"
F5_DOUBLE_D_BORES_H1: Final = "F5-DOUBLE-D-BORES-H1"
F5_POLYGONAL_BOSSES_H1: Final = "F5-POLYGONAL-BOSSES-H1"
F5_PADS_H1: Final = "F5-PADS-H1"
F5_HOLES_H1: Final = "F5-HOLES-H1"
F5_CHANNELS_H1: Final = "F5-CHANNELS-H1"
F5_PLATES_H1: Final = "F5-PLATES-H1"
F5_POLYGONAL_STOCK_H1: Final = "F5-POLYGONAL-STOCK-H1"
F5_SLOTS_H1: Final = "F5-SLOTS-H1"
F5_POCKETS_H1: Final = "F5-POCKETS-H1"
F5_REPEATING_RADIAL_PROFILES_H1: Final = "F5-REPEATING-RADIAL-PROFILES-H1"
F5_STEP_LEVELS_H1: Final = "F5-STEP-LEVELS-H1"
F5_RISERS_H1: Final = "F5-RISERS-H1"
F4B_SECTION_PASSAGES_H1: Final = "F4B-SECTION-PASSAGES-H1"
F3B_POLYGONAL_BOSSES_H1: Final = "F3B-POLYGONAL-BOSSES-H1"
SELECTION_POLICY_PATH: Final = (
    Path(__file__).parents[1] / "docs" / "corpora" / "mftrcad-selection.json"
)


@dataclass(frozen=True, slots=True)
class AllocationSpec:
    policy_id: str
    selection_token: str
    buckets: frozenset[int]
    status: str


ALLOCATION_SPECS: Final = (
    AllocationSpec(F5_FLATS_H1, "f5_flats_h1", frozenset({20}), "consumed"),
    AllocationSpec(F5_FILLETS_H1, "f5_fillets_h1", frozenset({21}), "consumed"),
    AllocationSpec(
        F5_COUNTERSINKS_H1,
        "f5_countersinks_h1",
        frozenset({22}),
        "consumed",
    ),
    AllocationSpec(F5_BOSSES_H1, "f5_bosses_h1", frozenset({23}), "consumed"),
    AllocationSpec(
        F5_DOUBLE_D_BORES_H1,
        "f5_double_d_bores_h1",
        frozenset({24}),
        "consumed",
    ),
    AllocationSpec(
        F5_POLYGONAL_BOSSES_H1,
        "f5_polygonal_bosses_h1",
        frozenset({25}),
        "consumed",
    ),
    AllocationSpec(F5_PADS_H1, "f5_pads_h1", frozenset({26}), "consumed"),
    AllocationSpec(F5_HOLES_H1, "f5_holes_h1", frozenset({27}), "consumed"),
    AllocationSpec(F5_CHANNELS_H1, "f5_channels_h1", frozenset({28}), "consumed"),
    AllocationSpec(F5_PLATES_H1, "f5_plates_h1", frozenset({29}), "consumed"),
    AllocationSpec(
        F5_POLYGONAL_STOCK_H1,
        "f5_polygonal_stock_h1",
        frozenset({30}),
        "consumed",
    ),
    AllocationSpec(F5_SLOTS_H1, "f5_slots_h1", frozenset({31}), "consumed"),
    AllocationSpec(F5_POCKETS_H1, "f5_pockets_h1", frozenset({32}), "consumed"),
    AllocationSpec(
        F5_REPEATING_RADIAL_PROFILES_H1,
        "f5_repeating_radial_profiles_h1",
        frozenset({33}),
        "consumed",
    ),
    AllocationSpec(F5_STEP_LEVELS_H1, "f5_step_levels_h1", frozenset({34}), "retired_unrevealed"),
    AllocationSpec(F5_RISERS_H1, "f5_risers_h1", frozenset({35}), "retired_unrevealed"),
    AllocationSpec(
        F4B_SECTION_PASSAGES_H1,
        "f4b_section_passages_h1",
        frozenset({36}),
        "consumed",
    ),
    AllocationSpec(
        F3B_POLYGONAL_BOSSES_H1,
        "f3b_polygonal_bosses_h1",
        frozenset({37}),
        "consumed",
    ),
)


def _validate_allocation_specs(
    specs: tuple[AllocationSpec, ...],
) -> tuple[AllocationSpec, ...]:
    ids = [spec.policy_id for spec in specs]
    tokens = [spec.selection_token for spec in specs]
    if len(ids) != len(set(ids)) or len(tokens) != len(set(tokens)):
        raise ValueError("allocation policy ids and selection tokens must be unique")
    if any(
        not spec.policy_id.replace("-", "").isalnum()
        or spec.policy_id.upper() != spec.policy_id
        or not spec.selection_token.isidentifier()
        or spec.selection_token.lower() != spec.selection_token
        for spec in specs
    ):
        raise ValueError("allocation policy ids or selection tokens are not canonical")
    if any(spec.status not in {"consumed", "retired_unrevealed"} for spec in specs):
        raise ValueError("allocation status is not closed")
    if any(
        not spec.buckets
        or any(
            not isinstance(bucket, int) or isinstance(bucket, bool) or not 0 <= bucket < 1000
            for bucket in spec.buckets
        )
        for spec in specs
    ):
        raise ValueError("allocation buckets must be nonempty exact integers in range")
    named: set[int] = set()
    for spec in specs:
        if named & spec.buckets:
            raise ValueError("historical allocation bucket groups must be mutually disjoint")
        named.update(spec.buckets)
    return specs


_validate_allocation_specs(ALLOCATION_SPECS)
NAMED_ALLOCATIONS: Final = {spec.policy_id: spec.buckets for spec in ALLOCATION_SPECS}
ALLOCATION_STATUSES: Final = {spec.policy_id: spec.status for spec in ALLOCATION_SPECS}

FEATURE_LABELS: Final = {
    0: "chamfer",
    1: "through_hole",
    2: "triangular_passage",
    3: "rectangular_passage",
    4: "6sides_passage",
    5: "triangular_through_slot",
    6: "rectangular_through_slot",
    7: "circular_through_slot",
    8: "rectangular_through_step",
    9: "2sides_through_step",
    10: "slanted_through_step",
    11: "Oring",
    12: "blind_hole",
    13: "triangular_pocket",
    14: "rectangular_pocket",
    15: "6sides_pocket",
    16: "circular_end_pocket",
    17: "rectangular_blind_slot",
    18: "v_circular_end_blind_slot",
    19: "h_circular_end_blind_slot",
    20: "triangular_blind_step",
    21: "circular_blind_step",
    22: "rectangular_blind_step",
    23: "round",
    24: "plane",
    25: "cylinder",
    26: "cone",
}

RELATION_TYPES: Final = frozenset(
    {
        "superpose_on",
        "transition",
        "general_paratactic",
        "line_array",
        "circle_array",
        "mirror",
        "intersecting",
    }
)

# This is a comparison aid, not reconciliation policy.  It follows capabilities.md's naming
# table, and deliberately permits both truthful rectangular-pocket interpretations where direct
# families overlap.  A corpus disagreement is reported; it never accepts or rejects a Candidate.
PACKAGE_FAMILIES_BY_LABEL: Final = {
    0: ("chamfers",),
    1: ("holes",),
    2: ("passages",),
    3: ("passages",),
    4: ("passages",),
    5: ("slots",),
    6: ("slots",),
    7: ("slots",),
    8: (),
    9: (),
    10: (),
    11: ("bosses",),
    12: ("holes",),
    13: ("prismatic_pockets",),
    14: ("pockets", "prismatic_pockets"),
    15: ("prismatic_pockets",),
    16: ("pockets",),
    17: ("pockets",),
    18: ("pockets",),
    19: ("pockets",),
    20: ("angled_steps",),
    21: ("fillets",),
    22: ("pockets",),
    23: ("fillets",),
    24: (),
    25: (),
    26: (),
}

Selection = Literal["development", "holdout"]
SELECTIONS: Final = frozenset({"development", "holdout"})
JsonObject: TypeAlias = dict[str, Any]


@dataclass(frozen=True, slots=True)
class ModelFiles:
    model_id: str
    step: Path
    labels: Path
    relations: Path


@dataclass(frozen=True, slots=True)
class Discovery:
    models: tuple[ModelFiles, ...]
    invalid: tuple[JsonObject, ...]
    selected_files: tuple[Path, ...]
    selected_step_entries: int


class AuditInputError(ValueError):
    """A closed external-corpus refusal safe to retain under ``--record-invalid``."""


def _validate_selection_policy(value: object) -> JsonObject:
    """Validate the reviewed manifest against the scanner's complete closed mirror."""

    if not isinstance(value, dict) or value.get("schema_version") != 2:
        raise ValueError("selection policy must use schema version 2")
    selection = value.get("selection")
    if not isinstance(selection, dict):
        raise ValueError("selection policy must contain a selection object")
    if set(selection) != {
        "algorithm",
        "namespace",
        "development_bucket_ranges",
        "holdout_bucket_ranges",
        "historical_named_allocations",
        "chosen_without",
    }:
        raise ValueError("selection policy keys differ from the schema-2 mirror")
    expected_allocations = {
        allocation: {
            "buckets": sorted(NAMED_ALLOCATIONS[allocation]),
            "status": ALLOCATION_STATUSES[allocation],
        }
        for allocation in sorted(NAMED_ALLOCATIONS)
    }
    if selection.get("algorithm") != (
        "big-endian uint64(sha256(utf8(namespace + NUL + model_id))[0:8]) modulo 1000"
    ):
        raise ValueError("selection policy algorithm differs from the scanner mirror")
    if selection.get("namespace") != SELECTION_NAMESPACE:
        raise ValueError("selection policy namespace differs from the scanner mirror")
    development_ranges = selection.get("development_bucket_ranges")
    if development_ranges != [[0, 499]]:
        raise ValueError("selection policy development buckets differ from the scanner mirror")
    holdout_ranges = selection.get("holdout_bucket_ranges")
    if holdout_ranges != [[500, 999]]:
        raise ValueError("selection policy holdout buckets differ from the scanner mirror")
    manifest_development = frozenset(
        bucket
        for lo, hi in cast(list[list[int]], development_ranges)
        for bucket in range(lo, hi + 1)
    )
    manifest_holdout = frozenset(
        bucket for lo, hi in cast(list[list[int]], holdout_ranges) for bucket in range(lo, hi + 1)
    )
    if manifest_development != DEVELOPMENT_BUCKETS or manifest_holdout != HOLDOUT_BUCKETS:
        raise ValueError("selection policy ranges differ from the scanner bucket sets")
    if selection.get("historical_named_allocations") != expected_allocations:
        raise ValueError("selection policy historical allocations differ from the scanner mirror")
    for allocation, expected_spec in expected_allocations.items():
        if not allocation.replace("-", "").isalnum() or allocation.upper() != allocation:
            raise ValueError(f"invalid allocation id {allocation!r}")
        if expected_spec["status"] not in {"consumed", "retired_unrevealed"}:
            raise ValueError(f"invalid allocation status for {allocation!r}")
        buckets = expected_spec["buckets"]
        if not buckets or any(
            not isinstance(bucket, int) or isinstance(bucket, bool) or not 0 <= bucket < 1000
            for bucket in buckets
        ):
            raise ValueError(f"invalid allocation buckets for {allocation!r}")
    selected = set(DEVELOPMENT_BUCKETS) | set(HOLDOUT_BUCKETS)
    if DEVELOPMENT_BUCKETS & HOLDOUT_BUCKETS or selected != set(range(SELECTION_MODULUS)):
        raise ValueError("selection policy does not partition the selection modulus")
    return cast(JsonObject, value)


def _selection_policy() -> tuple[JsonObject, str]:
    raw = SELECTION_POLICY_PATH.read_bytes()
    return _validate_selection_policy(json.loads(raw)), hashlib.sha256(raw).hexdigest()


def selection_bucket(model_id: str) -> int:
    """The stable bucket for *model_id*, independent of its labels and recognition output."""

    digest = hashlib.sha256(f"{SELECTION_NAMESPACE}\0{model_id}".encode()).digest()
    return int.from_bytes(digest[:8], "big") % SELECTION_MODULUS


def selection_of(model_id: str) -> Selection:
    bucket = selection_bucket(model_id)
    if bucket in DEVELOPMENT_BUCKETS:
        return "development"
    return "holdout"


def _preflight(
    selection: Selection,
    *,
    allow_holdout: bool,
    reveal_allocations: frozenset[str],
) -> None:
    """Authorise a selection before any filesystem or report boundary is crossed."""

    if selection not in SELECTIONS:
        raise ValueError(f"unknown selection {selection!r}")
    _selection_policy()
    if reveal_allocations:
        raise ValueError("historical allocations are retired and cannot be revealed")
    if selection == "holdout":
        if not allow_holdout or reveal_allocations:
            raise ValueError("selection 'holdout' requires only its explicit holdout authority")
        return
    if allow_holdout:
        raise ValueError(f"selection {selection!r} does not accept reveal authority")


def _role_model_ids(steps: Path, labels: Path) -> dict[str, set[str]]:
    return {
        "step": {path.name.removesuffix("_result.step") for path in steps.glob("*_result.step")},
        "labels": {
            path.name.removesuffix("_result.json")
            for path in labels.glob("*_result.json")
            if not path.name.endswith("_result_rel.json")
        },
        "relations": {
            path.name.removesuffix("_result_rel.json") for path in labels.glob("*_result_rel.json")
        },
    }


def _discover(
    root: Path,
    *,
    selection: Selection,
    record_invalid: bool,
    allow_holdout: bool = False,
    reveal_allocations: frozenset[str] = frozenset(),
) -> Discovery:
    """Inventory selected triples before opening them; never let another split block this one."""

    _preflight(
        selection,
        allow_holdout=allow_holdout,
        reveal_allocations=reveal_allocations,
    )
    steps = root / "steps"
    labels = root / "labels"
    if not steps.is_dir() or not labels.is_dir():
        raise ValueError(f"{root} must contain steps/ and labels/ directories")

    roles = _role_model_ids(steps, labels)
    model_ids = sorted(set().union(*roles.values()))
    model_ids = [model_id for model_id in model_ids if selection_of(model_id) == selection]

    models: list[ModelFiles] = []
    invalid: list[JsonObject] = []
    selected_files: list[Path] = []
    for model_id in model_ids:
        step = steps / f"{model_id}_result.step"
        label = labels / f"{model_id}_result.json"
        relation = labels / f"{model_id}_result_rel.json"
        paths = (step, label, relation)
        selected_files.extend(path for path in paths if path.is_file())
        missing = tuple(path.relative_to(root).as_posix() for path in paths if not path.is_file())
        if missing:
            error = f"{model_id} is incomplete; missing {', '.join(missing)}"
            if not record_invalid:
                raise ValueError(error)
            invalid.append({"model_id": model_id, "error": error})
            continue
        models.append(ModelFiles(model_id, step, label, relation))
    if not models and not invalid:
        raise ValueError(f"no models match selection {selection!r}")
    return Discovery(
        tuple(models),
        tuple(invalid),
        tuple(sorted(selected_files)),
        sum(model_id in roles["step"] for model_id in model_ids),
    )


def discover_models(
    root: Path,
    *,
    selection: Selection = "development",
    allow_holdout: bool = False,
    reveal_allocations: frozenset[str] = frozenset(),
) -> tuple[ModelFiles, ...]:
    """Return complete triples, strictly refusing malformed selected inventory."""

    _preflight(
        selection,
        allow_holdout=allow_holdout,
        reveal_allocations=reveal_allocations,
    )
    return _discover(
        root,
        selection=selection,
        record_invalid=False,
        allow_holdout=allow_holdout,
        reveal_allocations=reveal_allocations,
    ).models


def _read_object(path: Path) -> JsonObject:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return cast(JsonObject, value)


def _read_annotation_object(path: Path) -> JsonObject:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except UnicodeDecodeError as exc:
        raise AuditInputError(f"{path}: annotation is not valid UTF-8") from exc
    except json.JSONDecodeError as exc:
        raise AuditInputError(f"{path}: invalid JSON: {exc.msg}") from exc
    if not isinstance(value, dict):
        raise AuditInputError(f"{path} must contain a JSON object")
    return cast(JsonObject, value)


def _integer_face_map(value: object, *, path: Path) -> dict[int, int]:
    if not isinstance(value, dict):
        raise AuditInputError(f"{path}: cls must be an object")
    result: dict[int, int] = {}
    for face, label in value.items():
        if (
            not isinstance(face, str)
            or not face.isascii()
            or not face.isdecimal()
            or str(int(face)) != face
            or not isinstance(label, int)
            or isinstance(label, bool)
        ):
            raise AuditInputError(
                f"{path}: cls keys must be canonical decimal strings and values exact integers"
            )
        result[int(face)] = label
    if sorted(result) != list(range(len(result))):
        raise AuditInputError(f"{path}: cls face ids must be contiguous and zero-based")
    unknown = sorted(set(result.values()) - FEATURE_LABELS.keys())
    if unknown:
        raise AuditInputError(f"{path}: unknown feature labels {unknown}")
    return result


def _instances(
    value: object, *, face_ids: frozenset[int], path: Path
) -> tuple[tuple[int, ...], ...]:
    if not isinstance(value, list):
        raise AuditInputError(f"{path}: seg must be a list")
    result: list[tuple[int, ...]] = []
    for at, raw in enumerate(value):
        if not isinstance(raw, list) or not all(
            isinstance(face, int) and not isinstance(face, bool) for face in raw
        ):
            raise AuditInputError(f"{path}: seg[{at}] must be a list of integer face ids")
        instance = tuple(cast(list[int], raw))
        if len(instance) != len(set(instance)):
            raise AuditInputError(f"{path}: seg[{at}] repeats a face id")
        foreign = sorted(set(instance) - face_ids)
        if foreign:
            raise AuditInputError(f"{path}: seg[{at}] names unknown faces {foreign}")
        result.append(instance)
    return tuple(result)


def _relations(
    value: object, *, instance_count: int, path: Path
) -> tuple[tuple[str, tuple[int, ...]], ...]:
    if not isinstance(value, list):
        raise AuditInputError(f"{path}: relation must be a list")
    result: list[tuple[str, tuple[int, ...]]] = []
    seen: set[tuple[str, tuple[int, ...]]] = set()
    for at, raw in enumerate(value):
        if not isinstance(raw, list) or len(raw) != 2:
            raise AuditInputError(f"{path}: relation[{at}] must be [kind, instance_ids]")
        kind, ids = raw
        if not isinstance(kind, str) or kind not in RELATION_TYPES:
            raise AuditInputError(f"{path}: relation[{at}] has unknown kind {kind!r}")
        if not isinstance(ids, list) or not all(
            isinstance(item, int) and not isinstance(item, bool) for item in ids
        ):
            raise AuditInputError(f"{path}: relation[{at}] instance ids must be integers")
        members = tuple(cast(list[int], ids))
        if len(members) != len(set(members)):
            raise AuditInputError(f"{path}: relation[{at}] repeats an instance id")
        foreign = sorted(item for item in members if not 0 <= item < instance_count)
        if foreign:
            raise AuditInputError(f"{path}: relation[{at}] names unknown instances {foreign}")
        if len(members) < 2:
            raise AuditInputError(f"{path}: relation[{at}] must relate at least two instances")
        relation = (kind, members)
        identity = (kind, tuple(sorted(members)))
        if identity in seen:
            raise AuditInputError(f"{path}: relation[{at}] duplicates an earlier relationship")
        seen.add(identity)
        result.append(relation)
    return tuple(result)


def _bottom(value: object, *, face_ids: frozenset[int], path: Path) -> dict[int, int]:
    if not isinstance(value, dict):
        raise AuditInputError(f"{path}: bottom must be an object")
    result: dict[int, int] = {}
    for face, bottom in value.items():
        if (
            not isinstance(face, str)
            or not face.isascii()
            or not face.isdecimal()
            or str(int(face)) != face
            or not isinstance(bottom, int)
            or isinstance(bottom, bool)
            or bottom not in (0, 1)
        ):
            raise AuditInputError(
                f"{path}: bottom keys must be canonical face ids and values must be 0 or 1"
            )
        result[int(face)] = bottom
    if frozenset(result) != face_ids:
        raise AuditInputError(f"{path}: bottom must cover exactly the cls face ids")
    return result


def _annotation(
    files: ModelFiles,
) -> tuple[
    dict[int, int],
    tuple[tuple[int, ...], ...],
    tuple[tuple[str, tuple[int, ...]], ...],
]:
    labels = _read_annotation_object(files.labels)
    cls = _integer_face_map(labels.get("cls"), path=files.labels)
    seg = _instances(labels.get("seg"), face_ids=frozenset(cls), path=files.labels)
    _bottom(labels.get("bottom"), face_ids=frozenset(cls), path=files.labels)
    relation_doc = _read_annotation_object(files.relations)
    relations = _relations(
        relation_doc.get("relation"), instance_count=len(seg), path=files.relations
    )
    return cls, seg, relations


def _generator_face_order(part: Any) -> tuple[Any, ...]:
    """The raw TopExp order used by the audited upstream ``TopologyExplorer``."""

    from OCP.TopAbs import TopAbs_FACE
    from OCP.TopExp import TopExp_Explorer
    from OCP.TopoDS import TopoDS

    explorer = TopExp_Explorer(part.wrapped, TopAbs_FACE)
    faces: list[Any] = []
    while explorer.More():
        faces.append(TopoDS.Face_s(explorer.Current()))
        explorer.Next()
    return tuple(faces)


def _verified_faces(part: Any, *, model_id: str) -> list[Any]:
    """Refuse attribution unless build123d preserves the generator's raw OCCT face order."""

    faces = list(part.faces())
    generator_faces = _generator_face_order(part)
    if len(faces) != len(generator_faces) or any(
        not upstream.IsSame(face.wrapped)
        for upstream, face in zip(generator_faces, faces, strict=True)
    ):
        raise AuditInputError(
            f"{model_id}: imported face traversal differs from the audited generator order"
        )
    return faces


def _recognition(
    part: Any,
    cls: dict[int, int],
    seg: tuple[tuple[int, ...], ...],
    *,
    faces: list[Any],
) -> JsonObject:
    from quiddity.result import PHYSICAL_FAMILIES, _take_inventory

    face_at = {face: at for at, face in enumerate(faces)}
    instances_at_face: dict[int, list[int]] = defaultdict(list)
    for instance_id, members in enumerate(seg):
        for face_id in members:
            instances_at_face[face_id].append(instance_id)

    product = _take_inventory(part)
    accepted_inventory = product.accepted
    accepted = tuple(
        candidate
        for family in PHYSICAL_FAMILIES
        for candidate in accepted_inventory.candidate_set(family).candidates
    )
    proposals = tuple(
        candidate
        for family in PHYSICAL_FAMILIES
        for candidate in product.physical.candidate_set(family).candidates
    )
    accepted_ids = {id(candidate) for candidate in accepted}
    proposal_candidates_by_family: Counter[str] = Counter()
    proposal_attributed_by_family: Counter[str] = Counter()
    candidates_by_family: Counter[str] = Counter()
    attributed_by_family: Counter[str] = Counter()
    claimed_labels: dict[str, Counter[int]] = defaultdict(Counter)
    touched_instances: dict[str, set[int]] = defaultdict(set)
    accepted_owners: dict[int, set[str]] = defaultdict(set)
    proposal_owners: dict[int, set[str]] = defaultdict(set)
    aligned_faces = off_mapping_faces = 0

    for candidate in proposals:
        family = candidate.family.value
        proposal_candidates_by_family[family] += 1
        defining = product.evidence.defining_of(candidate)
        if defining:
            proposal_attributed_by_family[family] += 1
        if id(candidate) in accepted_ids:
            candidates_by_family[family] += 1
            if defining:
                attributed_by_family[family] += 1
        for node in defining:
            face_id = face_at[product.context.graph.face(node)]
            proposal_owners[face_id].add(family)
            if id(candidate) not in accepted_ids:
                continue
            label = cls[face_id]
            claimed_labels[family][label] += 1
            accepted_owners[face_id].add(family)
            touched_instances[family].update(instances_at_face.get(face_id, ()))
            if family in PACKAGE_FAMILIES_BY_LABEL[label]:
                aligned_faces += 1
            else:
                off_mapping_faces += 1

    contested_accepted = {
        str(face): sorted(families)
        for face, families in sorted(accepted_owners.items())
        if len(families) > 1
    }
    contested_proposals = {
        str(face): sorted(families)
        for face, families in sorted(proposal_owners.items())
        if len(families) > 1
    }
    dispositions = Counter(
        f"{item.outcome.value}:{item.reason.value}" for item in product.reconciliation.dispositions
    )
    return {
        "physical_proposals_by_family": dict(sorted(proposal_candidates_by_family.items())),
        "attributed_proposals_by_family": dict(sorted(proposal_attributed_by_family.items())),
        "accepted_candidates_by_family": dict(sorted(candidates_by_family.items())),
        "attributed_candidates_by_family": dict(sorted(attributed_by_family.items())),
        "claimed_faces_by_family_and_label": {
            family: {str(label): count for label, count in sorted(counts.items())}
            for family, counts in sorted(claimed_labels.items())
        },
        "instances_touched_by_family": {
            family: sorted(instance_ids)
            for family, instance_ids in sorted(touched_instances.items())
        },
        "contested_proposal_defining_faces": contested_proposals,
        "contested_accepted_defining_faces": contested_accepted,
        "dispositions_by_outcome_and_reason": dict(sorted(dispositions.items())),
        "taxonomy_alignment_diagnostic": {
            "aligned_defining_faces": aligned_faces,
            "off_mapping_defining_faces": off_mapping_faces,
            "policy": "comparison only; never used for acceptance or reconciliation",
        },
    }


def audit_model(files: ModelFiles, *, annotations_only: bool = False) -> JsonObject:
    cls, seg, relations = _annotation(files)
    face_labels = Counter(cls.values())
    instance_labels: Counter[int] = Counter()
    mixed_instances: dict[str, list[int]] = {}
    present = 0
    for instance_id, members in enumerate(seg):
        if not members:
            continue
        present += 1
        labels = sorted({cls[face] for face in members})
        if len(labels) == 1:
            instance_labels[labels[0]] += 1
        else:
            mixed_instances[str(instance_id)] = labels

    relation_pairs: Counter[str] = Counter()
    for kind, members in relations:
        relation_pairs[kind] += sum(1 for _ in combinations(members, 2))

    result: JsonObject = {
        "model_id": files.model_id,
        "selection": selection_of(files.model_id),
        "selection_bucket": selection_bucket(files.model_id),
        "faces": len(cls),
        "face_labels": {str(label): count for label, count in sorted(face_labels.items())},
        "instances": len(seg),
        "present_instances": present,
        "empty_instances": len(seg) - present,
        "instance_labels": {str(label): count for label, count in sorted(instance_labels.items())},
        "mixed_label_instances": mixed_instances,
        "relationships": len(relations),
        "relationship_groups_by_type": dict(sorted(Counter(kind for kind, _ in relations).items())),
        "relationship_pairs_by_type": dict(sorted(relation_pairs.items())),
    }
    if annotations_only:
        return result

    from OCP.BRepCheck import BRepCheck_Analyzer
    from OCP.Standard import Standard_Failure

    from quiddity import import_step_geometry as import_step

    try:
        part = import_step(files.step)
    except (ValueError, RuntimeError, Standard_Failure) as exc:
        raise AuditInputError(f"{files.model_id}: STEP import failed: {exc}") from exc
    if not BRepCheck_Analyzer(part.wrapped).IsValid():
        raise AuditInputError(f"{files.model_id}: imported STEP B-rep is invalid")
    if len(part.solids()) != 1:
        raise AuditInputError(f"{files.model_id}: STEP must contain exactly one solid")
    faces = _verified_faces(part, model_id=files.model_id)
    face_count = len(faces)
    if face_count != len(cls):
        raise AuditInputError(
            f"{files.model_id}: STEP has {face_count} faces but annotation has {len(cls)}"
        )
    result["recognition"] = _recognition(part, cls, seg, faces=faces)
    return result


def _artifact_digest(root: Path, files: tuple[Path, ...]) -> str:
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        digest.update(b"\0")
    return digest.hexdigest()


def _merge_counts(target: Counter[str], values: dict[str, object]) -> None:
    for key, value in values.items():
        if not isinstance(value, int):
            raise ValueError(f"count {key!r} must be an integer")
        target[key] += value


def _portable_error(exc: BaseException, root: PurePath) -> str:
    """Normalise selected-input diagnostics for byte-identical cross-platform reports."""

    return str(exc).replace(str(root), "<root>").replace("\\", "/")


def audit(
    root: Path,
    *,
    selection: Selection = "development",
    annotations_only: bool = False,
    record_invalid: bool | None = None,
    allow_holdout: bool = False,
    reveal_allocations: frozenset[str] = frozenset(),
) -> JsonObject:
    record_invalid = selection == "development" if record_invalid is None else record_invalid
    _preflight(
        selection,
        allow_holdout=allow_holdout,
        reveal_allocations=reveal_allocations,
    )
    discovery = _discover(
        root,
        selection=selection,
        record_invalid=record_invalid,
        allow_holdout=allow_holdout,
        reveal_allocations=reveal_allocations,
    )
    models = discovery.models
    reports_list: list[JsonObject] = []
    invalid_models: list[JsonObject] = list(discovery.invalid)
    for model in models:
        try:
            reports_list.append(audit_model(model, annotations_only=annotations_only))
        except AuditInputError as exc:
            if not record_invalid:
                raise
            invalid_models.append(
                {
                    "model_id": model.model_id,
                    "error": _portable_error(exc, root),
                }
            )
    reports = tuple(reports_list)
    if not reports and not invalid_models:
        raise ValueError(f"no valid models match selection {selection!r}")
    face_labels: Counter[str] = Counter()
    instance_labels: Counter[str] = Counter()
    relationship_groups: Counter[str] = Counter()
    relationship_pairs: Counter[str] = Counter()
    selections: Counter[str] = Counter()
    mixed = 0
    empty_instances = present_instances = faces = 0
    recognition_candidates: Counter[str] = Counter()
    recognition_attributed: Counter[str] = Counter()
    recognition_proposals: Counter[str] = Counter()
    recognition_attributed_proposals: Counter[str] = Counter()
    dispositions: Counter[str] = Counter()
    contested_accepted = contested_proposals = aligned_faces = off_mapping_faces = 0

    for report in reports:
        selections[str(report["selection"])] += 1
        faces += int(report["faces"])
        present_instances += int(report["present_instances"])
        empty_instances += int(report["empty_instances"])
        mixed += len(cast(dict[str, object], report["mixed_label_instances"]))
        _merge_counts(face_labels, cast(dict[str, object], report["face_labels"]))
        _merge_counts(instance_labels, cast(dict[str, object], report["instance_labels"]))
        _merge_counts(
            relationship_groups,
            cast(dict[str, object], report["relationship_groups_by_type"]),
        )
        _merge_counts(
            relationship_pairs,
            cast(dict[str, object], report["relationship_pairs_by_type"]),
        )
        recognition = cast(JsonObject | None, report.get("recognition"))
        if recognition is not None:
            _merge_counts(
                recognition_proposals,
                cast(dict[str, object], recognition["physical_proposals_by_family"]),
            )
            _merge_counts(
                recognition_attributed_proposals,
                cast(dict[str, object], recognition["attributed_proposals_by_family"]),
            )
            _merge_counts(
                recognition_candidates,
                cast(dict[str, object], recognition["accepted_candidates_by_family"]),
            )
            _merge_counts(
                recognition_attributed,
                cast(dict[str, object], recognition["attributed_candidates_by_family"]),
            )
            _merge_counts(
                dispositions,
                cast(dict[str, object], recognition["dispositions_by_outcome_and_reason"]),
            )
            contested_proposals += len(
                cast(dict[str, object], recognition["contested_proposal_defining_faces"])
            )
            contested_accepted += len(
                cast(dict[str, object], recognition["contested_accepted_defining_faces"])
            )
            alignment = cast(JsonObject, recognition["taxonomy_alignment_diagnostic"])
            aligned_faces += int(alignment["aligned_defining_faces"])
            off_mapping_faces += int(alignment["off_mapping_defining_faces"])

    summary: JsonObject = {
        "models": len(reports),
        "invalid_models": len(invalid_models),
        "models_by_selection": dict(sorted(selections.items())),
        "faces": faces,
        "face_labels": dict(sorted(face_labels.items(), key=lambda item: int(item[0]))),
        "present_instances": present_instances,
        "empty_instances": empty_instances,
        "instance_labels": dict(sorted(instance_labels.items(), key=lambda item: int(item[0]))),
        "mixed_label_instances": mixed,
        "relationship_groups_by_type": dict(sorted(relationship_groups.items())),
        "relationship_pairs_by_type": dict(sorted(relationship_pairs.items())),
    }
    if not annotations_only:
        summary["recognition"] = {
            "physical_proposals_by_family": dict(sorted(recognition_proposals.items())),
            "attributed_proposals_by_family": dict(
                sorted(recognition_attributed_proposals.items())
            ),
            "accepted_candidates_by_family": dict(sorted(recognition_candidates.items())),
            "attributed_candidates_by_family": dict(sorted(recognition_attributed.items())),
            "contested_proposal_defining_faces": contested_proposals,
            "contested_accepted_defining_faces": contested_accepted,
            "dispositions_by_outcome_and_reason": dict(sorted(dispositions.items())),
            "taxonomy_alignment_diagnostic": {
                "aligned_defining_faces": aligned_faces,
                "off_mapping_defining_faces": off_mapping_faces,
                "policy": "comparison only; never used for acceptance or reconciliation",
            },
        }

    report: JsonObject = {
        "schema_version": 1,
        "dataset": {
            "ref": DATASET_REF,
            "version": DATASET_VERSION,
            "kaggle_id": DATASET_ID,
            "created_at": DATASET_CREATED_AT,
            "licence": DATASET_LICENCE,
            "generator_commit_audited": GENERATOR_COMMIT,
            "selection_namespace": SELECTION_NAMESPACE,
            "selection_modulus": SELECTION_MODULUS,
            "development_buckets": sorted(DEVELOPMENT_BUCKETS),
            "holdout_buckets": sorted(HOLDOUT_BUCKETS),
        },
        "archive_inventory": {
            "selected_step_entries": discovery.selected_step_entries,
            "complete_annotation_triples": len(discovery.models),
            "incomplete_model_ids": [
                item["model_id"]
                for item in discovery.invalid
                if " is incomplete; missing " in cast(str, item["error"])
            ],
        },
        "selection": selection,
        "annotations_only": annotations_only,
        "selected_artifacts": {
            "files": len(discovery.selected_files),
            "sha256": _artifact_digest(root, discovery.selected_files),
            "digest_contract": "sha256(relative-path + NUL + bytes + NUL), sorted by path",
        },
        "summary": summary,
        "models": list(reports),
        "invalid": invalid_models,
    }
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        type=Path,
        help="external MFTRCAD root containing steps/ and labels/",
    )
    parser.add_argument(
        "--selection",
        choices=tuple(sorted(SELECTIONS)),
        default="development",
        help="stable selection to scan (default: development; holdout must stay sealed)",
    )
    parser.add_argument(
        "--annotations-only",
        action="store_true",
        help="validate/summarise annotations without importing STEP or running recognition",
    )
    parser.add_argument(
        "--record-invalid",
        action="store_true",
        help="record malformed selected models in the report instead of stopping",
    )
    parser.add_argument(
        "--reveal-holdout",
        action="store_true",
        help="explicitly authorise scanning the sealed holdout after its review gate",
    )
    parser.add_argument("--json", type=Path, help="write deterministic JSON report here")
    args = parser.parse_args()

    result = audit(
        args.root,
        selection=cast(Selection, args.selection),
        annotations_only=args.annotations_only,
        record_invalid=True if args.record_invalid else None,
        allow_holdout=args.reveal_holdout,
        reveal_allocations=frozenset(),
    )
    rendered = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.json is not None:
        args.json.write_text(rendered, encoding="utf-8")
    print(rendered, end="")


if __name__ == "__main__":
    main()
