# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Versioned, fail-closed correspondence between completed evidence views.

Receipts are durable JSON values.  Their payload is provider-owned: callers persist it and pass it
back, but do not interpret face locators or matching facts.  Resolution returns references issued
by the supplied current view and never starts recognition itself.
"""

from __future__ import annotations

import base64
import contextlib
import hashlib
import itertools
import json
import math
from dataclasses import dataclass
from enum import Enum
from importlib.resources import files
from typing import Protocol, cast

from quiddity._outer_profile import ProfileArc, ProfileLine
from quiddity.evidence import FaceRef, FeatureRef, PlanarOuterProfileEvidence, RecognitionRecord
from quiddity.holes import HoleRecord

CORRESPONDENCE_API_FORMAT = "quiddity-correspondence-api"
CORRESPONDENCE_API_FORMAT_VERSION = 1
_STRATEGY = "analytic-v1"
_PAYLOAD_SCHEMA = 1
_LENGTH_DIGITS = 6
_DIRECTION_DIGITS = 8
_SEARCH_BUDGET = 100_000


class CorrespondenceApiManifestError(ValueError):
    """The installed correspondence API document is unavailable or unsupported."""


class CorrespondenceReceiptError(ValueError):
    """A receipt cannot be issued, decoded, or used under its declared contract."""


class ReceiptSubjectKind(str, Enum):
    FACE = "face"
    FACE_SET = "face-set"
    FEATURE = "feature"


class ResolutionStatus(str, Enum):
    RESOLVED = "resolved"
    MISSING = "missing"
    AMBIGUOUS = "ambiguous"
    INCOMPATIBLE = "incompatible"


class _EvidenceView(Protocol):
    @property
    def features(self) -> tuple[FeatureRef, ...]: ...

    @property
    def faces(self) -> frozenset[FaceRef]: ...

    def family(self, feature: FeatureRef) -> str: ...

    def record(self, feature: FeatureRef) -> RecognitionRecord: ...

    def defining_faces(self, feature: FeatureRef) -> frozenset[FaceRef]: ...

    def constituent_faces(self, feature: FeatureRef) -> frozenset[FaceRef]: ...

    def planar_outer_profile(self, reference: FaceRef) -> object: ...


class CorrespondenceReceipt:
    """One opaque, serializable request to correlate a prior subject.

    ``lineage`` is a caller-owned model/document identity.  It prevents accidental comparison of
    unrelated parts; it is not proof that two revisions correspond.  The provider-owned payload is
    intentionally available only through :meth:`to_dict` and :meth:`to_json` for persistence.
    """

    __slots__ = ("__lineage", "__payload", "__subject_kind")
    __lineage: str
    __payload: str
    __subject_kind: ReceiptSubjectKind

    def __init__(self) -> None:
        raise TypeError("correspondence receipts are issued by issue_correspondence_receipt")

    @property
    def lineage(self) -> str:
        return self.__lineage

    @property
    def subject_kind(self) -> ReceiptSubjectKind:
        return self.__subject_kind

    @property
    def strategy(self) -> str:
        return _STRATEGY

    @property
    def format_version(self) -> int:
        return CORRESPONDENCE_API_FORMAT_VERSION

    def to_dict(self) -> dict[str, object]:
        return {
            "format": CORRESPONDENCE_API_FORMAT,
            "format_version": CORRESPONDENCE_API_FORMAT_VERSION,
            "lineage": self.__lineage,
            "payload": self.__payload,
            "strategy": _STRATEGY,
            "subject_kind": self.__subject_kind.value,
        }

    def to_json(self) -> str:
        return json.dumps(self.to_dict(), separators=(",", ":"), sort_keys=True)

    @classmethod
    def from_dict(cls, value: object) -> CorrespondenceReceipt:
        if not isinstance(value, dict) or set(value) != {
            "format",
            "format_version",
            "lineage",
            "payload",
            "strategy",
            "subject_kind",
        }:
            raise CorrespondenceReceiptError("receipt has an invalid closed shape")
        if (
            value["format"] != CORRESPONDENCE_API_FORMAT
            or value["format_version"] != CORRESPONDENCE_API_FORMAT_VERSION
            or value["strategy"] != _STRATEGY
        ):
            raise CorrespondenceReceiptError("receipt format or strategy is incompatible")
        lineage = value["lineage"]
        payload = value["payload"]
        try:
            kind = ReceiptSubjectKind(value["subject_kind"])
        except (TypeError, ValueError) as error:
            raise CorrespondenceReceiptError("receipt subject kind is unsupported") from error
        if not isinstance(lineage, str) or not lineage:
            raise CorrespondenceReceiptError("receipt lineage must be a non-empty string")
        if not isinstance(payload, str) or not payload:
            raise CorrespondenceReceiptError("receipt payload must be a non-empty string")
        _decode_payload(payload, expected_kind=kind)
        receipt = object.__new__(cls)
        object.__setattr__(receipt, "_CorrespondenceReceipt__lineage", lineage)
        object.__setattr__(receipt, "_CorrespondenceReceipt__payload", payload)
        object.__setattr__(receipt, "_CorrespondenceReceipt__subject_kind", kind)
        return receipt

    @classmethod
    def from_json(cls, value: str) -> CorrespondenceReceipt:
        if not isinstance(value, str):
            raise TypeError("receipt JSON must be a string")
        try:
            decoded = json.loads(value)
        except json.JSONDecodeError as error:
            raise CorrespondenceReceiptError("receipt is not valid JSON") from error
        return cls.from_dict(decoded)

    def _decoded_payload(self) -> dict[str, object]:
        return _decode_payload(self.__payload, expected_kind=self.__subject_kind)


@dataclass(frozen=True, slots=True)
class CorrespondenceResolution:
    """One receipt's outcome in the input batch order."""

    receipt: CorrespondenceReceipt
    status: ResolutionStatus
    feature: FeatureRef | None = None
    faces: frozenset[FaceRef] = frozenset()

    def __post_init__(self) -> None:
        if self.status is ResolutionStatus.RESOLVED:
            if (self.feature is None) == (not self.faces):
                raise ValueError("a resolved correspondence has exactly one subject representation")
        elif self.feature is not None or self.faces:
            raise ValueError("an unresolved correspondence cannot expose current references")


def _finite_number(value: object) -> bool:
    return type(value) in (int, float) and math.isfinite(cast(float, value))


def _validate_payload(value: object, expected_kind: ReceiptSubjectKind) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != {"kind", "schema", "subject"}:
        raise CorrespondenceReceiptError("receipt payload has an invalid closed shape")
    if value["schema"] != _PAYLOAD_SCHEMA or value["kind"] != expected_kind.value:
        raise CorrespondenceReceiptError("receipt payload schema or subject kind is incompatible")
    subject = value["subject"]
    if expected_kind in (ReceiptSubjectKind.FACE, ReceiptSubjectKind.FACE_SET):
        if not isinstance(subject, list) or not subject:
            raise CorrespondenceReceiptError("face receipt payload is empty or malformed")
        if expected_kind is ReceiptSubjectKind.FACE and len(subject) != 1:
            raise CorrespondenceReceiptError("single-face receipt contains multiple faces")
        if not all(_valid_face_descriptor(item) for item in subject):
            raise CorrespondenceReceiptError("face receipt contains a malformed descriptor")
    elif not _valid_hole_descriptor(subject):
        raise CorrespondenceReceiptError("feature receipt contains a malformed hole descriptor")
    return cast(dict[str, object], value)


def _valid_face_descriptor(value: object) -> bool:
    if not isinstance(value, dict) or set(value) != {"normal", "offset", "outer"}:
        return False
    normal, offset, outer = value["normal"], value["offset"], value["outer"]
    return (
        isinstance(normal, list)
        and len(normal) == 3
        and all(_finite_number(item) for item in normal)
        and _finite_number(offset)
        and isinstance(outer, list)
        and len(outer) >= 2
        and all(_valid_support(item) for item in outer)
    )


def _valid_support(value: object) -> bool:
    if not isinstance(value, list) or not value:
        return False
    if value[0] == "line":
        return len(value) == 7 and all(_finite_number(item) for item in value[1:])
    return (
        value[0] == "arc" and len(value) == 12 and all(_finite_number(item) for item in value[1:])
    )


def _valid_hole_descriptor(value: object) -> bool:
    if not isinstance(value, dict) or set(value) != {
        "axis",
        "bottom",
        "cbore",
        "csink",
        "depth",
        "diameter",
        "family",
        "location",
        "spotface",
    }:
        return False
    return (
        value["family"] == "holes"
        and isinstance(value["axis"], list)
        and len(value["axis"]) == 3
        and all(_finite_number(item) for item in value["axis"])
        and isinstance(value["location"], list)
        and len(value["location"]) == 3
        and all(_finite_number(item) for item in value["location"])
        and isinstance(value["bottom"], str)
        and _finite_number(value["depth"])
        and _finite_number(value["diameter"])
        and all(
            value[name] is None or isinstance(value[name], dict)
            for name in ("cbore", "csink", "spotface")
        )
    )


def _encode_payload(value: dict[str, object]) -> str:
    raw = json.dumps(value, separators=(",", ":"), sort_keys=True).encode()
    digest = hashlib.sha256(raw).digest()[:16]
    return base64.urlsafe_b64encode(digest + raw).decode("ascii")


def _decode_payload(payload: str, *, expected_kind: ReceiptSubjectKind) -> dict[str, object]:
    try:
        packed = base64.b64decode(payload.encode("ascii"), altchars=b"-_", validate=True)
    except (ValueError, UnicodeEncodeError) as error:
        raise CorrespondenceReceiptError("receipt payload encoding is invalid") from error
    if len(packed) <= 16:
        raise CorrespondenceReceiptError("receipt payload is truncated")
    digest, raw = packed[:16], packed[16:]
    if hashlib.sha256(raw).digest()[:16] != digest:
        raise CorrespondenceReceiptError("receipt payload integrity check failed")
    try:
        value = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise CorrespondenceReceiptError("receipt payload JSON is invalid") from error
    return _validate_payload(value, expected_kind)


def _rounded(value: float, *, direction: bool = False) -> float:
    result = round(float(value), _DIRECTION_DIGITS if direction else _LENGTH_DIGITS)
    return 0.0 if result == 0.0 else result


def _point(value: tuple[float, float, float], *, direction: bool = False) -> list[float]:
    return [_rounded(item, direction=direction) for item in value]


def _face_descriptor(view: _EvidenceView, reference: FaceRef) -> dict[str, object]:
    inspected = view.planar_outer_profile(reference)
    if not isinstance(inspected, PlanarOuterProfileEvidence):
        raise CorrespondenceReceiptError("analytic-v1 supports only proved planar source faces")
    profile = inspected.profile
    normal = _point(profile.normal, direction=True)
    offset = _rounded(sum(a * b for a, b in zip(profile.origin, profile.normal, strict=True)))
    supports: list[list[object]] = []
    for support in profile.supports:
        if isinstance(support, ProfileLine):
            supports.append(["line", *_point(support.start), *_point(support.end)])
        elif isinstance(support, ProfileArc):
            supports.append(
                [
                    "arc",
                    *_point(support.start),
                    *_point(support.end),
                    *_point(support.center),
                    _rounded(support.radius),
                    _rounded(support.sweep, direction=True),
                ]
            )
        else:  # pragma: no cover - closed PlanarOuterProfile contract
            raise CorrespondenceReceiptError("planar profile contains an unsupported support")
    return {"normal": normal, "offset": offset, "outer": supports}


def _nested_record(value: object) -> dict[str, object] | None:
    if value is None:
        return None
    to_dict = getattr(value, "to_dict", None)
    if not callable(to_dict):
        raise CorrespondenceReceiptError("hole contains an unsupported nested record")
    result = to_dict()
    if not isinstance(result, dict):
        raise CorrespondenceReceiptError("nested hole record is malformed")
    return cast(dict[str, object], result)


def _hole_descriptor(record: HoleRecord) -> dict[str, object]:
    return {
        "axis": _point(record.axis, direction=True),
        "bottom": record.bottom,
        "cbore": _nested_record(record.cbore),
        "csink": _nested_record(record.csink),
        "depth": _rounded(record.depth),
        "diameter": _rounded(record.diameter),
        "family": "holes",
        "location": _point(record.location),
        "spotface": _nested_record(record.spotface),
    }


def issue_correspondence_receipt(
    view: _EvidenceView,
    subject: FeatureRef | FaceRef | frozenset[FaceRef],
    *,
    lineage: str,
) -> CorrespondenceReceipt:
    """Issue one analytic-v1 receipt from an already completed evidence view."""

    if not isinstance(lineage, str) or not lineage:
        raise CorrespondenceReceiptError("lineage must be a non-empty string")
    if type(subject) is FeatureRef:
        if view.family(subject) != "holes" or not isinstance(
            record := view.record(subject), HoleRecord
        ):
            raise CorrespondenceReceiptError("analytic-v1 supports only accepted hole features")
        kind = ReceiptSubjectKind.FEATURE
        content: object = _hole_descriptor(record)
    elif type(subject) is FaceRef:
        kind = ReceiptSubjectKind.FACE
        content = [_face_descriptor(view, subject)]
    elif type(subject) is frozenset:
        if not subject or not all(type(reference) is FaceRef for reference in subject):
            raise CorrespondenceReceiptError("face-set subject must contain only issued FaceRefs")
        kind = ReceiptSubjectKind.FACE_SET
        descriptors = [_face_descriptor(view, reference) for reference in subject]
        content = sorted(descriptors, key=lambda item: json.dumps(item, sort_keys=True))
    else:
        raise TypeError("subject must be a FeatureRef, FaceRef, or frozenset[FaceRef]")
    payload = _encode_payload({"kind": kind.value, "schema": _PAYLOAD_SCHEMA, "subject": content})
    receipt = object.__new__(CorrespondenceReceipt)
    object.__setattr__(receipt, "_CorrespondenceReceipt__lineage", lineage)
    object.__setattr__(receipt, "_CorrespondenceReceipt__payload", payload)
    object.__setattr__(receipt, "_CorrespondenceReceipt__subject_kind", kind)
    return receipt


def _hole_match_rank(prior: dict[str, object], current: dict[str, object]) -> int | None:
    if prior == current:
        return 0
    fixed = ("axis", "bottom", "cbore", "csink", "location", "spotface")
    if all(prior[name] == current[name] for name in fixed):
        return 1  # same tool axis/opening with a resized bore/depth
    # A uniquely placed moved hole may retain its complete machining specification.  This predicate
    # intentionally ignores location only; the batch matcher refuses two interchangeable holes.
    if all(prior[name] == current[name] for name in prior if name != "location"):
        return 2
    return None


def _face_candidates(
    descriptors: list[object], current: list[tuple[FaceRef, dict[str, object]]]
) -> tuple[frozenset[FaceRef], ...]:
    rosters = [
        tuple(reference for reference, value in current if value == descriptor)
        for descriptor in descriptors
    ]
    if any(not roster for roster in rosters):
        return ()
    found: set[frozenset[FaceRef]] = set()
    for attempts, selection in enumerate(itertools.product(*rosters), start=1):
        if attempts > _SEARCH_BUDGET:
            raise CorrespondenceReceiptError("face-set correspondence search budget is exhausted")
        chosen = frozenset(selection)
        if len(chosen) == len(descriptors):
            found.add(chosen)
    return tuple(found)


def _candidate_key(candidate: FeatureRef | frozenset[FaceRef]) -> tuple[object, ...]:
    if isinstance(candidate, FeatureRef):
        return ("feature", id(candidate))
    return ("faces", *(sorted(id(reference) for reference in candidate)))


def _maximum_assignments(
    candidates: list[tuple[FeatureRef | frozenset[FaceRef], ...]],
) -> list[tuple[FeatureRef | frozenset[FaceRef] | None, ...]]:
    best: list[tuple[FeatureRef | frozenset[FaceRef] | None, ...]] = []
    best_count = -1
    attempts = 0

    def visit(
        at: int,
        used: set[tuple[object, ...]],
        assigned: list[FeatureRef | frozenset[FaceRef] | None],
    ) -> None:
        nonlocal attempts, best, best_count
        attempts += 1
        if attempts > _SEARCH_BUDGET:
            raise CorrespondenceReceiptError("correspondence assignment search budget is exhausted")
        if at == len(candidates):
            count = sum(item is not None for item in assigned)
            value = tuple(assigned)
            if count > best_count:
                best_count, best = count, [value]
            elif count == best_count:
                best.append(value)
            return
        visit(at + 1, used, [*assigned, None])
        for candidate in candidates[at]:
            key = _candidate_key(candidate)
            if key not in used:
                visit(at + 1, used | {key}, [*assigned, candidate])

    visit(0, set(), [])
    return best


def resolve_correspondence_receipts(
    receipts: tuple[CorrespondenceReceipt, ...],
    current: _EvidenceView,
    *,
    lineage: str,
) -> tuple[CorrespondenceResolution, ...]:
    """Resolve a batch against one completed current view with a global one-to-one assignment."""

    if type(receipts) is not tuple or not all(
        type(item) is CorrespondenceReceipt for item in receipts
    ):
        raise TypeError("receipts must be a tuple of CorrespondenceReceipt values")
    if not isinstance(lineage, str) or not lineage:
        raise CorrespondenceReceiptError("lineage must be a non-empty string")
    if not receipts:
        return ()

    planar: list[tuple[FaceRef, dict[str, object]]] = []
    for reference in current.faces:
        with contextlib.suppress(CorrespondenceReceiptError):
            planar.append((reference, _face_descriptor(current, reference)))
    holes = tuple(
        (feature, _hole_descriptor(record))
        for feature in current.features
        if current.family(feature) == "holes"
        and isinstance((record := current.record(feature)), HoleRecord)
    )

    compatible_indices: list[int] = []
    candidate_rosters: list[tuple[FeatureRef | frozenset[FaceRef], ...]] = []
    immediate: dict[int, CorrespondenceResolution] = {}
    for index, receipt in enumerate(receipts):
        if receipt.lineage != lineage or receipt.strategy != _STRATEGY:
            immediate[index] = CorrespondenceResolution(receipt, ResolutionStatus.INCOMPATIBLE)
            continue
        payload = receipt._decoded_payload()
        subject = payload["subject"]
        if receipt.subject_kind is ReceiptSubjectKind.FEATURE:
            assert isinstance(subject, dict)  # validated by _decoded_payload
            ranked = tuple(
                (feature, rank)
                for feature, value in holes
                if (rank := _hole_match_rank(subject, value)) is not None
            )
            best_rank = min((rank for _feature, rank in ranked), default=None)
            roster: tuple[FeatureRef | frozenset[FaceRef], ...] = tuple(
                feature for feature, rank in ranked if rank == best_rank
            )
        else:
            assert isinstance(subject, list)  # validated by _decoded_payload
            roster = _face_candidates(subject, planar)
        compatible_indices.append(index)
        candidate_rosters.append(roster)

    assignments = _maximum_assignments(candidate_rosters) if candidate_rosters else []
    for local, original in enumerate(compatible_indices):
        receipt = receipts[original]
        values = {
            None
            if assignment[local] is None
            else _candidate_key(cast(FeatureRef | frozenset[FaceRef], assignment[local]))
            for assignment in assignments
        }
        nonempty = {value for value in values if value is not None}
        if not nonempty:
            immediate[original] = CorrespondenceResolution(receipt, ResolutionStatus.MISSING)
        elif len(nonempty) != 1 or None in values:
            immediate[original] = CorrespondenceResolution(receipt, ResolutionStatus.AMBIGUOUS)
        else:
            chosen = next(
                assignment[local] for assignment in assignments if assignment[local] is not None
            )
            if type(chosen) is FeatureRef:
                immediate[original] = CorrespondenceResolution(
                    receipt, ResolutionStatus.RESOLVED, feature=chosen
                )
            else:
                immediate[original] = CorrespondenceResolution(
                    receipt, ResolutionStatus.RESOLVED, faces=cast(frozenset[FaceRef], chosen)
                )
    return tuple(immediate[index] for index in range(len(receipts)))


def correspondence_api_manifest(
    *, format_version: int = CORRESPONDENCE_API_FORMAT_VERSION
) -> dict[str, object]:
    if type(format_version) is not int or format_version != CORRESPONDENCE_API_FORMAT_VERSION:
        raise CorrespondenceApiManifestError(
            f"unsupported requested format version {format_version!r}"
        )
    raw = files("quiddity").joinpath("correspondence_api.json").read_text(encoding="utf-8")
    manifest = cast(dict[str, object], json.loads(raw))
    _validate_manifest(manifest)
    return cast(dict[str, object], json.loads(json.dumps(manifest)))


def _validate_manifest(manifest: object) -> None:
    from quiddity import __version__

    expected = {
        "CORRESPONDENCE_API_FORMAT",
        "CORRESPONDENCE_API_FORMAT_VERSION",
        "CorrespondenceApiManifestError",
        "CorrespondenceReceipt",
        "CorrespondenceReceiptError",
        "CorrespondenceResolution",
        "ReceiptSubjectKind",
        "ResolutionStatus",
        "correspondence_api_manifest",
        "correspondence_api_manifest_json",
        "issue_correspondence_receipt",
        "resolve_correspondence_receipts",
    }
    if not isinstance(manifest, dict) or set(manifest) != {
        "api",
        "format",
        "format_version",
        "package",
    }:
        raise CorrespondenceApiManifestError("correspondence API manifest has an invalid shape")
    api, package = manifest["api"], manifest["package"]
    if (
        manifest["format"] != CORRESPONDENCE_API_FORMAT
        or manifest["format_version"] != CORRESPONDENCE_API_FORMAT_VERSION
        or not isinstance(package, dict)
        or package != {"name": "quiddity", "version": __version__}
        or not isinstance(api, dict)
        or set(api) != {"major", "namespace", "symbols"}
        or api["major"] != 1
        or api["namespace"] != "quiddity.correspondence"
        or api["symbols"] != sorted(expected)
    ):
        raise CorrespondenceApiManifestError("correspondence API manifest is malformed")


def correspondence_api_manifest_json(
    *, format_version: int = CORRESPONDENCE_API_FORMAT_VERSION
) -> str:
    return (
        json.dumps(
            correspondence_api_manifest(format_version=format_version), indent=2, sort_keys=True
        )
        + "\n"
    )


__all__ = [
    "CORRESPONDENCE_API_FORMAT",
    "CORRESPONDENCE_API_FORMAT_VERSION",
    "CorrespondenceApiManifestError",
    "CorrespondenceReceipt",
    "CorrespondenceReceiptError",
    "CorrespondenceResolution",
    "ReceiptSubjectKind",
    "ResolutionStatus",
    "correspondence_api_manifest",
    "correspondence_api_manifest_json",
    "issue_correspondence_receipt",
    "resolve_correspondence_receipts",
]
