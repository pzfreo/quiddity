# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

import ast
import inspect
import sys
import types
import typing
from dataclasses import fields, replace
from inspect import signature
from pathlib import Path
from types import ModuleType

import pytest
from build123d import Box, Pos

import quiddity as public
import quiddity.result as result_module
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._definitions import AcceptedInputs
from quiddity._record import Record
from quiddity._registry import (
    DERIVED_DEFINITIONS,
    PHYSICAL_DEFINITIONS,
    Counted,
    DerivedId,
    FullyAttributed,
    IncompleteAttribution,
    NotCounted,
    always,
    prismatic,
    validate_census_contract,
    validate_definitions,
    validate_output,
    validate_result_fields,
)
from quiddity.census import CENSUS_BINDINGS, CENSUS_KEYS
from quiddity.result import MIGRATED, PHYSICAL_FAMILIES, _take_inventory

ROOT = Path(__file__).parents[1]


def test_registry_is_the_closed_ordered_internal_roster() -> None:
    assert len(PHYSICAL_DEFINITIONS) == 33
    assert len(DERIVED_DEFINITIONS) == 5
    assert tuple(item.family for item in PHYSICAL_DEFINITIONS) == PHYSICAL_FAMILIES
    assert set(PHYSICAL_FAMILIES) == set(FamilyId) - {FamilyId.LEGACY}
    assert tuple(item.identifier for item in DERIVED_DEFINITIONS) == tuple(DerivedId)
    assert all(item.public_entrypoint for item in DERIVED_DEFINITIONS)
    assert all(isinstance(item.census, Counted | NotCounted) for item in PHYSICAL_DEFINITIONS)
    assert all(isinstance(item.census, Counted | NotCounted) for item in DERIVED_DEFINITIONS)
    assert {
        item.family
        for item in PHYSICAL_DEFINITIONS
        if isinstance(item.attribution, FullyAttributed)
    } == {
        FamilyId.PRISMATIC_POCKETS,
        FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
        FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
        FamilyId.PASSAGES,
        FamilyId.ORIENTED_SLOTS,
        FamilyId.GROOVES,
        FamilyId.GUSSET_RIBS,
        FamilyId.TURNED_STEPS,
        FamilyId.CHAMFERS,
        FamilyId.ANGLED_STEPS,
        FamilyId.PAIRED_RAMP_STEPS,
        FamilyId.THROUGH_STEPS,
        FamilyId.CIRCULAR_BLIND_STEPS,
        FamilyId.FLATS,
        FamilyId.FILLETS,
        FamilyId.COUNTERSINKS,
        FamilyId.HOLES,
        FamilyId.CHANNELS,
        FamilyId.BOSSES,
        FamilyId.BLENDS,
        FamilyId.DOUBLE_D_BORES,
        FamilyId.POLYGONAL_BOSSES,
        FamilyId.POLYGONAL_STOCK,
        FamilyId.PADS,
        FamilyId.PLATES,
        FamilyId.REPEATING_RADIAL_PROFILES,
        FamilyId.SLOTS,
        FamilyId.RECTANGULAR_BLIND_SLOTS,
        FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
        FamilyId.POCKETS,
        FamilyId.STEP_LEVELS,
        FamilyId.RISERS,
        FamilyId.SECTION_RECESSES,
    }
    assert all(
        isinstance(item.attribution, FullyAttributed | IncompleteAttribution)
        for item in PHYSICAL_DEFINITIONS
    )
    assert PHYSICAL_FAMILIES == (
        FamilyId.COUNTERSINKS,
        FamilyId.HOLES,
        FamilyId.DOUBLE_D_BORES,
        FamilyId.BOSSES,
        FamilyId.POLYGONAL_BOSSES,
        FamilyId.POLYGONAL_STOCK,
        FamilyId.CHANNELS,
        FamilyId.SLOTS,
        FamilyId.RECTANGULAR_BLIND_SLOTS,
        FamilyId.ROUND_BOTTOM_BLIND_SLOTS,
        FamilyId.GROOVES,
        FamilyId.FLATS,
        FamilyId.POCKETS,
        FamilyId.PRISMATIC_POCKETS,
        FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
        FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
        FamilyId.SECTION_RECESSES,
        FamilyId.PADS,
        FamilyId.REPEATING_RADIAL_PROFILES,
        FamilyId.TURNED_STEPS,
        FamilyId.STEP_LEVELS,
        FamilyId.RISERS,
        FamilyId.CHAMFERS,
        FamilyId.ANGLED_STEPS,
        FamilyId.PAIRED_RAMP_STEPS,
        FamilyId.GUSSET_RIBS,
        FamilyId.THROUGH_STEPS,
        FamilyId.CIRCULAR_BLIND_STEPS,
        FamilyId.PASSAGES,
        FamilyId.ORIENTED_SLOTS,
        FamilyId.BLENDS,
        FamilyId.FILLETS,
        FamilyId.PLATES,
    )


@pytest.mark.parametrize(
    "attribution",
    [
        FullyAttributed(""),
        FullyAttributed("   "),
        IncompleteAttribution("", "follow-up"),
        IncompleteAttribution("   ", "follow-up"),
        IncompleteAttribution("reason", ""),
        IncompleteAttribution("reason", "   "),
    ],
)
def test_registry_rejects_empty_attribution_contracts(attribution) -> None:
    changed = (replace(PHYSICAL_DEFINITIONS[0], attribution=attribution), *PHYSICAL_DEFINITIONS[1:])
    with pytest.raises(ValueError, match="attribut"):
        validate_definitions(changed, DERIVED_DEFINITIONS)


def test_step_levels_fulfil_their_body_local_attribution_promise() -> None:
    product = _take_inventory(Box(60, 60, 10) + Pos(20, 0, 7.5) * Box(20, 20, 5))
    candidates = product.physical.candidate_set(FamilyId.STEP_LEVELS).candidates

    assert candidates
    assert all(product.evidence.defining_of(candidate) for candidate in candidates)
    assert all(
        product.context.graph.common_valid_solid(product.evidence.defining_of(candidate))
        is not None
        for candidate in candidates
    )


def test_terminal_validator_rechecks_partial_family_body_provenance(monkeypatch) -> None:
    product = _take_inventory(Box(30, 30, 10) - Box(12, 5, 20))
    slot = product.physical.candidate_set(FamilyId.SLOTS).candidates[0]
    assert product.evidence.defining_of(slot)
    monkeypatch.setattr(product.context.graph, "common_valid_solid", lambda nodes: None)

    with pytest.raises(ValueError, match="lost its common valid solid"):
        result_module._validate_attribution(product.context, product.physical, product.evidence)


def test_terminal_validator_reads_issuer_snapshots_not_mutated_candidate_state() -> None:
    product = _take_inventory(Box(30, 30, 10) - Box(12, 5, 20))
    slot = product.physical.candidate_set(FamilyId.SLOTS).candidates[0]
    object.__setattr__(slot.evidence, "defining", frozenset())

    with pytest.raises(ValueError, match="no longer matches its issued state"):
        result_module._validate_attribution(product.context, product.physical, product.evidence)


def test_registry_dependencies_are_explicit_and_restricted() -> None:
    dependencies = {
        item.family: item.dependencies for item in PHYSICAL_DEFINITIONS if item.dependencies
    }
    assert dependencies == {
        FamilyId.HOLES: (FamilyId.COUNTERSINKS,),
        FamilyId.ORIENTED_SLOTS: (FamilyId.PASSAGES,),
        FamilyId.PLATES: (FamilyId.TURNED_STEPS,),
        FamilyId.RISERS: (FamilyId.STEP_LEVELS,),
    }
    sources = {item.identifier: item.sources for item in DERIVED_DEFINITIONS}
    assert sources == {
        DerivedId.HOLE_PATTERNS: (FamilyId.HOLES,),
        DerivedId.SLOT_PATTERNS: (FamilyId.SLOTS,),
        DerivedId.ORIENTED_SLOT_PATTERNS: (FamilyId.ORIENTED_SLOTS,),
        DerivedId.POCKET_PATTERNS: (FamilyId.POCKETS,),
        DerivedId.GUSSET_RIB_PATTERNS: (FamilyId.GUSSET_RIBS,),
    }
    ledger = ClaimLedger(FaceGraph(Box(2, 2, 2)), definitions=PHYSICAL_DEFINITIONS)
    ledger.candidate_set_for(FamilyId.COUNTERSINKS, ())
    holes = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.HOLES)
    completed = ledger.restricted_inputs(holes)
    accepted = AcceptedInputs.restricted((FamilyId.SLOTS,), {FamilyId.SLOTS: ()})
    with pytest.raises(ValueError, match="not a declared"):
        completed.records(FamilyId.SLOTS, object)
    with pytest.raises(ValueError, match="not a declared"):
        accepted.records(FamilyId.HOLES, object)


def test_registry_rejects_wrong_typed_dependency_values() -> None:
    ledger = ClaimLedger(FaceGraph(Box(2, 2, 2)), definitions=PHYSICAL_DEFINITIONS)
    ledger.candidate_set_for(FamilyId.COUNTERSINKS, (object(),))
    holes = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.HOLES)
    completed = ledger.restricted_inputs(holes)
    with pytest.raises(TypeError, match="wrong record type"):
        completed.records(FamilyId.COUNTERSINKS, public.CounterSink)

    accepted = AcceptedInputs.restricted((FamilyId.HOLES,), {FamilyId.HOLES: (object(),)})
    with pytest.raises(TypeError, match="wrong record type"):
        accepted.records(FamilyId.HOLES, public.HoleRecord)


def test_registry_distinguishes_an_empty_dependency_from_one_not_run() -> None:
    ledger = ClaimLedger(FaceGraph(Box(2, 2, 2)), definitions=PHYSICAL_DEFINITIONS)
    ledger.candidate_set_for(FamilyId.COUNTERSINKS, ())
    holes = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.HOLES)
    completed = ledger.restricted_inputs(holes)
    assert completed.records(FamilyId.COUNTERSINKS, public.CounterSink) == ()

    with pytest.raises(ValueError, match="has not completed"):
        ClaimLedger(FaceGraph(Box(2, 2, 2)), definitions=PHYSICAL_DEFINITIONS).restricted_inputs(
            holes
        )


def test_inapplicable_family_completes_as_an_empty_dependency(monkeypatch) -> None:
    turned = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.TURNED_STEPS)
    definitions = tuple(
        replace(item, applicable=prismatic) if item is turned else item
        for item in PHYSICAL_DEFINITIONS
    )
    monkeypatch.setattr(result_module, "PHYSICAL_DEFINITIONS", definitions)

    result = result_module.build_recognition_result(Box(20, 20, 10), rotational=True)
    assert result.turned_steps == ()


def test_registry_fields_and_public_entrypoints_have_independent_coverage() -> None:
    result_fields = {item.name for item in fields(result_module._LegacyRecognitionResult)}
    orchestration_context = {
        "cylinders",
        "rotational",
        "section_recess_patterns",
        "section_recess_refusals",
    }
    validate_result_fields(frozenset(result_fields - orchestration_context))
    from tools._legacy_recognition import __all__ as retired

    assert {
        item.public_entrypoint
        for item in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
        if item.public_entrypoint is not None and item.public_entrypoint not in retired
    } == MIGRATED
    assert all(
        hasattr(public, item.public_entrypoint)
        for item in PHYSICAL_DEFINITIONS
        if item.public_entrypoint not in retired
    )
    assert all(
        item.public_entrypoint is None
        or item.public_entrypoint in retired
        or hasattr(public, item.public_entrypoint)
        for item in DERIVED_DEFINITIONS
    )
    manifest_entrypoints = {
        recogniser["entry_point"].removeprefix("quiddity.")
        for family in public.capability_manifest()["families"]
        for recogniser in family["recognisers"]
    }
    assert manifest_entrypoints == MIGRATED


def _record_types(annotation: object) -> set[type[Record]]:
    if inspect.isclass(annotation) and issubclass(typing.cast(type, annotation), Record):
        return {typing.cast(type[Record], annotation)}
    origin = typing.get_origin(annotation)
    if origin in {tuple, list, typing.Union, types.UnionType}:
        return set().union(*(_record_types(item) for item in typing.get_args(annotation)), set())
    return set()


def test_registry_record_types_match_public_entrypoints_and_result_fields() -> None:
    from tools._legacy_recognition import namespace

    detector_api = namespace()
    result_hints = typing.get_type_hints(result_module._LegacyRecognitionResult)
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS):
        declared = set(definition.record_types)
        entrypoint = getattr(detector_api, definition.public_entrypoint)
        public_return = typing.get_type_hints(entrypoint)["return"]
        assert declared == _record_types(public_return), definition.public_entrypoint
        assert declared == _record_types(result_hints[definition.result_field]), (
            definition.result_field
        )


def test_registry_rejects_runtime_output_outside_the_record_contract() -> None:
    holes = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.HOLES)
    with pytest.raises(TypeError, match="undeclared record type"):
        validate_output(holes, [object()])


def test_registry_census_dispositions_cover_the_existing_manual_keys() -> None:
    counted = {
        definition.result_field: definition.census.key
        for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
        if isinstance(definition.census, Counted)
    }
    assert counted == {source: key for key, source in CENSUS_BINDINGS}
    assert tuple(key for key, _source in CENSUS_BINDINGS) == CENSUS_KEYS

    swapped = tuple(
        replace(definition, census=Counted("boss"))
        if definition.family is FamilyId.HOLES
        else replace(definition, census=Counted("hole"))
        if definition.family is FamilyId.BOSSES
        else definition
        for definition in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="census bindings"):
        validate_census_contract(
            {source: key for key, source in CENSUS_BINDINGS}, swapped, DERIVED_DEFINITIONS
        )


def test_registry_applicability_is_context_only() -> None:
    for definition in PHYSICAL_DEFINITIONS:
        assert tuple(signature(definition.applicable).parameters) == ("context",)
        assert tuple(signature(definition.projected).parameters) == ("context",)
    assert {
        definition.family: definition.projected
        for definition in PHYSICAL_DEFINITIONS
        if definition.projected is not always
    } == {FamilyId.PASSAGES: prismatic}


def test_registry_applicability_is_pinned_per_family() -> None:
    """The eight families that only run on a prismatic part, named.

    `projected` was pinned above; `applicable` was not, though it decides the larger thing --
    whether the family runs at all. Moving one was not unnoticed before this, but it was never
    *named*: measured on three families, `test_golden_parity.py` fails on between one and
    thirty pinned snapshots, and `angled_steps` additionally trips the gating case in
    `test_recognition_explanations.py` that happens to use it as its example. Neither says a
    family's applicability changed, and regenerating goldens is an ordinary enough action to
    absorb a deliberate one without review.

    A separate test from the `projected` pin above, so that a regression in one does not
    short-circuit before the other is evaluated.

    The eight are written out rather than read off the registry: widening the set has to be a
    visible edit to this list, not something the code can grant itself.
    """

    assert {
        definition.family: definition.applicable
        for definition in PHYSICAL_DEFINITIONS
        if definition.applicable is not always
    } == {
        FamilyId.RECTANGULAR_BLIND_SLOTS: prismatic,
        FamilyId.ROUND_BOTTOM_BLIND_SLOTS: prismatic,
        FamilyId.ANGLED_STEPS: prismatic,
        FamilyId.PAIRED_RAMP_STEPS: prismatic,
        FamilyId.GUSSET_RIBS: prismatic,
        FamilyId.THROUGH_STEPS: prismatic,
        FamilyId.CIRCULAR_BLIND_STEPS: prismatic,
        FamilyId.ORIENTED_SLOTS: prismatic,
    }


def test_registry_validation_rejects_duplicate_missing_and_late_dependencies() -> None:
    with pytest.raises(ValueError, match="cover every non-legacy family"):
        validate_definitions(PHYSICAL_DEFINITIONS[:-1], DERIVED_DEFINITIONS)
    duplicate = (*PHYSICAL_DEFINITIONS[:-1], PHYSICAL_DEFINITIONS[0])
    with pytest.raises(ValueError, match="cover every non-legacy family"):
        validate_definitions(duplicate, DERIVED_DEFINITIONS)
    holes = next(item for item in PHYSICAL_DEFINITIONS if item.family is FamilyId.HOLES)
    invalid = tuple(
        replace(item, dependencies=(FamilyId.PLATES,)) if item is holes else item
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="dependencies must exist before"):
        validate_definitions(invalid, DERIVED_DEFINITIONS)
    duplicate_census = tuple(
        replace(item, census=Counted("hole")) if item.family is FamilyId.DOUBLE_D_BORES else item
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="census keys must be non-empty and unique"):
        validate_definitions(duplicate_census, DERIVED_DEFINITIONS)
    unreviewed_applicability = tuple(
        replace(item, applicable=lambda context: True) if item.family is FamilyId.BOSSES else item
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="reviewed neutral predicate"):
        validate_definitions(unreviewed_applicability, DERIVED_DEFINITIONS)
    unreviewed_projection = tuple(
        replace(item, projected=lambda context: True) if item.family is FamilyId.BOSSES else item
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="projection must use a reviewed neutral predicate"):
        validate_definitions(unreviewed_projection, DERIVED_DEFINITIONS)


def test_registry_validation_rejects_incomplete_physical_contract_metadata() -> None:
    first = PHYSICAL_DEFINITIONS[0]
    second = PHYSICAL_DEFINITIONS[1]

    duplicate_field = tuple(
        replace(item, result_field=first.result_field) if item is second else item
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="physical result fields must be unique"):
        validate_definitions(duplicate_field, DERIVED_DEFINITIONS)

    missing_record_contract = tuple(
        replace(item, record_types=()) if item is first else item for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="record and public contracts"):
        validate_definitions(missing_record_contract, DERIVED_DEFINITIONS)

    missing_census = tuple(
        replace(item, census=None) if item is first else item  # type: ignore[arg-type]
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="explicit census disposition"):
        validate_definitions(missing_census, DERIVED_DEFINITIONS)

    empty_reason = tuple(
        replace(item, census=NotCounted("")) if item is first else item
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="reasons must be non-empty"):
        validate_definitions(empty_reason, DERIVED_DEFINITIONS)

    missing_attribution = tuple(
        replace(item, attribution=None) if item is first else item  # type: ignore[arg-type]
        for item in PHYSICAL_DEFINITIONS
    )
    with pytest.raises(ValueError, match="attribution disposition"):
        validate_definitions(missing_attribution, DERIVED_DEFINITIONS)


def test_registry_validation_rejects_incomplete_derived_contract_metadata() -> None:
    """The derived checks, on the one derived roster there now is."""

    first = DERIVED_DEFINITIONS[0]

    with pytest.raises(ValueError, match="cover every derived id"):
        validate_definitions(PHYSICAL_DEFINITIONS, DERIVED_DEFINITIONS[:-1])

    overlapping_field = (
        replace(first, result_field=PHYSICAL_DEFINITIONS[0].result_field),
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="registry result fields must be unique"):
        validate_definitions(PHYSICAL_DEFINITIONS, overlapping_field)

    missing_entrypoint = (replace(first, public_entrypoint=""), *DERIVED_DEFINITIONS[1:])
    with pytest.raises(ValueError, match="derived definitions require a public entrypoint"):
        validate_definitions(PHYSICAL_DEFINITIONS, missing_entrypoint)

    missing_census = (
        replace(first, census=None),  # type: ignore[arg-type]
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="explicit census disposition"):
        validate_definitions(PHYSICAL_DEFINITIONS, missing_census)

    empty_reason = (replace(first, census=NotCounted("")), *DERIVED_DEFINITIONS[1:])
    with pytest.raises(ValueError, match="reasons must be non-empty"):
        validate_definitions(PHYSICAL_DEFINITIONS, empty_reason)

    missing_record_types = (replace(first, record_types=()), *DERIVED_DEFINITIONS[1:])
    with pytest.raises(ValueError, match="record contracts"):
        validate_definitions(PHYSICAL_DEFINITIONS, missing_record_types)

    invalid_source = (
        replace(first, sources=(FamilyId.LEGACY,)),
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="sources must be registered"):
        validate_definitions(PHYSICAL_DEFINITIONS, invalid_source)

    # `_project_result` copies a derived field across without asking whether its sources project
    # into this context, which is only safe while they always do. Every source family is
    # unconditionally projected today, so the registry says so rather than leaving the reader to
    # check 31 definitions -- and a derived family built on a conditionally projected one is
    # refused here, where the gate that used to handle it has to be reinstated deliberately.
    gated_family = DERIVED_DEFINITIONS[0].sources[0]
    gated = next(
        index
        for index, definition in enumerate(PHYSICAL_DEFINITIONS)
        if definition.family is gated_family
    )
    conditional_source = (
        *PHYSICAL_DEFINITIONS[:gated],
        replace(PHYSICAL_DEFINITIONS[gated], projected=prismatic),
        *PHYSICAL_DEFINITIONS[gated + 1 :],
    )
    with pytest.raises(ValueError, match="unconditionally projected"):
        validate_definitions(conditional_source, DERIVED_DEFINITIONS)


def test_registry_result_field_validation_rejects_stale_contract() -> None:
    fields_without_one = frozenset(
        item.result_field for item in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
    ) - {"holes"}
    with pytest.raises(ValueError, match="do not exactly cover"):
        validate_result_fields(fields_without_one)


def test_every_family_says_something_different_about_what_it_claims() -> None:
    """No two families share an attribution sentence or a `NotCounted` reason.

    These strings are prose, so nothing else in the suite reads them: a declaration that copied a
    neighbour's would pass every gate. They moved out of one file and into thirty-odd during the
    declared-family migration, which is exactly when a copy-paste between siblings stops being
    visible in one diff.
    """

    claims = [
        (item.family.name, item.attribution.proof_contract)
        for item in PHYSICAL_DEFINITIONS
        if isinstance(item.attribution, FullyAttributed)
    ]
    duplicated = {
        contract for _family, contract in claims if [c for _f, c in claims].count(contract) > 1
    }
    assert duplicated == set()

    reasons = [
        (item.family.name, item.census.reason)
        for item in PHYSICAL_DEFINITIONS
        if isinstance(item.census, NotCounted)
    ]
    shared = {reason for _family, reason in reasons if [r for _f, r in reasons].count(reason) > 1}
    # Two reasons are genuinely shared by groups of families -- the recess-projected ones, and
    # those whose records are counted under another family's key. Naming them here means a new
    # family cannot join either group silently.
    assert shared == {
        "Counted once through the unified section_recess projection",
        "not a distinct census key",
    }


#: Declared families whose records are deliberately defined next door, and why. Asserted exactly
#: and with the reasons required to be non-empty, because the default is that a family owns them.
_RECESS_RECORDS_REASON = (
    "`_recess_records` sits below the recess machinery that uses it. Eight modules import it at "
    "run time and none imports a family module; `_recess_core`, `_recess_obround` and "
    "`_recess_reduce` construct these records, the rest annotate or test against them. Moving "
    "them into `slots.py` closes the cycle slots -> _recess_features -> _recess_core -> slots."
)

#: A shorter route to the same problem, and worth its own reason: of the machinery modules only
#: `_recess_patterns` touches the pattern records, so the cycle above is not the one that bites.
_RECESS_PATTERNS_REASON = (
    "the pattern records are read by `_recess_patterns`, which `slots.py` imports, so moving them "
    "into `slots.py` closes the cycle slots -> _recess_patterns -> slots."
)

_SECTION_RECESS_REASON = (
    "`SectionRecess` is one of seventeen profile, end, geometry and projection types in "
    "`_section_recess` that are only comprehensible together: moving it alone breaks up the "
    "cluster, and moving all seventeen makes the family module about seven times its size."
)

RECORDS_DEFINED_NEXT_DOOR: dict[str, str] = {
    "SLOTS": _RECESS_RECORDS_REASON,
    "POCKETS": _RECESS_RECORDS_REASON,
    "CHANNELS": _RECESS_RECORDS_REASON,
    "SLOT_PATTERNS": _RECESS_PATTERNS_REASON,
    "POCKET_PATTERNS": _RECESS_PATTERNS_REASON,
    "SECTION_RECESSES": _SECTION_RECESS_REASON,
}


#: Declared families whose public entry point is defined somewhere other than the module that
#: declares them, and why. Every other family names its entry point by reference, which makes the
#: two the same module by construction; these name it as a string, so nothing else would notice.
ENTRYPOINT_DEFINED_NEXT_DOOR: dict[str, str] = {
    "SECTION_RECESSES": (
        "`recognise_section_recesses` runs the orchestrator and filters its output, so it is a "
        "view rather than a recogniser and lives in `result.py`. Importing it here would put "
        "`result` back in this module's chain and close the cycle through `_registry`."
    )
}


def test_a_declared_family_defines_its_own_entry_point() -> None:
    """The declaration and the public surface are one module, unless listed above with a reason.

    True by construction wherever `public_entrypoint` is `recognise_x.__name__`, since the name
    has to be imported to be read. A string entry point breaks that, and nothing else checks it:
    every other consumer of `public_entrypoint` resolves it against the package namespace.
    """

    elsewhere = set()
    checked = 0
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS):
        if not _is_declared_in_its_module(definition) or definition.public_entrypoint is None:
            continue
        checked += 1
        module = _declaring_module(definition)
        assert module is not None
        if not hasattr(module, definition.public_entrypoint):
            family = getattr(definition, "family", None) or definition.identifier
            elsewhere.add(family.name)

    assert checked == 38
    # The migration is finished, with no exception left: every definition is written in the
    # module of the family it describes. The last hold-out was the PASSAGES_COMPAT projection,
    # deleted with the rest of the legacy passage surface rather than relocated.
    assert [
        (getattr(definition, "family", None) or definition.identifier).name
        for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
        if not _is_declared_in_its_module(definition)
    ] == []
    assert all(ENTRYPOINT_DEFINED_NEXT_DOOR.values()), "an exception needs a reason, not just a key"
    assert elsewhere == set(ENTRYPOINT_DEFINED_NEXT_DOOR)


def _is_declared_in_its_module(definition: object) -> bool:
    """Whether the family describes itself, rather than being a literal in `_registry`.

    A declaration's discoverer is written in the family module; a registry literal's is an
    adapter defined in `_registry` itself. This lived in the manifest generator while that tool
    still had a hand-written evidence table to fall back to for the families it was false for.
    The table is gone, so the tool no longer asks the question and these tests own it.
    """

    discover = getattr(definition, "discover", None) or getattr(definition, "derive", None)
    return getattr(discover, "__module__", "") != "quiddity._registry"


def _classes_defined_in(path: Path) -> set[str]:
    """The classes whose `class` statement is in *path*, read from the source."""

    return {
        node.name
        for node in ast.parse(path.read_text(encoding="utf-8")).body
        if isinstance(node, ast.ClassDef)
    }


def _defining_file(record: type) -> str:
    """Where *record*'s `class` statement really is, for the failure message.

    `inspect.getfile` would name the module that re-exported it, which is the very confusion this
    test exists to catch; reporting it would send a reader to the wrong file.
    """

    for path in sorted((ROOT / "src" / "quiddity").glob("*.py")):
        if record.__name__ not in _classes_defined_in(path):
            continue
        module = sys.modules.get(f"quiddity.{path.stem}")
        if module is not None and getattr(module, record.__name__, None) is record:
            return path.name
    return "(not found in the package)"


def _declaring_module(definition: object) -> ModuleType | None:
    """The package module holding *definition* as a module-level name, if any."""

    for module in list(sys.modules.values()):
        name = getattr(module, "__name__", "")
        if not name.startswith("quiddity.") or getattr(module, "__file__", None) is None:
            continue
        if any(value is definition for value in vars(module).values()):
            return module
    return None


def test_a_declared_family_defines_its_own_record_types() -> None:
    """A family module owns its records, unless it is named above with a reason.

    True of every declared family since the migration began, but only by habit -- nothing said so,
    so nothing would have noticed the first to drift.

    Two things have to be read rather than asked for. **Where** a class is defined: `__module__` is
    not it, and neither is `inspect.getsourcefile`, which for a class is only `__module__` resolved
    to a filename. Six modules here rewrite `__module__` on names they publish elsewhere, and
    most of the record types they publish sit behind one: `Slot` reports `quiddity.slots` while
    its `class` statement is in `_recess_records.py`. And
    **which** class it is: matching on `__name__` alone would let a foreign record in under a local
    name, so the name found in the source must also resolve back to this very record.

    The module measured is the one holding the declaration, not the one holding the discoverer.
    Those are the same module today, and the rule is about the former: moving a `def` next to the
    records it wants would otherwise sidestep the rule rather than take the exception.
    """

    elsewhere: dict[str, dict[str, str]] = {}
    declared = 0
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS):
        if not _is_declared_in_its_module(definition):
            continue  # still described by a registry literal
        declared += 1
        module = _declaring_module(definition)
        assert module is not None, f"{definition} is declared but bound to no module-level name"
        defined_here = _classes_defined_in(Path(module.__file__))
        strays = {
            record.__name__: _defining_file(record)
            for record in definition.record_types
            if record.__name__ not in defined_here
            or getattr(module, record.__name__, None) is not record
        }
        if strays:
            family = getattr(definition, "family", None) or definition.identifier
            elsewhere[family.name] = strays

    # Guards the sweep itself: a predicate that stopped matching would otherwise pass vacuously.
    assert declared == 38

    assert all(RECORDS_DEFINED_NEXT_DOOR.values()), "an exception needs a reason, not just a key"
    unexplained = {
        family: strays
        for family, strays in elsewhere.items()
        if family not in RECORDS_DEFINED_NEXT_DOOR
    }
    assert unexplained == {}
    assert sorted(set(RECORDS_DEFINED_NEXT_DOOR) - set(elsewhere)) == []
