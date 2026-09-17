# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle


import ast
import builtins
import inspect
import json
import math
import sys
import types
from dataclasses import FrozenInstanceError, replace

import pytest
from build123d import Align, Box, Cylinder, Pos

from quiddity import (
    STEP_LADDER_BOUNDARY_MARGIN,
    CounterSink,
    FaceLevel,
    HoleRecord,
    RecognitionResult,
    Slot,
    TurnedStep,
    build_recognition_result,
)
from quiddity._candidates import FamilyId
from quiddity._registry import DERIVED_DEFINITIONS, PHYSICAL_DEFINITIONS
from tools._legacy_recognition import (
    Passage,
    Pocket,
)


def _plate_with_holes():
    plate = Box(60, 40, 8)
    return plate - Pos(-15, 0, 0) * Cylinder(3, 8) - Pos(15, 0, 0) * Cylinder(3, 8)


def test_recognition_result_is_frozen_and_owns_tuple_inventories():
    result = build_recognition_result(_plate_with_holes())

    assert isinstance(result, RecognitionResult)
    assert len(result.holes) == 2
    assert all(
        isinstance(value, tuple)
        for value in result.__dict__.values()
        if not isinstance(value, bool)
    )
    with pytest.raises(FrozenInstanceError):
        result.holes = ()


def test_projection_rejects_a_record_from_the_wrong_family_contract():
    import quiddity.result as result_module

    class WrongInventory:
        def records(self, family):
            del family
            return (object(),)

    with pytest.raises(TypeError, match="inventory has the wrong record type"):
        result_module._records(  # type: ignore[arg-type]
            WrongInventory(), FamilyId.HOLES, HoleRecord
        )


def _registry_discovery_targets() -> set[tuple[str, str]]:
    """Every (module, name) the orchestrator reaches discovery through, read from the registry.

    Each definition's `discover`/`derive` is a small adapter in the family's own module; the
    functions it calls are what a run actually executes. Deriving the roster this way is what
    stops a family being silently absent from the injection test below -- #624 found `gussets`
    missing, and seven more were missing when this was written.

    Bare-name calls only, and only those bound to a function in that module: `monkeypatch`
    rebinds a module attribute, so a call through `services.x` or a method is not interceptable
    there and is not a patch target.
    """

    targets: set[tuple[str, str]] = set()
    for definition in (*PHYSICAL_DEFINITIONS, *DERIVED_DEFINITIONS):
        adapter = getattr(definition, "discover", None) or definition.derive
        module = sys.modules[adapter.__module__]
        tree = ast.parse(inspect.getsource(module))
        node = next(
            child
            for child in tree.body
            if isinstance(child, ast.FunctionDef) and child.name == adapter.__name__
        )
        for call in ast.walk(node):
            if not isinstance(call, ast.Call) or not isinstance(call.func, ast.Name):
                continue
            name = call.func.id
            if hasattr(builtins, name):
                continue
            if isinstance(getattr(module, name, None), types.FunctionType):
                targets.add((adapter.__module__, name))
    return targets


def test_orchestrator_injects_each_shared_dependency_once(monkeypatch):
    import quiddity._run as run_module
    import quiddity.angled_steps as angled_steps_module
    import quiddity.blends as blends_module
    import quiddity.bosses as bosses_module
    import quiddity.chamfers as chamfers_module
    import quiddity.circular_blind_steps as circular_blind_steps_module
    import quiddity.countersinks as countersinks_module
    import quiddity.edge_open_circular_recesses as edge_open_circular_recesses_module
    import quiddity.edge_open_prismatic_recesses as edge_open_prismatic_recesses_module
    import quiddity.fillets as fillets_module
    import quiddity.flats as flats_module
    import quiddity.grooves as grooves_module
    import quiddity.gussets as gussets_module
    import quiddity.holes as holes_module
    import quiddity.levels as levels_module
    import quiddity.oriented_slots as oriented_slots_module
    import quiddity.pads as pads_module
    import quiddity.paired_ramp_steps as paired_ramp_steps_module
    import quiddity.passages as passages_module
    import quiddity.plates as plates_module
    import quiddity.polygonal_bosses as polygonal_bosses_module
    import quiddity.prismatic_pockets as prismatic_pockets_module
    import quiddity.profiled_bores as profiled_bores_module
    import quiddity.rectangular_blind_slots as rectangular_blind_slots_module
    import quiddity.repeating_profiles as repeating_profiles_module
    import quiddity.result as result_module
    import quiddity.round_bottom_slots as round_bottom_slots_module
    import quiddity.section_recesses as section_recesses_module
    import quiddity.slots as slots_module
    import quiddity.through_steps as through_steps_module
    import quiddity.turned as turned_module
    from quiddity._candidates import EvidenceIndex

    calls: dict[str, int] = {}
    # Every rebinding this test makes, so the roster check at the end reads what was actually
    # patched rather than a second hand-written list that could drift from the first.
    patched: set[tuple[str, str]] = set()

    def patch(module, name, replacement):
        patched.add((module.__name__, name))
        monkeypatch.setattr(module, name, replacement)

    cylinders = ([{"axis": "z"}], [{"axis": "x"}])
    # Fully-attributed Countersinks cannot be fabricated without original cone evidence in this
    # dependency-injection test; the dedicated lifecycle matrix owns nonempty attribution.
    countersinks: list[CounterSink] = []
    # Fully-attributed Holes cannot be fabricated without original cylindrical evidence in this
    # dependency-injection test; the dedicated lifecycle matrix owns nonempty attribution.
    holes: list[HoleRecord] = []
    # Fully-attributed Slots cannot be fabricated without original wall/cap evidence here.
    slots: list[Slot] = []
    # Fully-attributed Pockets likewise require original wall/cap evidence.
    pockets: list[Pocket] = []
    # Fully-attributed families cannot be faked with output records but no original-face
    # evidence. This orchestration test owns dependency injection, so keep that separate
    # contract represented by an empty (but still invoked and bound) Passage family.
    passages: list[Passage] = []

    def same_records(actual, expected):
        return len(actual) == len(expected) and all(
            left is right for left, right in zip(actual, expected, strict=True)
        )

    def counted(name, returns):
        def fake(part, **kwargs):
            calls[name] = calls.get(name, 0) + 1
            return returns

        return fake

    def cyl_consumer(name, returns):
        def fake(part, *, cyls=None, **kwargs):
            calls[name] = calls.get(name, 0) + 1
            assert cyls == cylinders and cyls is not cylinders
            return returns

        return fake

    def derived(name, source, returns):
        def fake(records):
            calls[name] = calls.get(name, 0) + 1
            assert same_records(records, source)
            return returns

        return fake

    def fake_cylinders(part, *, face_surfaces=None):
        assert face_surfaces is not None
        calls["cylinders"] = calls.get("cylinders", 0) + 1
        return cylinders

    def fake_holes(part, *, cyls=None, csinks=None, **kwargs):
        calls["holes"] = calls.get("holes", 0) + 1
        assert cyls == cylinders and cyls is not cylinders
        assert same_records(csinks, countersinks)
        return holes

    # Patched where the run derives it, not where the aggregate used to. The cylinder scan is
    # one of the facts `RecognitionRun` owns, so `_run` is the only place that asks for it.
    patch(run_module, "analyse_cylinders", fake_cylinders)
    patch(countersinks_module, "_discover_countersinks", counted("countersinks", countersinks))
    patch(holes_module, "_discover_holes", fake_holes)
    patch(
        profiled_bores_module,
        "_discover_double_d_bores",
        counted("double_d_bores", []),
    )
    patch(holes_module, "recognise_hole_patterns", derived("patterns", holes, []))
    patch(bosses_module, "_discover_bosses", cyl_consumer("bosses", []))
    patch(polygonal_bosses_module, "_discover_polygonal_bosses", counted("polygonal_bosses", []))
    patch(polygonal_bosses_module, "_discover_polygonal_stock", counted("polygonal_stock", []))
    patch(slots_module, "_discover_channels", counted("channels", []))
    patch(slots_module, "_discover_slots", counted("slots", slots))
    patch(slots_module, "recognise_slot_patterns", derived("slot_patterns", slots, []))
    patch(
        oriented_slots_module,
        "recognise_oriented_slot_patterns",
        derived("oriented_slot_patterns", [], []),
    )
    patch(grooves_module, "recognise_grooves", cyl_consumer("grooves", []))
    patch(flats_module, "_discover_flats", cyl_consumer("flats", []))
    patch(slots_module, "_discover_pockets", counted("pockets", pockets))
    patch(passages_module, "recognise_section_passages", counted("passages", passages))
    patch(slots_module, "recognise_pocket_patterns", derived("pocket_patterns", pockets, []))
    patch(pads_module, "_discover_rectangular_pads", counted("pads", []))
    patch(
        repeating_profiles_module,
        "_discover_repeating_radial_profiles",
        counted("radial_profiles", []),
    )
    patch(turned_module, "recognise_turned_steps", cyl_consumer("turned_steps", []))
    # Fully-attributed FaceLevels cannot be fabricated without original horizontal-face evidence.
    # This test owns dependency injection, so keep the family empty but still invoked and bound.
    levels: list[FaceLevel] = []
    patch(levels_module, "_discover_step_levels", counted("step_levels", levels))
    patch(levels_module, "_discover_risers", counted("risers", []))
    patch(chamfers_module, "recognise_chamfers", counted("chamfers", []))
    patch(angled_steps_module, "recognise_angled_steps", counted("angled_steps", []))
    patch(
        paired_ramp_steps_module,
        "recognise_paired_ramp_steps",
        counted("paired_ramp_steps", []),
    )
    patch(
        through_steps_module,
        "recognise_through_steps",
        counted("through_steps", []),
    )
    patch(
        circular_blind_steps_module,
        "_discover_circular_blind_steps",
        counted("circular_blind_steps", []),
    )
    patch(
        rectangular_blind_slots_module,
        "recognise_rectangular_blind_slots",
        counted("rectangular_blind_slots", []),
    )
    patch(
        round_bottom_slots_module,
        "recognise_round_bottom_blind_slots",
        counted("round_bottom_blind_slots", []),
    )

    # All five recess families are proposed before one reconciler decides among them. Passage
    # discovery is a counted family call of its own; the reconciler receives completed records
    # and the point-in-time read capability rather than a Part or mutable ledger.
    def fake_recesses(
        found_slots,
        found_pockets,
        prismatic,
        found_passages,
        evidence,
        *,
        rectangular_blind_slots,
        edge_open_circular_pockets,
    ):
        calls["reconcile_recesses"] = calls.get("reconcile_recesses", 0) + 1
        assert same_records(
            [candidate.record for candidate in found_slots.candidates], slots
        ) and same_records([candidate.record for candidate in found_pockets.candidates], pockets)
        assert same_records([candidate.record for candidate in found_passages.candidates], passages)
        assert rectangular_blind_slots.candidates == ()
        assert edge_open_circular_pockets.candidates == ()
        assert isinstance(evidence, EvidenceIndex)
        return ()

    def fake_policy(name):
        def reconcile(*args):
            calls[name] = calls.get(name, 0) + 1
            assert isinstance(args[-1], EvidenceIndex)
            return ()

        return reconcile

    patch(result_module, "reconcile_recess_candidates", fake_recesses)
    patch(result_module, "reconcile_bevel_candidates", fake_policy("reconcile_bevels"))
    patch(
        result_module,
        "reconcile_circular_step_fillets",
        fake_policy("reconcile_circular_step_fillets"),
    )
    patch(
        result_module,
        "reconcile_profiled_bore_candidates",
        fake_policy("reconcile_profiled_bores"),
    )
    patch(
        result_module,
        "reconcile_step_groove_candidates",
        fake_policy("reconcile_step_grooves"),
    )

    def fake_diagnostics(reconciliation, evidence):
        calls["diagnose_residuals"] = calls.get("diagnose_residuals", 0) + 1
        assert isinstance(evidence, EvidenceIndex)
        # Every fabricated physical family is empty because fully-attributed records require real
        # topology. The lifecycle tests own nonempty dispositions; this test owns single injection.
        assert reconciliation.dispositions == ()
        return ()

    patch(result_module, "diagnose_residuals", fake_diagnostics)
    patch(fillets_module, "_discover_fillets", counted("fillets", []))
    # A declared family is patched where it lives; the registry no longer imports its core.
    patch(plates_module, "_discover_plates", counted("plates", []))
    patch(gussets_module, "_discover_gusset_ribs", counted("gusset_ribs", []))

    # Seven families reached real discovery until the roster check below was written: the four
    # recess families, the section-recess aggregate, oriented slots and gusset-rib patterns.
    # None of them failed, because an empty part yields empty records either way -- which is
    # exactly why nothing noticed. #624 found `gussets` in the same state.
    patch(
        prismatic_pockets_module,
        "recognise_prismatic_pockets",
        counted("prismatic_pockets", []),
    )
    patch(
        edge_open_circular_recesses_module,
        "recognise_edge_open_circular_pockets",
        counted("edge_open_circular_pockets", []),
    )
    patch(
        edge_open_prismatic_recesses_module,
        "recognise_edge_open_prismatic_recesses",
        counted("edge_open_prismatic_recesses", []),
    )
    patch(blends_module, "_discover_blends", counted("blends", []))
    patch(
        gussets_module,
        "recognise_gusset_rib_patterns",
        derived("gusset_rib_patterns", [], []),
    )

    # These two take no `part`, so they cannot use the shared fakes above.
    def fake_section_recesses(*, writer, surfaces):
        calls["section_recesses"] = calls.get("section_recesses", 0) + 1
        assert writer is not None and surfaces is not None
        return []

    patch(section_recesses_module, "discover_section_recesses", fake_section_recesses)

    def fake_body_keys(graph, solids):
        calls["oriented_slot_body_keys"] = calls.get("oriented_slot_body_keys", 0) + 1
        assert solids == ()
        return {}

    patch(oriented_slots_module, "_body_keys", fake_body_keys)

    def fake_project(source, body_key):
        # Unreachable here -- `passages` is empty, so the loop this sits in never runs -- but it
        # is still a route the registry says discovery goes through, so it is intercepted rather
        # than excused. An exception roster would need maintaining as the empty families change.
        calls["oriented_slot_project"] = calls.get("oriented_slot_project", 0) + 1
        return None

    patch(oriented_slots_module, "_project", fake_project)

    # A part rather than a bare object: the orchestrator now builds one face graph for the
    # families that record which faces they were built from, and an empty inventory is all this
    # test needs -- every recogniser that would read it is replaced above.
    class _Part:
        def faces(self):
            return []

    # Every route the registry says a run reaches discovery through was intercepted. Without
    # this the test still passes with a family missing -- its real recogniser runs, returns
    # nothing on an empty part, and contributes no counter to compare. That is how `gussets`
    # (#624) and seven more went unnoticed.
    unintercepted = sorted(_registry_discovery_targets() - patched)
    assert unintercepted == [], f"reached real discovery: {unintercepted}"

    built = result_module.build_recognition_result(_Part())

    expected = {
        "angled_steps",
        "paired_ramp_steps",
        "through_steps",
        "circular_blind_steps",
        "rectangular_blind_slots",
        "round_bottom_blind_slots",
        "passages",
        "reconcile_recesses",
        "reconcile_bevels",
        "reconcile_circular_step_fillets",
        "reconcile_profiled_bores",
        "reconcile_step_grooves",
        "diagnose_residuals",
        "cylinders",
        "countersinks",
        "holes",
        "double_d_bores",
        "patterns",
        "bosses",
        "polygonal_bosses",
        "polygonal_stock",
        "channels",
        "slots",
        "slot_patterns",
        "oriented_slot_patterns",
        "grooves",
        "flats",
        "pockets",
        "pocket_patterns",
        "pads",
        "radial_profiles",
        "turned_steps",
        "step_levels",
        "risers",
        "chamfers",
        "fillets",
        "plates",
        "gusset_ribs",
        "prismatic_pockets",
        "edge_open_circular_pockets",
        "edge_open_prismatic_recesses",
        "blends",
        "gusset_rib_patterns",
        "section_recesses",
        "oriented_slot_body_keys",
    }
    assert set(calls) == expected
    assert set(calls.values()) == {1}
    assert built.holes == tuple(holes)
    assert built.step_levels == tuple(levels)
    assert built.step_ladder_for_z_span(0.0, 10.0) == []


def test_supplied_cylinder_inventory_is_not_rediscovered(monkeypatch):
    import quiddity._run as run_module
    import quiddity.result as result_module

    cylinders = ([], [])

    def forbidden(part):
        raise AssertionError("supplied cylinder substrate was rediscovered")

    monkeypatch.setattr(run_module, "analyse_cylinders", forbidden)
    result = result_module.build_recognition_result(Box(10, 10, 10), cylinders=cylinders)
    assert result.cylinders == ((), ())


def test_aggregate_inventory_has_one_named_candidate_per_physical_output() -> None:
    import quiddity.result as result_module
    from quiddity._candidates import FamilyId

    product = result_module._take_inventory(Box(10, 10, 10))

    assert FamilyId.LEGACY not in result_module.PHYSICAL_FAMILIES
    for family in result_module.PHYSICAL_FAMILIES:
        candidate_set = product.physical.candidate_set(family)
        assert candidate_set.family is family
        assert tuple(candidate.record for candidate in candidate_set.candidates) == tuple(
            product.physical.records(family)
        )
        assert all(candidate.family is family for candidate in candidate_set.candidates)
        assert (
            product.evidence.candidate_set_for(family, product.physical.records(family)).candidates
            == candidate_set.candidates
        )


def test_physical_roster_matches_every_nonlegacy_family_and_result_field() -> None:
    """`result.py` refuses these at import; this names the four fields no definition owns."""

    import quiddity.result as result_module
    from quiddity._candidates import FamilyId

    assert len(result_module.PHYSICAL_FAMILIES) == len(set(result_module.PHYSICAL_FAMILIES))
    assert set(result_module.PHYSICAL_FAMILIES) == set(FamilyId) - {FamilyId.LEGACY}
    nonphysical = {
        "cylinders",
        "section_recess_patterns",
        "section_recess_refusals",
        "rotational",
    } | {
        definition.result_field
        for definition in (
            *result_module.DERIVED_DEFINITIONS,
            *result_module.PROJECTION_DEFINITIONS,
        )
    }
    assert {definition.result_field for definition in result_module.PHYSICAL_DEFINITIONS} == (
        set(result_module._LegacyRecognitionResult.__dataclass_fields__) - nonphysical
    )


def test_context_copies_caller_owned_cylinder_lists() -> None:
    import quiddity._run as run_module

    supplied = ([{"axis": "z"}], [{"axis": "x"}])
    context = run_module.start(Box(10, 10, 10), supplied)
    supplied[0].clear()
    supplied[1].append({"axis": "y"})

    assert tuple(item["axis"] for item in context.cylinders[0]) == ("z",)
    assert tuple(item["axis"] for item in context.cylinders[1]) == ("x",)


def _ladder_result(
    *, steps: tuple[TurnedStep, ...] = (), levels: tuple[FaceLevel, ...] = ()
) -> RecognitionResult:
    return replace(
        build_recognition_result(Box(10, 10, 10)),
        turned_steps=steps,
        step_levels=levels,
    )


def test_explicit_z_span_filters_only_interior_z_shoulders_at_the_named_margin() -> None:
    steps = tuple(
        TurnedStep("z", lo, hi, diameter)
        for lo, hi, diameter in (
            (0.0, 0.6, 10.0),
            (0.6, 1.2, 8.0),
            (1.2, 9.4, 12.0),
            (9.4, 10.0, 10.0),
        )
    )
    result = _ladder_result(steps=steps)

    assert STEP_LADDER_BOUNDARY_MARGIN == 0.6
    first = result.step_ladder_for_z_span(0.0, 10.0)
    second = result.step_ladder_for_z_span(0.0, 10.0)

    assert first == second == [1.2]
    assert all(type(value) is float for value in first)
    assert json.loads(json.dumps(first)) == [1.2]


def test_explicit_z_span_validates_bounds_margin_and_narrow_span_edges() -> None:
    result = _ladder_result(
        steps=(
            TurnedStep("z", 0.0, 1.0, 10.0),
            TurnedStep("z", 1.0, 2.0, 8.0),
        )
    )

    assert result.step_ladder_for_z_span(0.0, 2.0, boundary_margin=1.0) == []
    with pytest.raises(ValueError, match="z_min must not exceed z_max"):
        result.step_ladder_for_z_span(2.0, 1.0)
    for value in (math.nan, math.inf, -math.inf):
        with pytest.raises(ValueError, match="finite"):
            result.step_ladder_for_z_span(value, 2.0)
    for margin in (-0.1, math.nan, math.inf):
        with pytest.raises(ValueError, match="boundary_margin"):
            result.step_ladder_for_z_span(0.0, 2.0, boundary_margin=margin)


def test_non_z_or_prismatic_ladder_preserves_pre_filtered_face_levels() -> None:
    levels = (FaceLevel(4.0), FaceLevel(9.0))
    prismatic = _ladder_result(levels=levels)
    x_turned = _ladder_result(
        steps=(
            TurnedStep("x", 0.0, 4.0, 10.0),
            TurnedStep("x", 4.0, 10.0, 8.0),
        ),
        levels=levels,
    )

    assert prismatic.step_ladder_for_z_span(0.0, 10.0) == [4.0, 9.0]
    assert x_turned.step_ladder_for_z_span(0.0, 10.0) == [4.0, 9.0]


def test_build123d_bounds_call_is_compatible_but_deprecated_until_1_0() -> None:
    result = _ladder_result(
        steps=(
            TurnedStep("z", 0.0, 4.0, 10.0),
            TurnedStep("z", 4.0, 10.0, 8.0),
        )
    )
    bounds = Box(10, 10, 10, align=Align.MIN).bounding_box()

    with pytest.warns(DeprecationWarning, match=r"0\.2\.1.*no earlier than 1\.0\.0"):
        legacy = result.step_ladder(bounds)

    assert legacy == result.step_ladder_for_z_span(bounds.min.Z, bounds.max.Z) == [4.0]
