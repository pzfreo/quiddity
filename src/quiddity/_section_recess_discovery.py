# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Private aggregate discovery for geometry-first section recesses."""

from __future__ import annotations

from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._section_recess import (
    SectionRecess,
    SectionRecessClassification,
    SectionRecessEvidence,
)
from quiddity._section_recess_geometry import _candidates
from quiddity._typing import Part


def discover_section_recesses(
    part: Part, *, writer: EvidenceWriter | None = None
) -> list[SectionRecess]:
    """Discover native section recesses without depending on the public facade."""

    graph = writer.graph if writer is not None else FaceGraph(part)
    found = _candidates(graph)
    records = [
        SectionRecess(
            index,
            candidate.body,
            candidate.geometry,
            SectionRecessClassification("pocket", candidate.section_shape),
            SectionRecessEvidence(candidate.defining_faces, candidate.constituent_faces),
        )
        for index, candidate in enumerate(found)
    ]
    if writer is not None:
        for record in records:
            defining = tuple(graph.nodes[index] for index in record.evidence.defining_faces)
            constituent = tuple(graph.nodes[index] for index in record.evidence.constituent_faces)
            writer.add_defining(
                record,
                defining,
                family=FamilyId.SECTION_RECESSES,
                constituent=constituent,
            )
    return records
