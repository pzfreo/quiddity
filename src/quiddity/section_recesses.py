# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Constant-section recess records and recognition.

``SectionRecess`` is the public, geometry-first recess contract selected by ADR 0019.  Face and
body references are zero-based indices in the input part's deterministic face/solid rosters;
they are meaningful only within the recognition result produced for that part.
"""

from __future__ import annotations

from dataclasses import replace

from quiddity._section_recess import (
    ClosedSectionProfile,
    CylindricalEndSurface,
    OpenSectionProfile,
    PlanarEndSurface,
    SectionEnd,
    SectionRecess,
    SectionRecessArray,
    SectionRecessBodyRef,
    SectionRecessClassification,
    SectionRecessDocument,
    SectionRecessEnds,
    SectionRecessEvidence,
    SectionRecessFaceRef,
    SectionRecessGeometry,
    SectionRecessGrid,
    SectionRecessRefusal,
)
from quiddity._typing import Part


def recognise_section_recesses(part: Part) -> list[SectionRecess]:
    """Return every accepted unified constant-section recess in *part*."""

    # Aggregate orchestration calls the private discovery core from the registry.  The public
    # unified view instead projects its completed inventory so specialised passage/recess proofs
    # converge here without sibling recognition or a second reconciliation path.
    from quiddity.result import build_raw_recognition_result

    return list(build_raw_recognition_result(part).section_recesses)


def build_section_recess_document(part: Part) -> SectionRecessDocument:
    """Project accepted aggregate recesses into one deterministic JSON-safe document.

    Recognition and reconciliation run exactly once through the ordinary raw/caller-coordinate
    aggregate.  Occurrence indices are then made dense within this document; body and face indices
    retain the aggregate run's complete input rosters.
    """

    # Local to avoid making result.py and this public facade depend on one another at import time.
    from quiddity.result import build_raw_recognition_result

    result = build_raw_recognition_result(part)
    occurrences = tuple(
        replace(record, index=index) for index, record in enumerate(result.section_recesses)
    )
    return SectionRecessDocument(
        3,
        "result",
        tuple(SectionRecessBodyRef(index) for index, _ in enumerate(part.solids())),
        tuple(SectionRecessFaceRef(index) for index, _ in enumerate(part.faces())),
        occurrences,
        result.section_recess_refusals,
        result.section_recess_patterns,
    )


__all__ = [
    "ClosedSectionProfile",
    "CylindricalEndSurface",
    "OpenSectionProfile",
    "PlanarEndSurface",
    "SectionEnd",
    "SectionRecess",
    "SectionRecessBodyRef",
    "SectionRecessClassification",
    "SectionRecessDocument",
    "SectionRecessEnds",
    "SectionRecessEvidence",
    "SectionRecessFaceRef",
    "SectionRecessGeometry",
    "SectionRecessRefusal",
    "SectionRecessArray",
    "SectionRecessGrid",
    "build_section_recess_document",
    "recognise_section_recesses",
]
