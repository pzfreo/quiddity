#!/usr/bin/env python3
"""Strict dataset adapters and one-inventory effectiveness scoring for Epic 0005 E0.

Corpus annotations are comparison data. They never alter candidates, reconciliation, or package
semantics. This module deliberately consumes the private frozen ``InventoryProduct`` because the
report is repository tooling, not a second public recognition API.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
import statistics
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any

REPORT_FORMAT = "b123d-recognisers-effectiveness"
REPORT_FORMAT_VERSION = 3
_MFCAD_LABEL = re.compile(rb"ADVANCED_FACE\('(\d+)'")
_CLASS_STATUSES = frozenset({"supported", "partial", "unsupported", "incomparable"})
_MAPPED_CLASS_STATUSES = frozenset({"supported", "partial"})
_PUBLIC_FAMILY_EXCEPTIONS = {"pads": "rectangular-pads", "step_levels": "face-levels"}


class EffectivenessDataError(ValueError):
    """The corpus or report cannot support the claimed measurement."""


@dataclass(frozen=True, slots=True)
class DatasetTruth:
    model_id: str
    step_path: Path
    semantic: tuple[int, ...]
    instances: tuple[frozenset[int], ...]
    bottom: tuple[bool, ...] | None
    source_sha256: str


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_mfcadpp_truth(step_path: Path) -> DatasetTruth:
    """Read MFCAD++ semantic face labels without importing or unpickling data."""

    if not step_path.is_file():
        raise EffectivenessDataError(f"missing STEP file: {step_path}")
    labels = tuple(int(value) for value in _MFCAD_LABEL.findall(step_path.read_bytes()))
    if not labels:
        raise EffectivenessDataError(f"no ADVANCED_FACE labels in {step_path}")
    return DatasetTruth(step_path.stem, step_path, labels, (), None, _sha256(step_path))


def _indexed_values(value: object, count: int, context: str) -> tuple[int, ...]:
    if not isinstance(value, dict) or set(value) != {str(index) for index in range(count)}:
        raise EffectivenessDataError(f"{context} must contain exactly face keys 0..{count - 1}")
    result = tuple(value[str(index)] for index in range(count))
    if not all(isinstance(item, int) and not isinstance(item, bool) for item in result):
        raise EffectivenessDataError(f"{context} values must be integers")
    return result


def _instance_components(value: object, count: int) -> tuple[frozenset[int], ...]:
    if not isinstance(value, list) or len(value) != count:
        raise EffectivenessDataError("inst must be a square face-count matrix")
    rows: list[tuple[int, ...]] = []
    for row in value:
        if (
            not isinstance(row, list)
            or len(row) != count
            or any(item not in (0, 1, False, True) for item in row)
        ):
            raise EffectivenessDataError("inst must be a binary square face-count matrix")
        rows.append(tuple(int(item) for item in row))
    if any(
        rows[left][right] != rows[right][left] for left in range(count) for right in range(count)
    ):
        raise EffectivenessDataError("inst must be symmetric")
    remaining = set(range(count))
    components: list[frozenset[int]] = []
    while remaining:
        seed = min(remaining)
        component = frozenset(index for index, linked in enumerate(rows[seed]) if linked)
        if not component:
            # A face can belong to no feature instance. Almost always that is a Stock face,
            # whose row the published data leaves entirely zero. It is not only those: four
            # models in the test partition leave a *feature* face with no instance row, twice
            # while a sibling face of the same class does have one. That is an annotation
            # defect in the corpus rather than a format this adapter can repair, and failing
            # the model would cost the other 9369 under the default fail-closed policy.
            # Both cases contribute no component, so the affected feature never becomes a
            # truth instance and is absent from the instance-recall denominator.
            remaining.discard(seed)
            continue
        if seed not in component or any(
            frozenset(index for index, linked in enumerate(rows[item]) if linked) != component
            for item in component
        ):
            raise EffectivenessDataError("inst rows must encode disjoint equivalence classes")
        components.append(component)
        remaining -= component
    return tuple(components)


def load_mfinstseg_truth(root: Path, model_id: str) -> DatasetTruth:
    """Read the published MFInstSeg ``steps``/``labels`` layout fail closed."""

    candidates = (root / "steps" / f"{model_id}.step", root / "steps" / f"{model_id}.stp")
    present = tuple(path for path in candidates if path.is_file())
    if len(present) != 1:
        raise EffectivenessDataError(f"{model_id}: expected exactly one STEP file")
    label_path = root / "labels" / f"{model_id}.json"
    if not label_path.is_file():
        raise EffectivenessDataError(f"{model_id}: missing label JSON")
    try:
        payload = json.loads(label_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EffectivenessDataError(f"{model_id}: unreadable label JSON") from error
    if (
        not isinstance(payload, list)
        or len(payload) != 1
        or not isinstance(payload[0], list)
        or len(payload[0]) != 2
        or not isinstance(payload[0][1], dict)
    ):
        raise EffectivenessDataError(f"{model_id}: unexpected label document shape")
    labels = payload[0][1]
    if set(labels) != {"seg", "inst", "bottom"}:
        raise EffectivenessDataError(f"{model_id}: labels must contain seg, inst, and bottom")
    inst = labels["inst"]
    if not isinstance(inst, list):
        raise EffectivenessDataError(f"{model_id}: inst must be an array")
    count = len(inst)
    semantic = _indexed_values(labels["seg"], count, f"{model_id}.seg")
    bottom_raw = _indexed_values(labels["bottom"], count, f"{model_id}.bottom")
    if any(value not in (0, 1) for value in bottom_raw):
        raise EffectivenessDataError(f"{model_id}.bottom values must be binary")
    instances = _instance_components(inst, count)
    for instance in instances:
        if len({semantic[index] for index in instance}) != 1:
            raise EffectivenessDataError(f"{model_id}: one instance spans semantic classes")
    combined = hashlib.sha256(
        (_sha256(present[0]) + ":" + _sha256(label_path)).encode("ascii")
    ).hexdigest()
    return DatasetTruth(
        model_id,
        present[0],
        semantic,
        instances,
        tuple(bool(value) for value in bottom_raw),
        combined,
    )


def load_taxonomy(
    path: Path, dataset: str, *, contents: bytes | None = None
) -> dict[int, dict[str, Any]]:
    """Load and validate the closed mapping used by a report."""

    try:
        payload = json.loads(
            path.read_text(encoding="utf-8") if contents is None else contents.decode("utf-8")
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise EffectivenessDataError("taxonomy is unreadable") from error
    if payload.get("format") != "b123d-recognisers-effectiveness-taxonomy":
        raise EffectivenessDataError("unexpected taxonomy format")
    if payload.get("format_version") != 1:
        raise EffectivenessDataError("unsupported taxonomy version")
    datasets = payload.get("datasets")
    if not isinstance(datasets, dict) or dataset not in datasets:
        raise EffectivenessDataError(f"taxonomy has no {dataset} mapping")
    entry = datasets[dataset]
    if not isinstance(entry, dict):
        raise EffectivenessDataError(f"taxonomy {dataset} entry must be an object")
    if "classes_from" in entry:
        source = entry["classes_from"]
        if not isinstance(source, str) or source not in datasets or source == dataset:
            raise EffectivenessDataError("taxonomy classes_from is invalid")
        entry = datasets[source]
    classes = entry.get("classes")
    if not isinstance(classes, dict) or set(classes) != {str(index) for index in range(25)}:
        raise EffectivenessDataError("taxonomy must define exactly classes 0..24")
    result: dict[int, dict[str, Any]] = {}
    for raw_id, row in classes.items():
        if (
            not isinstance(row, dict)
            or set(row) != {"families", "name", "status"}
            or row.get("status") not in _CLASS_STATUSES
            or not isinstance(row.get("name"), str)
            or not isinstance(row.get("families"), list)
            or any(not isinstance(family, str) or not family for family in row["families"])
        ):
            raise EffectivenessDataError(f"invalid taxonomy class {raw_id}")
        if (row["status"] in _MAPPED_CLASS_STATUSES) != bool(row["families"]):
            raise EffectivenessDataError(f"taxonomy class {raw_id} has inconsistent support")
        result[int(raw_id)] = row
    return result


def _public_family_id(internal: str) -> str:
    return _PUBLIC_FAMILY_EXCEPTIONS.get(internal, internal.replace("_", "-"))


def _scoring_family(candidate: object) -> str:
    """Map a physical record to benchmark taxonomy without changing its reported identity."""

    family = _public_family_id(candidate.family.value)  # type: ignore[attr-defined]
    if family != "section-recesses":
        return family
    if candidate.record.classification.feature_kind == "passage":  # type: ignore[attr-defined]
        return "passages"
    shape = candidate.record.classification.section_shape  # type: ignore[attr-defined]
    return "pockets" if shape in {"obround", "circular"} else "prismatic-pockets"


def score_inventory(
    truth: DatasetTruth,
    part: object,
    product: object,
    taxonomy: dict[int, dict[str, Any]],
    seconds: float,
) -> dict[str, Any]:
    """Score one production inventory, adapting physical families to dataset taxonomy."""

    # Authority is captured before production recognisers are imported by the corpus runner.
    from quiddity._candidates import FamilyId, PredicateId
    from quiddity._dispositions import Outcome

    if not math.isfinite(seconds) or seconds < 0.0:
        raise EffectivenessDataError("runtime must be finite and non-negative")
    faces = tuple(part.faces())  # type: ignore[attr-defined]
    if len(faces) != len(truth.semantic):
        raise EffectivenessDataError(
            f"{truth.model_id}: {len(faces)} imported faces != {len(truth.semantic)} labels"
        )
    unknown = sorted(set(truth.semantic) - set(taxonomy))
    if unknown:
        raise EffectivenessDataError(f"{truth.model_id}: unknown classes {unknown}")
    graph = product.context.graph  # type: ignore[attr-defined]
    face_index = {face: index for index, face in enumerate(faces)}
    accepted = tuple(
        disposition.candidate
        for disposition in product.reconciliation.dispositions  # type: ignore[attr-defined]
        if disposition.outcome is Outcome.ACCEPTED
    )
    records: dict[str, int] = {}
    claims: list[tuple[str, frozenset[int]]] = []
    constituents: list[frozenset[int]] = []
    for candidate in accepted:
        record_family = _public_family_id(candidate.family.value)
        scoring_family = _scoring_family(candidate)
        records[record_family] = records.get(record_family, 0) + 1
        indices = frozenset(
            face_index[graph.face(node)]
            for node in product.evidence.defining_of(candidate)  # type: ignore[attr-defined]
        )
        if indices:
            claims.append((scoring_family, indices))
        constituents.append(
            frozenset(
                face_index[graph.face(node)]
                for node in product.evidence.constituent_of(candidate)  # type: ignore[attr-defined]
            )
        )

    accepted_constituent_faces = set().union(*constituents) if constituents else set()
    per_class: dict[str, dict[str, int | str]] = {}
    for class_id, mapping in taxonomy.items():
        labelled = {index for index, value in enumerate(truth.semantic) if value == class_id}
        matched = {
            index
            for family, indices in claims
            if family in mapping["families"]
            for index in indices
            if index in labelled
        }
        covered = labelled.intersection(accepted_constituent_faces)
        claimed = {
            index
            for family, indices in claims
            if family in mapping["families"]
            for index in indices
        }
        truth_instances = tuple(
            instance
            for instance in truth.instances
            if instance and truth.semantic[min(instance)] == class_id
        )
        recalled_instances = sum(
            any(
                family in mapping["families"] and bool(indices & instance)
                for family, indices in claims
            )
            for instance in truth_instances
        )
        per_class[str(class_id)] = {
            "status": mapping["status"],
            "labelled_faces": len(labelled),
            "matched_defining_faces": len(matched),
            "covered_faces": len(covered),
            "mapped_defining_faces": len(claimed),
            "truth_instances": len(truth_instances),
            "recalled_instances": recalled_instances,
        }

    rejected: dict[str, int] = {}
    for disposition in product.reconciliation.dispositions:  # type: ignore[attr-defined]
        if disposition.outcome is Outcome.REJECTED:
            key = disposition.reason.value
            rejected[key] = rejected.get(key, 0) + 1
    diagnostics: dict[str, int] = {}
    for diagnostic in product.diagnostics:  # type: ignore[attr-defined]
        key = diagnostic.code.value
        diagnostics[key] = diagnostics.get(key, 0) + 1
    observations = {
        f"{_public_family_id(family.value)}:{predicate.value}": len(
            product.evidence.observations(family, predicate)  # type: ignore[attr-defined]
        )
        for family in FamilyId
        if family is not FamilyId.LEGACY
        for predicate in PredicateId
        if product.evidence.observations(family, predicate)  # type: ignore[attr-defined]
    }
    mapped_classes: dict[str, int] = {}
    for family, indices in claims:
        matches = Counter(
            truth.semantic[index]
            for index in indices
            if family in taxonomy[truth.semantic[index]]["families"]
        )
        if matches:
            best = max(matches.values())
            winners = sorted(class_id for class_id, count in matches.items() if count == best)
            key = str(winners[0]) if len(winners) == 1 else "ambiguous"
        else:
            key = "unmapped"
        mapped_classes[key] = mapped_classes.get(key, 0) + 1
    mismatches = sum(
        1
        for family, indices in claims
        for index in indices
        if taxonomy[truth.semantic[index]]["status"] in _MAPPED_CLASS_STATUSES
        and family not in taxonomy[truth.semantic[index]]["families"]
    )
    return {
        "model_id": truth.model_id,
        "source_sha256": truth.source_sha256,
        "seconds": seconds,
        "physical_records": dict(sorted(records.items())),
        "mapped_dataset_class_records": dict(sorted(mapped_classes.items())),
        "no_physical_records": not accepted,
        "taxonomy_mismatch_defining_faces": mismatches,
        "reconciliation_drops": dict(sorted(rejected.items())),
        "unsupported_diagnostics": dict(sorted(diagnostics.items())),
        "predicate_observations": dict(sorted(observations.items())),
        "classes": per_class,
    }


def canonical_json(value: object) -> str:
    return json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n"


def summarize_rows(rows: list[dict[str, Any]], selected: int, invalid: int) -> dict[str, Any]:
    """Derive every aggregate from the immutable per-model evidence."""

    valid = [row for row in rows if row.get("status") == "evaluated"]
    records: Counter[str] = Counter()
    mapped_classes: Counter[str] = Counter()
    drops: Counter[str] = Counter()
    diagnostics: Counter[str] = Counter()
    observations: Counter[str] = Counter()
    per_class: dict[str, Counter[str]] = {}
    mismatches = 0
    for row in valid:
        records.update(row["physical_records"])
        mapped_classes.update(row["mapped_dataset_class_records"])
        drops.update(row["reconciliation_drops"])
        diagnostics.update(row["unsupported_diagnostics"])
        observations.update(row["predicate_observations"])
        mismatches += row["taxonomy_mismatch_defining_faces"]
        for class_id, class_row in row["classes"].items():
            aggregate = per_class.setdefault(class_id, Counter())
            for field in (
                "labelled_faces",
                "matched_defining_faces",
                "covered_faces",
                "mapped_defining_faces",
                "truth_instances",
                "recalled_instances",
            ):
                aggregate[field] += class_row[field]
            aggregate["status"] = class_row["status"]

    def ratio(numerator: int, denominator: int) -> dict[str, int | float | None]:
        return {
            "numerator": numerator,
            "denominator": denominator,
            "value": None if denominator == 0 else numerator / denominator,
        }

    classes = {}
    for class_id, aggregate in sorted(per_class.items(), key=lambda item: int(item[0])):
        classes[class_id] = {
            "status": aggregate["status"],
            "defining_face_precision": ratio(
                aggregate["matched_defining_faces"], aggregate["mapped_defining_faces"]
            ),
            "defining_face_recall": ratio(
                aggregate["matched_defining_faces"], aggregate["labelled_faces"]
            ),
            "face_coverage": ratio(aggregate["covered_faces"], aggregate["labelled_faces"]),
            "instance_recall": ratio(aggregate["recalled_instances"], aggregate["truth_instances"]),
        }
    return {
        "selected": selected,
        "loaded": selected - invalid,
        "invalid": invalid,
        "evaluated": len(valid),
        "empty": sum(row["no_physical_records"] for row in valid),
        "physical_records": dict(sorted(records.items())),
        "mapped_dataset_class_records": dict(sorted(mapped_classes.items())),
        "taxonomy_mismatch_defining_faces": mismatches,
        "reconciliation_drops": dict(sorted(drops.items())),
        "unsupported_diagnostics": dict(sorted(diagnostics.items())),
        "predicate_observations": dict(sorted(observations.items())),
        "classes": classes,
    }


def summarize_runtime(rows: list[dict[str, Any]]) -> dict[str, int | float | None]:
    """Derive the runtime distribution from evaluated model rows."""

    values = sorted(row["seconds"] for row in rows if row.get("status") == "evaluated")
    if not values:
        return {
            "count": 0,
            "total_seconds": 0.0,
            "min_seconds": None,
            "median_seconds": None,
            "p95_seconds": None,
            "max_seconds": None,
        }
    return {
        "count": len(values),
        "total_seconds": sum(values),
        "min_seconds": values[0],
        "median_seconds": statistics.median(values),
        "p95_seconds": values[math.ceil(0.95 * len(values)) - 1],
        "max_seconds": values[-1],
    }


def validate_report(report: object) -> None:
    """Validate the closed top-level report contract and its denominator invariants."""

    if not isinstance(report, dict):
        raise EffectivenessDataError("report must be an object")
    required = {
        "format",
        "format_version",
        "dataset",
        "package",
        "environment",
        "selection",
        "mapping",
        "models",
        "summary",
        "runtime",
    }
    if set(report) != required:
        raise EffectivenessDataError("report has unexpected or missing top-level fields")
    if report["format"] != REPORT_FORMAT or report["format_version"] != REPORT_FORMAT_VERSION:
        raise EffectivenessDataError("unsupported report format")
    for field, keys in (
        ("dataset", {"name", "version"}),
        ("package", {"name", "version", "commit"}),
        ("environment", {"python", "build123d", "ocp", "os"}),
        ("mapping", {"format_version", "sha256", "path"}),
    ):
        value = report[field]
        if (
            not isinstance(value, dict)
            or set(value) != keys
            or any(not isinstance(item, (str, int)) for item in value.values())
        ):
            raise EffectivenessDataError(f"report.{field} has invalid metadata")
    selection = report["selection"]
    if (
        not isinstance(selection, dict)
        or not {"rule", "limit", "selected_ids_sha256", "excluded"} <= set(selection)
        or not isinstance(selection["rule"], str)
        or not isinstance(selection["selected_ids_sha256"], str)
        or not isinstance(selection["excluded"], dict)
        or (selection["limit"] is not None and not isinstance(selection["limit"], int))
    ):
        raise EffectivenessDataError("report.selection has invalid metadata")
    models = report["models"]
    if not isinstance(models, list):
        raise EffectivenessDataError("models must be an array")
    ids: list[str] = []
    for row in models:
        if not isinstance(row, dict) or not isinstance(row.get("model_id"), str):
            raise EffectivenessDataError("every model row needs a string model_id")
        status = row.get("status")
        if status == "invalid":
            if set(row) != {"model_id", "status", "reason"} or not isinstance(row["reason"], str):
                raise EffectivenessDataError("invalid model rows need only a reason")
        elif status == "evaluated":
            evaluated_fields = {
                "model_id",
                "source_sha256",
                "seconds",
                "physical_records",
                "mapped_dataset_class_records",
                "no_physical_records",
                "taxonomy_mismatch_defining_faces",
                "reconciliation_drops",
                "unsupported_diagnostics",
                "predicate_observations",
                "classes",
                "status",
            }
            if set(row) != evaluated_fields:
                raise EffectivenessDataError("evaluated model row has invalid fields")
            if (
                not isinstance(row["seconds"], (int, float))
                or not math.isfinite(row["seconds"])
                or row["seconds"] < 0
                or not isinstance(row["no_physical_records"], bool)
            ):
                raise EffectivenessDataError("evaluated model row has invalid scalar values")
            classes = row["classes"]
            if not isinstance(classes, dict) or set(classes) != {
                str(class_id) for class_id in range(25)
            }:
                raise EffectivenessDataError("evaluated model row needs exactly classes 0..24")
            class_fields = {
                "status",
                "labelled_faces",
                "matched_defining_faces",
                "covered_faces",
                "mapped_defining_faces",
                "truth_instances",
                "recalled_instances",
            }
            for class_row in classes.values():
                if (
                    not isinstance(class_row, dict)
                    or set(class_row) != class_fields
                    or class_row.get("status") not in _CLASS_STATUSES
                ):
                    raise EffectivenessDataError("evaluated class row has invalid fields")
                counts = {field: class_row[field] for field in class_fields if field != "status"}
                if any(
                    not isinstance(value, int) or isinstance(value, bool) or value < 0
                    for value in counts.values()
                ):
                    raise EffectivenessDataError(
                        "evaluated class counts must be non-negative integers"
                    )
                if not (
                    counts["matched_defining_faces"]
                    <= counts["covered_faces"]
                    <= counts["labelled_faces"]
                    and counts["matched_defining_faces"] <= counts["mapped_defining_faces"]
                    and counts["recalled_instances"] <= counts["truth_instances"]
                ):
                    raise EffectivenessDataError("evaluated class denominators are inconsistent")
        else:
            raise EffectivenessDataError("model row status must be evaluated or invalid")
        ids.append(row["model_id"])
    if ids != sorted(ids) or len(ids) != len(set(ids)):
        raise EffectivenessDataError("model rows must have unique sorted IDs")
    expected_selection_hash = hashlib.sha256(("\n".join(ids) + "\n").encode("utf-8")).hexdigest()
    if selection["selected_ids_sha256"] != expected_selection_hash:
        raise EffectivenessDataError("selection hash does not match model rows")
    summary = report["summary"]
    if not isinstance(summary, dict):
        raise EffectivenessDataError("summary must be an object")
    for field in ("selected", "loaded", "invalid", "evaluated", "empty"):
        if not isinstance(summary.get(field), int) or summary[field] < 0:
            raise EffectivenessDataError(f"summary.{field} must be non-negative integer")
    if summary["loaded"] + summary["invalid"] != summary["selected"]:
        raise EffectivenessDataError("loaded + invalid must equal selected")
    if summary["evaluated"] > summary["loaded"] or summary["empty"] > summary["evaluated"]:
        raise EffectivenessDataError("summary model denominators are inconsistent")
    if summary["selected"] != len(models):
        raise EffectivenessDataError("summary.selected must equal model row count")
    statuses = Counter(row["status"] for row in models)
    if summary["invalid"] != statuses["invalid"] or summary["evaluated"] != statuses["evaluated"]:
        raise EffectivenessDataError("summary status counts do not match model rows")
    runtime = report["runtime"]
    runtime_fields = {
        "count",
        "total_seconds",
        "min_seconds",
        "median_seconds",
        "p95_seconds",
        "max_seconds",
    }
    if (
        not isinstance(runtime, dict)
        or set(runtime) != runtime_fields
        or runtime.get("count") != summary["evaluated"]
    ):
        raise EffectivenessDataError("runtime metadata does not match evaluated models")
    try:
        expected_summary = summarize_rows(models, len(models), statuses["invalid"])
        expected_runtime = summarize_runtime(models)
    except (KeyError, TypeError, ValueError) as error:
        raise EffectivenessDataError("evaluated model row has invalid nested evidence") from error
    if summary != expected_summary:
        raise EffectivenessDataError("summary does not match model evidence")
    if runtime != expected_runtime:
        raise EffectivenessDataError("runtime does not match model evidence")


__all__ = [
    "DatasetTruth",
    "EffectivenessDataError",
    "REPORT_FORMAT",
    "REPORT_FORMAT_VERSION",
    "canonical_json",
    "load_mfcadpp_truth",
    "load_mfinstseg_truth",
    "load_taxonomy",
    "score_inventory",
    "summarize_rows",
    "summarize_runtime",
    "validate_report",
]
