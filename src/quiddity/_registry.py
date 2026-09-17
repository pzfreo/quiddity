# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Closed internal execution registry for recognition orchestration.

The registry is deliberately not a plugin system and does not publish API or schema.  It owns
only physical discovery order, declared physical dependencies, neutral applicability, derived
pattern order, and explicit census coverage.  Public exports, capability metadata, result
projection, reconciliation policy, and census key order remain independently reviewed surfaces.
"""

from __future__ import annotations

from collections.abc import Mapping

from quiddity import (
    angled_steps,
    blends,
    bosses,
    chamfers,
    circular_blind_steps,
    countersinks,
    edge_open_circular_recesses,
    edge_open_prismatic_recesses,
    fillets,
    flats,
    grooves,
    gussets,
    holes,
    levels,
    oriented_slots,
    pads,
    paired_ramp_steps,
    passages,
    plates,
    polygonal_bosses,
    prismatic_pockets,
    profiled_bores,
    rectangular_blind_slots,
    repeating_profiles,
    round_bottom_slots,
    section_recesses,
    slots,
    through_steps,
    turned,
)
from quiddity._candidates import (
    DerivedId,
    FamilyId,
)
from quiddity._definitions import (
    Counted,
    DerivedDefinition,
    FullyAttributed,
    IncompleteAttribution,
    NotCounted,
    PhysicalDefinition,
    always,
    prismatic,
)

# Internal detector identities survive the public SectionRecess schema replacement so that
# discovery, reconciliation and effectiveness scoring remain comparable across the cutover.
RECESS_SOURCE_FAMILIES = frozenset(
    {
        FamilyId.POCKETS,
        FamilyId.CHANNELS,
        FamilyId.PRISMATIC_POCKETS,
        FamilyId.PASSAGES,
        FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
        FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
        FamilyId.RECTANGULAR_BLIND_SLOTS,
        FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
    }
)


PHYSICAL_DEFINITIONS: tuple[PhysicalDefinition, ...] = (
    countersinks.DEFINITION,
    holes.DEFINITION,
    profiled_bores.DEFINITION,
    bosses.DEFINITION,
    polygonal_bosses.BOSSES,
    polygonal_bosses.STOCK,
    slots.CHANNELS,
    slots.SLOTS,
    rectangular_blind_slots.DEFINITION,
    round_bottom_slots.DEFINITION,
    grooves.DEFINITION,
    flats.DEFINITION,
    slots.POCKETS,
    prismatic_pockets.DEFINITION,
    edge_open_circular_recesses.DEFINITION,
    edge_open_prismatic_recesses.DEFINITION,
    section_recesses.DEFINITION,
    pads.DEFINITION,
    repeating_profiles.DEFINITION,
    turned.DEFINITION,
    levels.STEP_LEVELS,
    levels.RISERS,
    chamfers.DEFINITION,
    angled_steps.DEFINITION,
    paired_ramp_steps.DEFINITION,
    gussets.DEFINITION,
    through_steps.DEFINITION,
    circular_blind_steps.DEFINITION,
    passages.DEFINITION,
    oriented_slots.DEFINITION,
    blends.DEFINITION,
    fillets.DEFINITION,
    plates.DEFINITION,
)


DERIVED_DEFINITIONS: tuple[DerivedDefinition, ...] = (
    holes.PATTERNS,
    slots.SLOT_PATTERNS,
    oriented_slots.PATTERNS,
    slots.POCKET_PATTERNS,
    gussets.PATTERNS,
)


def validate_definitions(
    physical: tuple[PhysicalDefinition, ...],
    derived: tuple[DerivedDefinition, ...],
) -> None:
    """Fail closed when the closed internal registry is incomplete or incoherent."""

    families = tuple(definition.family for definition in physical)
    expected = tuple(family for family in FamilyId if family is not FamilyId.LEGACY)
    if len(set(families)) != len(families) or set(families) != set(expected):
        raise ValueError("physical definitions must cover every non-legacy family exactly once")
    positions = {family: index for index, family in enumerate(families)}
    fields = [definition.result_field for definition in physical]
    if len(set(fields)) != len(fields):
        raise ValueError("physical result fields must be unique")
    counted_keys = [
        definition.census.key for definition in physical if isinstance(definition.census, Counted)
    ] + [definition.census.key for definition in derived if isinstance(definition.census, Counted)]
    if len(set(counted_keys)) != len(counted_keys) or any(not key for key in counted_keys):
        raise ValueError("counted census keys must be non-empty and unique")
    for index, definition in enumerate(physical):
        if not definition.record_types or not definition.public_entrypoint:
            raise ValueError("physical definitions require record and public contracts")
        if not isinstance(definition.census, Counted | NotCounted):
            raise ValueError("physical definitions require an explicit census disposition")
        if isinstance(definition.census, NotCounted) and not definition.census.reason:
            raise ValueError("not-counted census reasons must be non-empty")
        if not isinstance(definition.attribution, FullyAttributed | IncompleteAttribution):
            raise ValueError("physical definitions require an attribution disposition")
        if (
            isinstance(definition.attribution, FullyAttributed)
            and not definition.attribution.proof_contract.strip()
        ):
            raise ValueError("fully-attributed proof contracts must be non-empty")
        if isinstance(definition.attribution, IncompleteAttribution) and (
            not definition.attribution.reason.strip()
            or not definition.attribution.follow_up_or_exclusion.strip()
        ):
            raise ValueError("incomplete-attribution reasons and dispositions must be non-empty")
        if definition.applicable not in {always, prismatic}:
            raise ValueError("physical applicability must use a reviewed neutral predicate")
        if definition.projected not in {always, prismatic}:
            raise ValueError("physical projection must use a reviewed neutral predicate")
        if any(
            dependency not in positions or positions[dependency] >= index
            for dependency in definition.dependencies
        ):
            raise ValueError("physical dependencies must exist before their consumer")
    derived_ids = tuple(definition.identifier for definition in derived)
    if len(set(derived_ids)) != len(derived_ids) or set(derived_ids) != set(DerivedId):
        raise ValueError("derived definitions must cover every derived id exactly once")
    derived_fields = [definition.result_field for definition in derived]
    if len(set(derived_fields)) != len(derived_fields) or set(fields) & set(derived_fields):
        raise ValueError("registry result fields must be unique")
    for discoverer in derived:
        if not discoverer.public_entrypoint:
            raise ValueError("derived definitions require a public entrypoint")
    for derived_definition in derived:
        if not derived_definition.record_types:
            raise ValueError("derived definitions require record contracts")
        if not isinstance(derived_definition.census, Counted | NotCounted):
            raise ValueError("derived definitions require an explicit census disposition")
        if (
            isinstance(derived_definition.census, NotCounted)
            and not derived_definition.census.reason
        ):
            raise ValueError("not-counted census reasons must be non-empty")
        if any(source not in positions for source in derived_definition.sources):
            raise ValueError("derived sources must be registered physical families")
        # `_project_result` copies a derived field across verbatim. That is only truthful while
        # every source family projects into the same contexts the derived field does: a
        # projection of an unprojected family has nothing to say. It used to hold that gate at
        # runtime, for the one derived definition sourcing from `passages` -- the sole family
        # with a context-dependent `projected`. That definition is gone, so the gate became a
        # branch no context could reach and no test could cover. Declaring the condition here
        # refuses the case at import instead: reinstate the gate, with a test, before adding a
        # derived family whose source is conditionally projected.
        if any(
            physical[positions[source]].projected is not always
            for source in derived_definition.sources
        ):
            raise ValueError("derived sources must be unconditionally projected")


def validate_result_fields(result_fields: frozenset[str]) -> None:
    """Validate registry coverage against independently declared internal detector fields."""

    registered = {definition.result_field for definition in PHYSICAL_DEFINITIONS} | {
        definition.result_field for definition in DERIVED_DEFINITIONS
    }
    if registered != result_fields:
        raise ValueError("registry fields do not exactly cover physical and derived results")


def validate_output(
    definition: PhysicalDefinition | DerivedDefinition,
    records: list[object],
) -> None:
    """Reject an adapter output that violates its declared record contract."""

    if not all(isinstance(record, definition.record_types) for record in records):
        raise TypeError(f"{definition.result_field} discovery returned an undeclared record type")


def validate_census_contract(
    expected: Mapping[str, str],
    physical: tuple[PhysicalDefinition, ...] = PHYSICAL_DEFINITIONS,
    derived: tuple[DerivedDefinition, ...] = DERIVED_DEFINITIONS,
) -> None:
    """Compare census key-to-source bindings with the independent manual census contract."""

    actual = {
        definition.result_field: definition.census.key
        for definition in physical
        if isinstance(definition.census, Counted)
    } | {
        definition.result_field: definition.census.key
        for definition in derived
        if isinstance(definition.census, Counted)
    }
    if actual != dict(expected):
        raise ValueError("registry census bindings do not match the manual census contract")


validate_definitions(PHYSICAL_DEFINITIONS, DERIVED_DEFINITIONS)
