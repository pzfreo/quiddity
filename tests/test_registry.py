# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

import ast
import inspect
import types
import typing
from dataclasses import fields, replace
from inspect import signature
from pathlib import Path

import pytest
from build123d import Box, BuildPart, BuildSketch, Mode, Pos, RegularPolygon, extrude

import quiddity as public
import quiddity._registry as registry_module
import quiddity.result as result_module
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._record import Record
from quiddity._registry import (
    DERIVED_DEFINITIONS,
    PHYSICAL_DEFINITIONS,
    AcceptedInputs,
    AcceptedProjectionInputs,
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


def test_registry_is_the_closed_ordered_internal_roster() -> None:
    assert len(PHYSICAL_DEFINITIONS) == 32
    assert len(DERIVED_DEFINITIONS) == 5
    assert tuple(item.family for item in PHYSICAL_DEFINITIONS) == PHYSICAL_FAMILIES
    assert set(PHYSICAL_FAMILIES) == set(FamilyId) - {FamilyId.LEGACY}
    assert tuple(item.identifier for item in DERIVED_DEFINITIONS) == tuple(DerivedId)
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
        DerivedId.PASSAGES_COMPAT: (FamilyId.PASSAGES,),
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


def test_passage_projection_inputs_revalidate_the_exact_accepted_roster() -> None:
    with BuildPart() as built:
        Box(30, 30, 10)
        with BuildSketch():
            RegularPolygon(5, 3)
        extrude(amount=20, both=True, mode=Mode.SUBTRACT)
    product = _take_inventory(built.part)
    accepted = product.accepted.candidate_set(FamilyId.PASSAGES)
    inputs = registry_module._issue_projection_inputs(accepted, product.evidence)
    expected = inputs.passage_views()
    assert len(expected) == len(accepted.candidates) == 1

    object.__setattr__(inputs, "_candidates", ())
    with pytest.raises(ValueError, match="roster changed"):
        inputs.passage_views()
    object.__setattr__(inputs, "_candidates", accepted.candidates + accepted.candidates)
    with pytest.raises(ValueError, match="roster changed"):
        inputs.passage_views()
    object.__setattr__(inputs, "_candidates", accepted.candidates)
    assert inputs.passage_views() == expected

    original_candidates = accepted.candidates
    object.__setattr__(accepted, "candidates", ())
    object.__setattr__(inputs, "_candidates", accepted.candidates)
    with pytest.raises(ValueError, match="roster changed"):
        inputs.passage_views()
    object.__setattr__(accepted, "candidates", original_candidates)
    object.__setattr__(inputs, "_candidates", original_candidates)
    assert inputs.passage_views() == expected

    object.__setattr__(inputs, "_allowed", frozenset())
    with pytest.raises(ValueError, match="not a declared"):
        inputs.passage_views()
    object.__setattr__(inputs, "_allowed", frozenset((FamilyId.PASSAGES,)))
    assert inputs.passage_views() == expected

    with pytest.raises(TypeError):
        AcceptedProjectionInputs(  # type: ignore[call-arg]
            frozenset((FamilyId.PASSAGES,)), accepted, accepted.candidates, product.evidence
        )

    assert not hasattr(registry_module, "_PROJECTION_AUTHORITY_TOKEN")
    assert not hasattr(registry_module, "_ProjectionInputIssuer")
    assert not hasattr(inputs._issuer, "_issued")


def test_projection_input_authority_has_one_closed_production_caller() -> None:
    callers = []
    references = []
    for path in sorted(Path(registry_module.__file__).parent.glob("*.py")):
        source = path.read_text(encoding="utf-8")
        if "_issue_projection_inputs" in source:
            references.append(path.name)
        tree = ast.parse(source, filename=str(path))
        if any(
            isinstance(node, ast.Call)
            and (
                (isinstance(node.func, ast.Name) and node.func.id == "_issue_projection_inputs")
                or (
                    isinstance(node.func, ast.Attribute)
                    and node.func.attr == "_issue_projection_inputs"
                )
            )
            for node in ast.walk(tree)
        ):
            callers.append(path.name)
    assert callers == ["result.py"]
    assert references == ["_registry.py", "result.py"]

    registry_source = inspect.getsource(registry_module)
    assert "_PROJECTION_AUTHORITY_TOKEN" not in registry_source
    assert "class _ProjectionInputIssuer" not in registry_source


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
        if definition.public_entrypoint is not None:
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
    first = DERIVED_DEFINITIONS[0]

    with pytest.raises(ValueError, match="cover every derived id"):
        validate_definitions(PHYSICAL_DEFINITIONS, DERIVED_DEFINITIONS[:-1])

    overlapping_field = (
        replace(first, result_field=PHYSICAL_DEFINITIONS[0].result_field),
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="registry result fields must be unique"):
        validate_definitions(PHYSICAL_DEFINITIONS, overlapping_field)

    missing_record_contract = (
        replace(first, public_entrypoint=""),
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="discoverer definitions require a public entrypoint"):
        validate_definitions(PHYSICAL_DEFINITIONS, missing_record_contract)

    projection = DERIVED_DEFINITIONS[-1]
    invalid_projection_entrypoint = (
        *DERIVED_DEFINITIONS[:-1],
        replace(projection, public_entrypoint="recognise_passages"),
    )
    with pytest.raises(ValueError, match="projection definitions cannot declare"):
        validate_definitions(PHYSICAL_DEFINITIONS, invalid_projection_entrypoint)

    missing_census = (
        replace(first, census=None),  # type: ignore[arg-type]
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="explicit census disposition"):
        validate_definitions(PHYSICAL_DEFINITIONS, missing_census)

    empty_reason = (
        replace(first, census=NotCounted("")),
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="reasons must be non-empty"):
        validate_definitions(PHYSICAL_DEFINITIONS, empty_reason)

    missing_record_types = (
        replace(first, record_types=()),
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="record contracts"):
        validate_definitions(PHYSICAL_DEFINITIONS, missing_record_types)

    unknown_role = (
        replace(first, role="unknown"),  # type: ignore[arg-type]
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="role is not recognized"):
        validate_definitions(PHYSICAL_DEFINITIONS, unknown_role)

    invalid_source = (
        replace(first, sources=(FamilyId.LEGACY,)),
        *DERIVED_DEFINITIONS[1:],
    )
    with pytest.raises(ValueError, match="sources must be registered"):
        validate_definitions(PHYSICAL_DEFINITIONS, invalid_source)


def test_registry_result_field_validation_rejects_stale_contract() -> None:
    fields_without_one = frozenset(
        item.result_field for item in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS)
    ) - {"holes"}
    with pytest.raises(ValueError, match="do not exactly cover"):
        validate_result_fields(fields_without_one)
