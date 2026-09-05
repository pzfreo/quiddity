# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Supported within-run references from accepted recognition to source faces.

References issued here are deliberately not persistent names. They are valid only with the
exact :class:`RecognitionEvidence` that issued them and while its source part remains unchanged.
"""

from __future__ import annotations

import copy
import json
import math
from dataclasses import dataclass
from enum import Enum
from importlib.resources import files
from typing import Generic, NoReturn, Protocol, SupportsIndex, TypeVar, cast

from build123d import Shape
from OCP.TopoDS import TopoDS_Shape

from quiddity._adjacency import FaceNode
from quiddity._candidates import FamilyId
from quiddity._registry import PHYSICAL_DEFINITIONS, RECESS_SOURCE_FAMILIES
from quiddity._typing import CylinderInventory, FaceLike, Part
from quiddity.explanations import RecognitionReport, _project_report
from quiddity.result import InventoryProduct, RecognitionResult, _take_inventory

EVIDENCE_API_FORMAT = "quiddity-evidence-api"
EVIDENCE_API_FORMAT_VERSION = 1


class EvidenceApiManifestError(ValueError):
    """The installed recognition-evidence API document is unavailable or unsupported."""


class RecognitionRecord(Protocol):
    """Common serializable surface of every physical recognition record."""

    def to_dict(self) -> dict[str, object]: ...


MeasureValue = TypeVar("MeasureValue", int, float)
FrameValue = TypeVar("FrameValue")
_ResultValue = TypeVar("_ResultValue")


@dataclass(frozen=True, slots=True)
class AssociationMeasure(Generic[MeasureValue]):
    """One explicit total and its associated/unassociated partition.

    ``ratio`` is undefined when ``total`` is zero. It is association coverage, never an
    accuracy, recall or correctness score.
    """

    total: MeasureValue
    associated: MeasureValue
    unassociated: MeasureValue

    @property
    def ratio(self) -> float | None:
        """Return ``associated / total``, or ``None`` for a zero denominator."""

        if self.total == 0:
            return None
        return float(self.associated) / float(self.total)


@dataclass(frozen=True, slots=True)
class FamilyAssociation:
    """Union contribution from one emitted physical family.

    Contributions from different families may overlap and therefore are not additive.
    """

    family: str
    face_count: int
    surface_area: float


@dataclass(frozen=True, slots=True)
class GeometryAssociation:
    """Bounded accounting of accepted constituent evidence against original faces."""

    face_count: AssociationMeasure[int]
    surface_area: AssociationMeasure[float]
    families: tuple[FamilyAssociation, ...]
    unassociated_faces: frozenset[FaceRef]


class FramedEvidenceRefusalReason(Enum):
    """Why accepted evidence cannot be paired back to caller faces."""

    CALLER_FACE_MAPPING_UNAVAILABLE = "caller-face-mapping-unavailable"


@dataclass(frozen=True, slots=True)
class RefusedFramedEvidence(Generic[_ResultValue]):
    """Mapping refusal, retaining a completed result when recognition already ran.

    Provider-issued values carry ``None`` only when refused before inventory.
    No partially authorized evidence or references are exposed.
    """

    reason: FramedEvidenceRefusalReason
    result: _ResultValue | None = None


class FaceRef:
    """Opaque identity for one source face within one recognition-evidence view."""

    __slots__ = ("__authority",)
    __authority: object

    def __init__(self) -> None:
        raise TypeError("face references are issued by a recognition evidence lifecycle")

    def __reduce_ex__(self, protocol: SupportsIndex) -> NoReturn:
        del protocol
        raise TypeError("face references are run-local and cannot be serialized")


class FeatureRef:
    """Opaque identity for one accepted feature occurrence within one evidence view."""

    __slots__ = ("__authority",)
    __authority: object

    def __init__(self) -> None:
        raise TypeError("feature references are issued by build_recognition_evidence")

    def __reduce_ex__(self, protocol: SupportsIndex) -> NoReturn:
        del protocol
        raise TypeError("feature references are run-local and cannot be serialized")


class RecognitionEvidence:
    """One immutable projection of accepted occurrences and exact source-part faces."""

    __slots__ = (
        "__authority",
        "__result",
        "__report",
        "__features",
        "__feature_records",
        "__feature_defining",
        "__feature_constituent",
        "__feature_families",
        "__faces",
        "__face_nodes",
        "__node_refs",
        "__node_faces",
        "__association",
    )
    __authority: object
    __result: RecognitionResult
    __report: RecognitionReport
    __features: tuple[FeatureRef, ...]
    __feature_records: tuple[RecognitionRecord, ...]
    __feature_defining: tuple[frozenset[FaceNode], ...]
    __feature_constituent: tuple[frozenset[FaceNode], ...]
    __feature_families: tuple[str, ...]
    __faces: frozenset[FaceRef]
    __face_nodes: dict[int, FaceNode]
    __node_refs: dict[FaceNode, FaceRef]
    __node_faces: dict[FaceNode, FaceLike]
    __association: GeometryAssociation

    def __init__(self) -> None:
        raise TypeError("recognition evidence is created by a recognition evidence lifecycle")

    @property
    def result(self) -> RecognitionResult:
        """The existing immutable result projected from this view's one recognition run."""

        return self.__result

    @property
    def report(self) -> RecognitionReport:
        """Bounded explanations from the same inventory, sharing this exact result.

        Detector-family dispositions are not an exhaustive explanation of unassociated
        faces, nor necessarily a one-to-one census of unified public occurrences.
        """

        return self.__report

    @property
    def features(self) -> tuple[FeatureRef, ...]:
        """Accepted physical evidence, including explicit unified-geometry refusals.

        A SectionRecessRefusal preserves an accepted detector's source association but is not
        reconstructible geometry. Consumers must distinguish it from a SectionRecess.
        """

        return self.__features

    @property
    def faces(self) -> frozenset[FaceRef]:
        """All original faces in the exact input part, as unordered opaque references."""

        return self.__faces

    @property
    def association(self) -> GeometryAssociation:
        """Account for faces associated with accepted constituent evidence.

        This does not establish that a feature classification is correct or that an
        unassociated face is a missed feature. Intentional stock/background faces are included
        in the denominator.
        """

        return self.__association

    def family(self, feature: FeatureRef) -> str:
        """Return the stable package family identifier for *feature*."""

        position = self.__feature_position(feature)
        return self.__feature_families[position]

    def record(self, feature: FeatureRef) -> RecognitionRecord:
        """Return the existing immutable recognition record for *feature*."""

        return self.__feature_records[self.__feature_position(feature)]

    def defining_faces(self, feature: FeatureRef) -> frozenset[FaceRef]:
        """Return the exact original faces that establish *feature*."""

        return frozenset(
            self.__node_refs[node]
            for node in self.__feature_defining[self.__feature_position(feature)]
        )

    def constituent_faces(self, feature: FeatureRef) -> frozenset[FaceRef]:
        """Return the exact original faces physically belonging to *feature*."""

        return frozenset(
            self.__node_refs[node]
            for node in self.__feature_constituent[self.__feature_position(feature)]
        )

    def face(self, reference: FaceRef) -> FaceLike:
        """Resolve *reference* to its borrowed source build123d face."""

        node = self.__face_node(reference)
        return self.__node_faces[node]

    def __feature_position(self, feature: FeatureRef) -> int:
        if type(feature) is not FeatureRef:
            raise TypeError("feature must be a FeatureRef")
        if getattr(feature, "_FeatureRef__authority", None) is not self.__authority:
            raise ValueError("feature reference is foreign, copied, forged, or stale")
        try:
            position = self.__features.index(feature)
        except ValueError as error:  # copied values carry the token but not issued identity
            raise ValueError("feature reference is foreign, copied, forged, or stale") from error
        return position

    def __face_node(self, reference: FaceRef) -> FaceNode:
        if type(reference) is not FaceRef:
            raise TypeError("face must be a FaceRef")
        if getattr(reference, "_FaceRef__authority", None) is not self.__authority:
            raise ValueError("face reference is foreign, copied, forged, or stale")
        node = self.__face_nodes.get(id(reference))
        if node is None or self.__node_refs.get(node) is not reference:
            raise ValueError("face reference is foreign, copied, forged, or stale")
        return node


class FramedRecognitionEvidence(Generic[FrameValue]):
    """One framed result with exact working- and caller-face evidence projections.

    The caller must not mutate either retained part while using this view.
    """

    __slots__ = ("__frame", "__part", "__caller_part", "__evidence", "__caller_faces")
    __frame: FrameValue
    __part: Shape[TopoDS_Shape]
    __caller_part: Part
    __evidence: RecognitionEvidence
    __caller_faces: tuple[tuple[FaceRef, FaceLike], ...]

    def __init__(self) -> None:
        raise TypeError("framed recognition evidence is created by the framed evidence lifecycle")

    @property
    def frame(self) -> FrameValue:
        """The caller-space placement of this view's local recognition frame."""

        return self.__frame

    @property
    def part(self) -> Shape[TopoDS_Shape]:
        """The exact local working part used by recognition and :meth:`face`."""

        return self.__part

    @property
    def caller_part(self) -> Part:
        """The exact caller part used by :meth:`caller_face`."""

        return self.__caller_part

    @property
    def result(self) -> RecognitionResult:
        """The existing local-coordinate result from this view's one recognition run."""

        return self.__evidence.result

    @property
    def report(self) -> RecognitionReport:
        """Same-run bounded explanations in the exact local working coordinates."""

        return self.__evidence.report

    @property
    def features(self) -> tuple[FeatureRef, ...]:
        """Accepted occurrences in stable registry/source order."""

        return self.__evidence.features

    @property
    def faces(self) -> frozenset[FaceRef]:
        """Opaque references to every face of the exact local working part."""

        return self.__evidence.faces

    @property
    def association(self) -> GeometryAssociation:
        """Association coverage over this view's local working faces."""

        return self.__evidence.association

    def family(self, feature: FeatureRef) -> str:
        """Return the stable package family identifier for *feature*."""

        return self.__evidence.family(feature)

    def record(self, feature: FeatureRef) -> RecognitionRecord:
        """Return the existing local-coordinate record for *feature*."""

        return self.__evidence.record(feature)

    def defining_faces(self, feature: FeatureRef) -> frozenset[FaceRef]:
        """Return exact local working faces that establish *feature*."""

        return self.__evidence.defining_faces(feature)

    def constituent_faces(self, feature: FeatureRef) -> frozenset[FaceRef]:
        """Return exact local working faces physically belonging to *feature*."""

        return self.__evidence.constituent_faces(feature)

    def face(self, reference: FaceRef) -> FaceLike:
        """Resolve *reference* to its borrowed local working face."""

        return self.__evidence.face(reference)

    def caller_face(self, reference: FaceRef) -> FaceLike:
        """Resolve *reference* to its exact topology-partner face in the caller part."""

        self.__evidence.face(reference)  # validates type, issuer and exact issued identity
        for issued, face in self.__caller_faces:
            if issued is reference:
                return face
        raise ValueError("face reference is foreign, copied, forged, or stale")


def _issue_reference(
    reference_type: type[FaceRef] | type[FeatureRef], authority: object
) -> FaceRef | FeatureRef:
    reference = object.__new__(reference_type)
    object.__setattr__(reference, f"_{reference_type.__name__}__authority", authority)
    return reference


def _issue_framed_recognition_evidence(
    frame: FrameValue,
    part: Shape[TopoDS_Shape],
    caller_part: Part,
    evidence: RecognitionEvidence,
    caller_faces: tuple[tuple[FaceRef, FaceLike], ...],
) -> FramedRecognitionEvidence[FrameValue]:
    """Issue one paired carrier after the frame layer proves exact face mapping."""

    result = object.__new__(FramedRecognitionEvidence)
    object.__setattr__(result, "_FramedRecognitionEvidence__frame", frame)
    object.__setattr__(result, "_FramedRecognitionEvidence__part", part)
    object.__setattr__(result, "_FramedRecognitionEvidence__caller_part", caller_part)
    object.__setattr__(result, "_FramedRecognitionEvidence__evidence", evidence)
    object.__setattr__(result, "_FramedRecognitionEvidence__caller_faces", caller_faces)
    return result


def _project_recognition_evidence(product: InventoryProduct) -> RecognitionEvidence:
    """Project one already-completed inventory without running recognition again."""

    authority = object()
    result = object.__new__(RecognitionEvidence)
    node_refs: dict[FaceNode, FaceRef] = {}
    face_nodes: dict[int, FaceNode] = {}
    node_faces: dict[FaceNode, FaceLike] = {}
    for node in product.context.graph.nodes:
        reference = cast(FaceRef, _issue_reference(FaceRef, authority))
        node_refs[node] = reference
        face_nodes[id(reference)] = node
        node_faces[node] = product.context.graph.face(node)

    feature_refs: list[FeatureRef] = []
    records: list[RecognitionRecord] = []
    defining_sets: list[frozenset[FaceNode]] = []
    constituent_sets: list[frozenset[FaceNode]] = []
    families: list[str] = []
    accepted = product.accepted
    for definition in PHYSICAL_DEFINITIONS:
        if definition.family in RECESS_SOURCE_FAMILIES | {FamilyId.SECTION_RECESSES}:
            continue
        for candidate in accepted.candidate_set(definition.family).candidates:
            feature_refs.append(cast(FeatureRef, _issue_reference(FeatureRef, authority)))
            records.append(cast(RecognitionRecord, candidate.record))
            defining_sets.append(product.evidence.defining_of(candidate))
            constituent_sets.append(product.evidence.constituent_of(candidate))
            families.append(definition.family.value)

    from quiddity._section_recess import SectionRecess, SectionRecessRefusal

    nodes_by_index = {node.index: node for node in product.context.graph.nodes}
    recess_records: tuple[SectionRecess | SectionRecessRefusal, ...] = (
        *product.result.section_recesses,
        *product.result.section_recess_refusals,
    )
    for recess in recess_records:
        feature_refs.append(cast(FeatureRef, _issue_reference(FeatureRef, authority)))
        records.append(recess)
        families.append(FamilyId.SECTION_RECESSES.value)
        defining_sets.append(frozenset(nodes_by_index[i] for i in recess.evidence.defining_faces))
        constituent_sets.append(
            frozenset(nodes_by_index[i] for i in recess.evidence.constituent_faces)
        )
    object.__setattr__(result, "_RecognitionEvidence__authority", authority)
    object.__setattr__(result, "_RecognitionEvidence__result", product.result)
    object.__setattr__(result, "_RecognitionEvidence__report", _project_report(product))
    object.__setattr__(result, "_RecognitionEvidence__features", tuple(feature_refs))
    object.__setattr__(result, "_RecognitionEvidence__feature_records", tuple(records))
    object.__setattr__(result, "_RecognitionEvidence__feature_defining", tuple(defining_sets))
    object.__setattr__(result, "_RecognitionEvidence__feature_constituent", tuple(constituent_sets))
    object.__setattr__(result, "_RecognitionEvidence__feature_families", tuple(families))
    object.__setattr__(result, "_RecognitionEvidence__faces", frozenset(node_refs.values()))
    object.__setattr__(result, "_RecognitionEvidence__face_nodes", face_nodes)
    object.__setattr__(result, "_RecognitionEvidence__node_refs", node_refs)
    object.__setattr__(result, "_RecognitionEvidence__node_faces", node_faces)
    family_nodes: dict[str, set[FaceNode]] = {}
    associated_nodes: set[FaceNode] = set()
    for member_nodes, family in zip(constituent_sets, families, strict=True):
        constituent = set(member_nodes)
        family_nodes.setdefault(family, set()).update(constituent)
        associated_nodes.update(constituent)
    all_nodes = set(node_refs)
    unassociated_nodes = all_nodes - associated_nodes

    def area(nodes: set[FaceNode]) -> float:
        ordered = sorted(nodes, key=lambda node: node.index)
        return math.fsum(float(node_faces[node].area) for node in ordered)

    associated_area = area(associated_nodes)
    unassociated_area = area(unassociated_nodes)
    association = GeometryAssociation(
        face_count=AssociationMeasure(
            total=len(all_nodes),
            associated=len(associated_nodes),
            unassociated=len(unassociated_nodes),
        ),
        surface_area=AssociationMeasure(
            total=associated_area + unassociated_area,
            associated=associated_area,
            unassociated=unassociated_area,
        ),
        families=tuple(
            FamilyAssociation(
                family=definition.family.value,
                face_count=len(nodes),
                surface_area=area(nodes),
            )
            for definition in PHYSICAL_DEFINITIONS
            if (nodes := family_nodes.get(definition.family.value))
        ),
        unassociated_faces=frozenset(node_refs[node] for node in unassociated_nodes),
    )
    object.__setattr__(result, "_RecognitionEvidence__association", association)
    return result


def build_recognition_evidence(
    part: Part,
    *,
    cylinders: CylinderInventory | None = None,
    rotational: bool = False,
) -> RecognitionEvidence:
    """Recognise *part* once and project accepted occurrences to its exact original faces.

    The caller must not mutate *part* while using the returned view. This is the explicit
    raw/caller-coordinate route; use :func:`build_framed_recognition_evidence` for the ordinary
    part-relative lifecycle.
    """

    return _project_recognition_evidence(
        _take_inventory(part, cylinders=cylinders, rotational=rotational)
    )


def evidence_api_manifest(
    *, format_version: int = EVIDENCE_API_FORMAT_VERSION
) -> dict[str, object]:
    """Return an isolated copy of the installed evidence API contract."""

    if type(format_version) is not int or format_version != EVIDENCE_API_FORMAT_VERSION:
        raise EvidenceApiManifestError(f"unsupported requested format version {format_version!r}")
    raw = files("quiddity").joinpath("evidence_api.json").read_text(encoding="utf-8")
    manifest = cast(dict[str, object], json.loads(raw))
    _validate_manifest(manifest)
    return copy.deepcopy(manifest)


def _validate_manifest(manifest: object) -> None:
    from quiddity import __version__

    if not isinstance(manifest, dict) or set(manifest) != {
        "api",
        "format",
        "format_version",
        "package",
    }:
        raise EvidenceApiManifestError("evidence API manifest has an invalid closed shape")
    if manifest["format"] != EVIDENCE_API_FORMAT or (
        type(manifest["format_version"]) is not int
        or manifest["format_version"] != EVIDENCE_API_FORMAT_VERSION
    ):
        raise EvidenceApiManifestError("evidence API manifest format is unsupported")
    package = manifest["package"]
    if (
        not isinstance(package, dict)
        or set(package) != {"name", "version"}
        or (package["name"] != "quiddity" or package["version"] != __version__)
    ):
        raise EvidenceApiManifestError("evidence API package identity or version is invalid")
    api = manifest["api"]
    if not isinstance(api, dict) or set(api) != {
        "major",
        "namespace",
        "references",
        "symbols",
    }:
        raise EvidenceApiManifestError("evidence API declaration has an invalid closed shape")
    symbols = api["symbols"]
    references = api["references"]
    expected_symbols = sorted(
        {
            "EVIDENCE_API_FORMAT",
            "EVIDENCE_API_FORMAT_VERSION",
            "EvidenceApiManifestError",
            "AssociationMeasure",
            "FamilyAssociation",
            "FaceRef",
            "FeatureRef",
            "FramedEvidenceRefusalReason",
            "FramedRecognitionEvidence",
            "GeometryAssociation",
            "RefusedFramedEvidence",
            "RecognitionEvidence",
            "RecognitionRecord",
            "build_recognition_evidence",
            "evidence_api_manifest",
            "evidence_api_manifest_json",
        }
    )
    if (
        api["major"] != 1
        or api["namespace"] != "quiddity.evidence"
        or not isinstance(references, dict)
        or set(references) != {"FaceRef", "FeatureRef"}
        or not all(isinstance(value, str) and value for value in references.values())
        or symbols != expected_symbols
    ):
        raise EvidenceApiManifestError("evidence API declaration is malformed")


def evidence_api_manifest_json(*, format_version: int = EVIDENCE_API_FORMAT_VERSION) -> str:
    """Return the canonical installed evidence API document."""

    manifest = evidence_api_manifest(format_version=format_version)
    return json.dumps(manifest, indent=2, sort_keys=True) + "\n"


__all__ = [
    "AssociationMeasure",
    "EVIDENCE_API_FORMAT",
    "EVIDENCE_API_FORMAT_VERSION",
    "EvidenceApiManifestError",
    "FamilyAssociation",
    "FaceRef",
    "FeatureRef",
    "FramedEvidenceRefusalReason",
    "FramedRecognitionEvidence",
    "GeometryAssociation",
    "RefusedFramedEvidence",
    "RecognitionEvidence",
    "RecognitionRecord",
    "build_recognition_evidence",
    "evidence_api_manifest",
    "evidence_api_manifest_json",
]
