# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""MFTRCAD stays external; these fixtures pin its ingestion contract, not its outcomes."""

from __future__ import annotations

import json
import sys
from copy import deepcopy
from pathlib import Path, PureWindowsPath
from typing import get_args

import pytest
from build123d import Box, export_step

sys.path.insert(0, str(Path(__file__).parents[1] / "tools"))

import mftrcad_audit as audit_module  # noqa: E402
from mftrcad_audit import (  # noqa: E402
    DATASET_REF,
    DATASET_VERSION,
    DEVELOPMENT_BUCKETS,
    FEATURE_LABELS,
    HOLDOUT_BUCKETS,
    NAMED_ALLOCATIONS,
    PACKAGE_FAMILIES_BY_LABEL,
    SELECTIONS,
    Selection,
    audit,
    discover_models,
    selection_bucket,
    selection_of,
)

ROOT = Path(__file__).parents[1]
DEFAULT_MODEL_ID = next(
    f"development-{at}"
    for at in range(10_000)
    if selection_of(f"development-{at}") == "development"
)


def _dataset(tmp_path: Path, *, model_id: str = DEFAULT_MODEL_ID) -> Path:
    root = tmp_path / "mftrcad"
    steps = root / "steps"
    labels = root / "labels"
    steps.mkdir(parents=True, exist_ok=True)
    labels.mkdir(exist_ok=True)

    part = Box(10, 8, 6)
    export_step(part, steps / f"{model_id}_result.step")
    count = len(part.faces())
    cls = {str(at): 24 for at in range(count)}
    cls["0"] = 14
    cls["1"] = 0
    (labels / f"{model_id}_result.json").write_text(
        json.dumps(
            {
                "cls": cls,
                "seg": [[0], [1], []],
                "bottom": {str(at): 0 for at in range(count)},
            }
        ),
        encoding="utf-8",
    )
    (labels / f"{model_id}_result_rel.json").write_text(
        json.dumps({"relation": [["intersecting", [0, 1]]]}),
        encoding="utf-8",
    )
    return root


def test_selection_is_outcome_independent_disjoint_and_stable() -> None:
    assert set(get_args(Selection)) == SELECTIONS
    assert {"development", "holdout"} == SELECTIONS
    assert frozenset(range(0, 500)) == DEVELOPMENT_BUCKETS
    assert frozenset(range(500, 1000)) == HOLDOUT_BUCKETS
    assert DEVELOPMENT_BUCKETS.isdisjoint(HOLDOUT_BUCKETS)
    assert set(range(1000)) == DEVELOPMENT_BUCKETS | HOLDOUT_BUCKETS
    assert selection_bucket("20240116_231044_0") == 113
    assert selection_of("20240116_231044_0") == "development"

    # This exercises both selected arms without reading a label or STEP file. A selection
    # rule that accidentally hashed annotation content could not have this API.
    selected = {f"model-{at}": selection_of(f"model-{at}") for at in range(10_000)}
    assert set(selected.values()) == SELECTIONS
    assert not (
        {name for name, value in selected.items() if value == "development"}
        & {name for name, value in selected.items() if value == "holdout"}
    )


def test_taxonomy_mapping_is_total_and_marks_the_unsupported_group() -> None:
    assert set(PACKAGE_FAMILIES_BY_LABEL) == set(FEATURE_LABELS)
    assert PACKAGE_FAMILIES_BY_LABEL[8] == ()
    assert PACKAGE_FAMILIES_BY_LABEL[9] == ()
    assert PACKAGE_FAMILIES_BY_LABEL[10] == ()
    assert PACKAGE_FAMILIES_BY_LABEL[14] == ("pockets", "prismatic_pockets")
    assert all(PACKAGE_FAMILIES_BY_LABEL[label] == () for label in (24, 25, 26))


def test_checked_in_selection_and_baseline_are_versioned_and_sealed() -> None:
    selection = json.loads(
        (ROOT / "docs/corpora/mftrcad-selection.json").read_text(encoding="utf-8")
    )
    baseline = json.loads(
        (ROOT / "docs/corpora/mftrcad-development-baseline.json").read_text(encoding="utf-8")
    )

    assert selection["dataset"]["ref"] == baseline["dataset_ref"] == DATASET_REF
    assert selection["dataset"]["version"] == baseline["dataset_version"] == DATASET_VERSION
    assert selection["schema_version"] == 2
    assert selection["selection"]["development_bucket_ranges"] == [[0, 499]]
    assert selection["selection"]["holdout_bucket_ranges"] == [[500, 999]]
    assert selection["selection"]["historical_named_allocations"] == {
        spec.policy_id: {"buckets": sorted(spec.buckets), "status": spec.status}
        for spec in audit_module.ALLOCATION_SPECS
    }
    assert baseline["archive_inventory"] == {
        "selected_step_entries": 301,
        "complete_annotation_triples": 300,
        "incomplete_model_ids": ["20240125_003844_9903"],
    }
    assert baseline["selected_artifacts"] == {
        "files": 901,
        "sha256": "5383b0135da4705ffea3f27a27c30c090325ffa44645372448e2ef554ab22e83",
        "digest_contract": "sha256(relative-path + NUL + bytes + NUL), sorted by path",
    }
    assert baseline["holdout"] == {
        "membership_count_inspected": False,
        "models_opened": 0,
        "outcomes_inspected": False,
    }


def test_holdout_requires_an_explicit_post_review_reveal(tmp_path: Path) -> None:
    model_id = next(
        f"holdout-{at}" for at in range(10_000) if selection_of(f"holdout-{at}") == "holdout"
    )
    root = _dataset(tmp_path, model_id=model_id)

    with pytest.raises(ValueError, match="requires only its explicit holdout authority"):
        audit(root, selection="holdout", annotations_only=True)
    with pytest.raises(ValueError, match="requires only its explicit holdout authority"):
        discover_models(root, selection="holdout")
    assert (
        audit(
            root,
            selection="holdout",
            annotations_only=True,
            allow_holdout=True,
        )["summary"]["models"]
        == 1
    )


def test_historical_allocation_authority_is_retired(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="historical allocations are retired"):
        audit(
            tmp_path / "must-not-be-read",
            reveal_allocations=frozenset({next(iter(NAMED_ALLOCATIONS))}),
        )


@pytest.mark.parametrize("entry", ["_discover", "discover_models", "audit"])
def test_unknown_selection_fails_before_touching_the_root(tmp_path: Path, entry: str) -> None:
    root = tmp_path / "must-not-be-read"
    call = getattr(audit_module, entry)
    kwargs = {"selection": "unknown"}
    if entry == "_discover":
        kwargs["record_invalid"] = False
    with pytest.raises(ValueError, match="unknown selection 'unknown'"):
        call(root, **kwargs)


@pytest.mark.parametrize(
    ("mutate", "message"),
    [
        (lambda policy: policy.update(schema_version=1), "schema version 2"),
        (
            lambda policy: policy["selection"].update(namespace="wrong"),
            "namespace differs",
        ),
        (
            lambda policy: policy["selection"]["historical_named_allocations"].update(
                {"F5-EXTRA-H1": {"buckets": [21], "status": "sealed_unrevealed"}}
            ),
            "historical allocations differ",
        ),
        (
            lambda policy: policy["selection"]["historical_named_allocations"][
                next(iter(NAMED_ALLOCATIONS))
            ].update(status="unknown"),
            "historical allocations differ",
        ),
        (
            lambda policy: policy["selection"].update(development_bucket_ranges=[[0, 498]]),
            "development buckets differ",
        ),
        (
            lambda policy: policy["selection"].update(named_allocations={}),
            "keys differ",
        ),
        (
            lambda policy: policy["selection"].update(unselected_bucket_ranges=[[0, 999]]),
            "keys differ",
        ),
    ],
)
def test_selection_policy_mutations_fail_closed(mutate, message: str) -> None:
    policy = json.loads((ROOT / "docs/corpora/mftrcad-selection.json").read_text(encoding="utf-8"))
    changed = deepcopy(policy)
    mutate(changed)
    with pytest.raises(ValueError, match=message):
        audit_module._validate_selection_policy(changed)


@pytest.mark.parametrize(
    "specs",
    [
        (
            audit_module.AllocationSpec(
                "F5-FLATS-H1", "f5_flats_h1", frozenset({20}), "retired_unrevealed"
            ),
            audit_module.AllocationSpec(
                "F5-FLATS-H1", "f5_other_h1", frozenset({21}), "retired_unrevealed"
            ),
        ),
        (
            audit_module.AllocationSpec("F5-FLATS-H1", "f5_flats_h1", frozenset({20}), "consumed"),
            audit_module.AllocationSpec(
                "F5-OTHER-H1", "f5_other_h1", frozenset({20}), "retired_unrevealed"
            ),
        ),
        (
            audit_module.AllocationSpec(
                "F5-FLATS-H1", "f5_flats_h1", frozenset({20}), "retired_unrevealed"
            ),
            audit_module.AllocationSpec(
                "F5-OTHER-H1", "f5_flats_h1", frozenset({21}), "retired_unrevealed"
            ),
        ),
        (
            audit_module.AllocationSpec(
                "F5-FLATS-H1", "F5-FLATS-H1", frozenset({20}), "retired_unrevealed"
            ),
        ),
    ],
)
def test_allocation_roster_refuses_duplicate_or_noncanonical_mappings(specs) -> None:
    with pytest.raises(ValueError, match="unique|canonical|disjoint"):
        audit_module._validate_allocation_specs(specs)


def test_annotation_audit_is_deterministic_and_counts_instances_and_relations(
    tmp_path: Path,
) -> None:
    root = _dataset(tmp_path)
    first = audit(root, annotations_only=True)
    second = audit(root, annotations_only=True)
    assert first == second

    summary = first["summary"]
    assert summary["models"] == 1
    assert summary["faces"] == 6
    assert summary["present_instances"] == 2
    assert summary["empty_instances"] == 1
    assert summary["instance_labels"] == {"0": 1, "14": 1}
    assert summary["relationship_groups_by_type"] == {"intersecting": 1}
    assert summary["relationship_pairs_by_type"] == {"intersecting": 1}


def test_full_audit_proves_step_face_identity_and_uses_accepted_inventory(
    tmp_path: Path,
) -> None:
    result = audit(_dataset(tmp_path), annotations_only=False)
    recognition = result["summary"]["recognition"]
    assert "physical_proposals_by_family" in recognition
    assert "accepted_candidates_by_family" in recognition
    assert "dispositions_by_outcome_and_reason" in recognition
    assert recognition["taxonomy_alignment_diagnostic"]["policy"] == (
        "comparison only; never used for acceptance or reconciliation"
    )


def test_incomplete_model_is_refused(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    next((root / "labels").glob("*_result_rel.json")).unlink()
    with pytest.raises(ValueError, match="is incomplete; missing labels/"):
        discover_models(root)


def test_selected_incomplete_model_is_recorded_but_holdout_one_cannot_block(
    tmp_path: Path,
) -> None:
    root = _dataset(tmp_path)
    selected_relation = root / "labels" / f"{DEFAULT_MODEL_ID}_result_rel.json"
    selected_relation.unlink()
    result = audit(root, annotations_only=True)
    assert result["summary"]["models"] == 0
    assert result["summary"]["invalid_models"] == 1
    assert result["invalid"][0]["model_id"] == DEFAULT_MODEL_ID
    assert "missing labels/" in result["invalid"][0]["error"]

    holdout_id = next(
        f"holdout-{at}" for at in range(10_000) if selection_of(f"holdout-{at}") == "holdout"
    )
    _dataset(tmp_path, model_id=holdout_id)
    (root / "labels" / f"{holdout_id}_result_rel.json").unlink()
    _dataset(tmp_path, model_id=DEFAULT_MODEL_ID)
    result = audit(root, annotations_only=True)
    assert result["summary"]["models"] == 1
    assert result["summary"]["invalid_models"] == 0


def test_selected_orphan_annotation_is_recorded(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    (root / "steps" / f"{DEFAULT_MODEL_ID}_result.step").unlink()
    result = audit(root, annotations_only=True)
    assert result["summary"]["models"] == 0
    assert result["invalid"][0]["model_id"] == DEFAULT_MODEL_ID
    assert "missing steps/" in result["invalid"][0]["error"]


def test_noncontiguous_face_ids_are_refused(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    label_path = next((root / "labels").glob("*_result.json"))
    label = json.loads(label_path.read_text(encoding="utf-8"))
    label["cls"]["7"] = label["cls"].pop("5")
    label_path.write_text(json.dumps(label), encoding="utf-8")
    with pytest.raises(ValueError, match="contiguous and zero-based"):
        audit(root, annotations_only=True, record_invalid=False)


@pytest.mark.parametrize(
    ("mutate", "message"),
    [
        (lambda label: label["cls"].update({"01": 24}), "canonical decimal"),
        (lambda label: label["cls"].update({"0": True}), "exact integers"),
        (lambda label: label["seg"].append([True]), "integer face ids"),
        (lambda label: label["bottom"].pop("0"), "cover exactly"),
        (lambda label: label["bottom"].update({"0": True}), "values must be 0 or 1"),
    ],
)
def test_noncanonical_annotation_scalars_fail_closed(tmp_path: Path, mutate, message: str) -> None:
    root = _dataset(tmp_path)
    label_path = root / "labels" / f"{DEFAULT_MODEL_ID}_result.json"
    label = json.loads(label_path.read_text(encoding="utf-8"))
    mutate(label)
    label_path.write_text(json.dumps(label), encoding="utf-8")
    with pytest.raises(ValueError, match=message):
        audit(root, annotations_only=True, record_invalid=False)


def test_unknown_relation_and_foreign_instance_are_refused(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    relation_path = next((root / "labels").glob("*_result_rel.json"))
    relation_path.write_text(json.dumps({"relation": [["touches", [0, 9]]]}), encoding="utf-8")
    with pytest.raises(ValueError, match="unknown kind 'touches'"):
        audit(root, annotations_only=True, record_invalid=False)


def test_invalid_model_can_be_recorded_without_becoming_evidence(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    relation_path = next((root / "labels").glob("*_result_rel.json"))
    relation_path.write_text(json.dumps({"relation": [["intersecting", [0, 0]]]}), encoding="utf-8")

    result = audit(root, annotations_only=True, record_invalid=True)
    assert result["summary"]["models"] == 0
    assert result["summary"]["invalid_models"] == 1
    assert result["invalid"] == [
        {
            "model_id": DEFAULT_MODEL_ID,
            "error": f"<root>/labels/{DEFAULT_MODEL_ID}_result_rel.json: "
            "relation[0] repeats an instance id",
        }
    ]


@pytest.mark.parametrize(
    "error",
    [KeyError("scanner defect"), ValueError("scanner defect"), RuntimeError("scanner defect")],
)
def test_record_invalid_does_not_hide_scanner_programming_errors(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, error: Exception
) -> None:
    root = _dataset(tmp_path)
    monkeypatch.setattr(
        audit_module,
        "audit_model",
        lambda *args, **kwargs: (_ for _ in ()).throw(error),
    )
    with pytest.raises(type(error), match="scanner defect"):
        audit(root, annotations_only=True, record_invalid=True)


def test_record_invalid_does_not_relabel_annotation_programming_errors(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = _dataset(tmp_path)
    monkeypatch.setattr(
        audit_module,
        "_annotation",
        lambda *args, **kwargs: (_ for _ in ()).throw(ValueError("parser invariant failed")),
    )
    with pytest.raises(ValueError, match="parser invariant failed"):
        audit(root, annotations_only=True, record_invalid=True)


def test_record_invalid_retains_non_utf8_annotation_as_input_error(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    label_path = root / "labels" / f"{DEFAULT_MODEL_ID}_result.json"
    label_path.write_bytes(b"\xff")

    result = audit(root, annotations_only=True, record_invalid=True)

    assert result["summary"]["models"] == 0
    assert result["summary"]["invalid_models"] == 1
    assert result["invalid"] == [
        {
            "model_id": DEFAULT_MODEL_ID,
            "error": f"<root>/labels/{DEFAULT_MODEL_ID}_result.json: annotation is not valid UTF-8",
        }
    ]


@pytest.mark.parametrize("error_type", [ValueError, RuntimeError])
def test_record_invalid_does_not_hide_recognition_lifecycle_errors(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    error_type: type[Exception],
) -> None:
    root = _dataset(tmp_path)
    monkeypatch.setattr(
        audit_module,
        "_recognition",
        lambda *args, **kwargs: (_ for _ in ()).throw(error_type("lifecycle defect")),
    )
    with pytest.raises(error_type, match="lifecycle defect"):
        audit(root, annotations_only=False, record_invalid=True)


def test_duplicate_relationship_group_is_refused(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    relation_path = root / "labels" / f"{DEFAULT_MODEL_ID}_result_rel.json"
    relation_path.write_text(
        json.dumps(
            {
                "relation": [
                    ["intersecting", [0, 1]],
                    ["intersecting", [1, 0]],
                ]
            }
        ),
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="duplicates an earlier relationship"):
        audit(root, annotations_only=True, record_invalid=False)


def test_step_annotation_face_count_mismatch_is_refused(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    label_path = next((root / "labels").glob("*_result.json"))
    label = json.loads(label_path.read_text(encoding="utf-8"))
    label["cls"]["6"] = 24
    label["bottom"]["6"] = 0
    label_path.write_text(json.dumps(label), encoding="utf-8")
    with pytest.raises(ValueError, match="STEP has 6 faces but annotation has 7"):
        audit(root, annotations_only=False, record_invalid=False)


def test_generator_face_order_mismatch_refuses_attribution(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = _dataset(tmp_path)
    original = audit_module._generator_face_order
    monkeypatch.setattr(
        audit_module,
        "_generator_face_order",
        lambda part: tuple(reversed(original(part))),
    )
    with pytest.raises(ValueError, match="traversal differs from the audited generator order"):
        audit(root, annotations_only=False, record_invalid=False)


def test_selected_artifact_digest_is_deterministic_and_content_bound(tmp_path: Path) -> None:
    root = _dataset(tmp_path)
    first = audit(root, annotations_only=True)["selected_artifacts"]
    second = audit(root, annotations_only=True)["selected_artifacts"]
    assert first == second

    relation_path = root / "labels" / f"{DEFAULT_MODEL_ID}_result_rel.json"
    relation_path.write_text(relation_path.read_text(encoding="utf-8") + "\n", encoding="utf-8")
    assert audit(root, annotations_only=True)["selected_artifacts"]["sha256"] != first["sha256"]


def test_report_errors_use_portable_paths() -> None:
    root = PureWindowsPath(r"C:\external\mftrcad")
    error = ValueError(r"C:\external\mftrcad\labels\model_result.json: malformed")

    assert audit_module._portable_error(error, root) == (
        "<root>/labels/model_result.json: malformed"
    )
