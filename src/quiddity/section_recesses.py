# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Constant-section recess records and recognition.

``SectionRecess`` is the public, geometry-first recess contract selected by ADR 0019.  Face and
body references are zero-based indices in the input part's deterministic face/solid rosters;
they are meaningful only within the recognition result produced for that part.
"""

from __future__ import annotations

from quiddity._candidates import CompletedInputs, FamilyId
from quiddity._definitions import (
    Counted,
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    PhysicalDefinition,
    always,
)
from quiddity._section_recess import (
    ClosedSectionProfile,
    CylindricalEndSurface,
    OpenSectionProfile,
    PlanarEndSurface,
    PlanarEndTerm,
    PlanarEnvelopeEndSurface,
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
from quiddity._section_recess_discovery import discover_section_recesses

__all__ = [
    "ClosedSectionProfile",
    "CylindricalEndSurface",
    "OpenSectionProfile",
    "PlanarEndSurface",
    "PlanarEndTerm",
    "PlanarEnvelopeEndSurface",
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
]


# What this family declares about itself; `_registry` decides where it runs.
def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs  # no completed predecessors
    return list(
        discover_section_recesses(
            writer=services.writer,
            surfaces=services.context.surfaces,
        )
    )


DEFINITION = PhysicalDefinition(
    family=FamilyId.SECTION_RECESSES,
    record_types=(SectionRecess,),
    result_field="section_recesses",
    # Named as a string, not by reference. `recognise_section_recesses` runs the orchestrator to
    # project a completed run, so it lives in `result.py` with the other views; importing
    # it here would put `result` back in this module's chain and close the cycle through
    # `_registry`. `tests/test_registry.py` resolves the name, so a rename still fails.
    public_entrypoint="recognise_section_recesses",
    dependencies=(),
    applicable=always,
    discover=_discover,
    census=Counted("section_recess"),
    attribution=FullyAttributed(
        "every SectionRecess publishes its original wall faces and complete constituent set"
    ),
    evidence=ManifestEvidence(
        golden_paths=(
            "tests/section_recess_expected.json",
            "tests/section_recess_geometry_expected.json",
        ),
        tests=(
            "tests/test_section_recesses.py",
            "tests/test_section_recess_geometry_golden.py",
            "tests/test_section_recess_migration.py",
            "tests/test_section_adapter_rounding.py",
            "tests/test_corner_section.py",
            "tests/test_section_recess_cutover.py",
            "tests/test_open_channel_section.py",
        ),
        extra_records=(
            ("ClosedSectionProfile", "nested", ()),
            ("CylindricalEndSurface", "nested", ()),
            ("OpenSectionProfile", "nested", ()),
            ("PassageFrame", "nested", ()),
            ("PassageSection", "nested", ()),
            ("PassageSectionVertex", "nested", ()),
            ("PlanarEndSurface", "nested", ()),
            ("PlanarEndTerm", "nested", ()),
            ("PlanarEnvelopeEndSurface", "nested", ()),
            ("SectionEnd", "nested", ()),
            ("SectionRecessArray", "projection", ("RecognitionResult.section_recess_patterns",)),
            ("SectionRecessBodyRef", "nested", ()),
            ("SectionRecessClassification", "nested", ()),
            ("SectionRecessDocument", "aggregate", ()),
            ("SectionRecessEnds", "nested", ()),
            ("SectionRecessEvidence", "nested", ()),
            ("SectionRecessFaceRef", "nested", ()),
            ("SectionRecessGeometry", "nested", ()),
            ("SectionRecessGrid", "projection", ("RecognitionResult.section_recess_patterns",)),
            ("SectionRecessRefusal", "projection", ("RecognitionResult.section_recess_refusals",)),
        ),
    ),
)
