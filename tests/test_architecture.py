# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

import ast
import importlib
import inspect
import textwrap
import typing
from pathlib import Path

import quiddity as recognition

ROOT = Path(__file__).parents[1]
PACKAGE = ROOT / "src" / "quiddity"

PUBLIC_MODULES = {
    "blends",
    "angled_steps",
    "capabilities",
    "census",
    "chamfers",
    "circular_blind_steps",
    "cli",
    "countersinks",
    "document",
    "edge_open_circular_recesses",
    "edge_open_prismatic_recesses",
    "evidence",
    "experimental_geometry",
    "explanations",
    "fillets",
    "flats",
    "frames",
    "grooves",
    "inspection",
    "levels",
    "oriented_slots",
    "pads",
    "paired_ramp_steps",
    "through_steps",
    "passages",
    "plates",
    "prismatic_pockets",
    "polygonal_bosses",
    "profiled_bores",
    "repeating_profiles",
    "rectangular_blind_slots",
    "round_bottom_slots",
    "section_recesses",
    "result",
    "slots",
    "step_io",
    "turned",
}


def test_correspondence_matcher_remains_private_and_result_neutral() -> None:
    assert not hasattr(recognition, "correspondence_changes")
    assert not hasattr(recognition, "CorrespondenceResult")
    result_source = (PACKAGE / "result.py").read_text()
    assert "_correspondence_match" not in result_source


MODULE_SEAM_EDGES = {
    "_corner_section": {"_adjacency", "_section_passages", "_sections"},
    "_open_channel_section": {"_adjacency", "_recess_records", "_section_passages", "_sections"},
    # Base layer: the kernel, the shared type aliases, and `_geometry`'s alignment threshold.
    "_body_identity": {"_typing"},
    "_analytic_surfaces": {"_geometry"},
    "_body_geometry": set(),
    "_adjacency": {"_analytic_surfaces", "_body_geometry", "_geometry", "_typing"},
    "edge_open_prismatic_recesses": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_geometry",
        "_record",
        "_rings",
        "_typing",
    },
    "edge_open_circular_recesses": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_geometry",
        "_record",
        "_rings",
        "_typing",
    },
    # Interpretation depends on geometric fact; the reverse edge is what keeps `FaceGraph`
    # immutable, so it must stay absent.
    # Three recognisers begin with the same two questions of a face. Naming the layer is
    # what lets this map have an opinion about it -- see the module docstring.
    "_bevel": {"_geometry", "_typing"},
    "paired_ramp_steps": {
        "_adjacency",
        "_bevel",
        "_candidates",
        "_claims",
        "_geometry",
        "_record",
        "_typing",
    },
    "through_steps": {
        "_adjacency",
        "_body_identity",
        "_candidates",
        "_claims",
        "_geometry",
        "_record",
        "_typing",
        "_volume_probe",
    },
    "circular_blind_steps": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_cylinder_substrate",
        "_effective_surfaces",
        "_geometry",
        "_record",
        "_typing",
    },
    "round_bottom_slots": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_geometry",
        "_record",
        "_typing",
    },
    "blends": {
        "_adjacency",
        "_analytic_surfaces",
        "_blend_view",
        "_candidates",
        "_claims",
        "_effective_surfaces",
        "_geometry",
        "_record",
        "_typing",
    },
    "rectangular_blind_slots": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_record",
        "_typing",
        "round_bottom_slots",
    },
    "_candidates": {"_adjacency", "_effective_surfaces", "_passage_compat"},
    "_correspondence": {
        "_adjacency",
        "_body_geometry",
        "_candidates",
        "_dispositions",
        "_run",
        "repeating_profiles",
    },
    # F6b is a private, optional consumer of issuer-validated F6a snapshots. It may use the
    # immutable descriptor grammar/tolerances, but must never reach recognition orchestration,
    # candidates, evidence, reconciliation, or public results.
    "_correspondence_partition": {"_body_geometry"},
    "_correspondence_match": {
        "_body_geometry",
        "_correspondence",
        "_correspondence_partition",
    },
    "_dispositions": {"_candidates"},
    "_diagnostics": {"_candidates", "_dispositions", "chamfers"},
    "_claims": {"_adjacency", "_candidates", "_effective_surfaces"},
    # One aggregate run constructs the graph-bound facade and shares its surface index.
    "_run": {
        "_adjacency",
        "_cylinder_substrate",
        "_effective_surfaces",
        "_typing",
        "experimental_geometry",
    },
    # Native cylinders retain the compatibility fast path; recovered cylinders consume the
    # restricted run-owned analytic/material-side query recorded by ADR 0004.
    "_cylinder_substrate": {
        "_adjacency",
        "_analytic_surfaces",
        "_effective_surfaces",
        "_geometry",
        "_typing",
    },
    "_hole_features": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_cylinder_substrate",
        "_effective_surfaces",
        "_geometry",
        "_record",
        "_typing",
        "countersinks",
        "edge_open_prismatic_recesses",
    },
    "_pattern_geometry": {"_geometry"},
    "profiled_bores": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_geometry",
        "_record",
        "_typing",
    },
    "pads": {
        "_analytic_surfaces",
        "_candidates",
        "_claims",
        "_effective_surfaces",
        "_geometry",
        "_record",
        "_typing",
        "experimental_geometry",
    },
    "_hole_patterns": {"_hole_features", "_pattern_geometry", "_record", "_typing"},
    # Ring geometry: `passages` owned it while it was the only family walking rings.
    "_rings": {"_adjacency", "_geometry", "_typing"},
    "_recess_records": {"_record", "_typing"},
    # Exact volumetric evidence is shared without importing either recognition policy.
    "_volume_probe": {"_typing"},
    # The recess stack, bottom to top: faces are read, candidates are proposed from them,
    # obround ends recover the ones no wall pair found, and reduction turns what is left into
    # features. Each layer may import the ones below it and none may import one above, which is
    # the property the split was for -- a family predicate cannot quietly become substrate.
    "_recess_faces": {"_adjacency", "_recess_records", "_typing", "_volume_probe"},
    "_recess_reduce": {
        "_adjacency",
        "_body_identity",
        "_geometry",
        "_recess_faces",
        "_recess_records",
        "_typing",
        "_volume_probe",
    },
    "_recess_obround": {
        "_adjacency",
        "_geometry",
        "_recess_faces",
        "_recess_records",
        "_recess_reduce",
        "_typing",
    },
    "_recess_core": {
        "_adjacency",
        "_geometry",
        "_recess_faces",
        "_recess_obround",
        "_recess_records",
        "_recess_reduce",
        "_typing",
    },
    "_recess_features": {
        "_adjacency",
        "_body_identity",
        "_candidates",
        "_claims",
        "_recess_core",
        "_recess_records",
        "_recess_reduce",
        "_typing",
    },
    # The reconciler names both families it decides between, so it sits above them and neither
    # sits above it -- a recogniser importing this is the order dependence ADR 0003 forbids.
    "_reconcile": {
        "_candidates",
        "_claims",
        "_dispositions",
        "_passage_compat",
        "_recess_records",
        "angled_steps",
        "chamfers",
        "grooves",
        "passages",
        "prismatic_pockets",
        "turned",
    },
    # Internal orchestration registry: it names family adapters but owns no geometry or policy.
    # Family modules never import it, so the edge remains one-way from orchestration to families.
    "_registry": {
        "_candidates",
        "_claims",
        "_passage_compat",
        "_features",
        "_hole_features",
        "_recess_features",
        "_run",
        "_section_recess",
        "_section_recess_discovery",
        "_typing",
        "angled_steps",
        "blends",
        "chamfers",
        "circular_blind_steps",
        "countersinks",
        "edge_open_circular_recesses",
        "edge_open_prismatic_recesses",
        "fillets",
        "flats",
        "grooves",
        "levels",
        "oriented_slots",
        "pads",
        "paired_ramp_steps",
        "through_steps",
        "passages",
        "plates",
        "polygonal_bosses",
        "prismatic_pockets",
        "profiled_bores",
        "repeating_profiles",
        "rectangular_blind_slots",
        "round_bottom_slots",
        "section_recesses",
        "slots",
        "turned",
    },
    "_recess_patterns": {"_pattern_geometry", "_recess_records"},
    "oriented_slots": {
        "_adjacency",
        "_candidates",
        "_claims",
        "_geometry",
        "_pattern_geometry",
        "_record",
        "_section_passages",
        "_typing",
        "passages",
    },
    # Epic 0004's private geometry values are a stdlib-only leaf. The adapter names exactly the
    # two polygonal records whose legacy values round-trip; production recognition does not use it.
    "_sections": set(),
    "_section_adapters": {"_sections", "_section_recess", "passages", "prismatic_pockets"},
    # Effective analytic facts sit above original graph identity and below run orchestration.
    "_effective_surfaces": {"_adjacency", "_analytic_surfaces", "_geometry", "_typing"},
    # Neutral opt-in support bridges consume only original graph and effective-surface facts.
    "_blend_view": {"_adjacency", "_analytic_surfaces", "_effective_surfaces"},
    # Provisional F7 spike facade: explicitly public-by-module but absent from root exports.
    # It may wrap the neutral layers; consumers must not reach those concrete classes.
    "experimental_geometry": {
        "_adjacency",
        "_analytic_surfaces",
        "_blend_view",
        "_effective_surfaces",
        "_typing",
        "inspection",
    },
    # Supported F7 declaration-inspection surface. It projects the neutral analytic
    # substrate and re-exports only the four independently proven family readers.
    "inspection": {
        "_adjacency",
        "_bevel",
        "_effective_surfaces",
        "_typing",
        "countersinks",
        "grooves",
        "profiled_bores",
    },
    # Supported issue-375 projection of one completed accepted inventory. It may translate
    # private run identity into opaque public references but owns no discovery or policy.
    # Issue #494 also permits the bounded same-product explanation projection (ADR 0012).
    "evidence": {
        "_adjacency", "_candidates", "_registry", "_section_recess", "_typing",
        "explanations", "result",
    },
    # The only graph/evidence translation seam. Feature consumers receive facade refs and
    # cannot import the concrete graph or writer themselves.
    "_geometry_evidence": {
        "_candidates",
        "_claims",
        "_typing",
        "experimental_geometry",
    },
    "polygonal_bosses": {
        "_candidates",
        "_geometry",
        "_geometry_evidence",
        "_record",
        "_typing",
        "experimental_geometry",
    },
}

ARC_READER_SITES = {
    "src/quiddity/edge_open_circular_recesses:_ordered_chain:arc:1": ("legacy-contract"),
    **{
        f"src/quiddity/edge_open_circular_recesses:"
        f"recognise_edge_open_circular_pockets:arc:{ordinal}": disposition
        for ordinal, disposition in enumerate(
            ("exact-nonsmooth", "legacy-contract", "legacy-contract"), start=1
        )
    },
    **{
        f"src/quiddity/edge_open_prismatic_recesses:"
        f"_complete_wall_boundaries:arc:{ordinal}": "exact-nonsmooth"
        for ordinal in (1, 2)
    },
    **{
        f"src/quiddity/edge_open_prismatic_recesses:"
        f"recognise_edge_open_prismatic_recesses:arc:{ordinal}": disposition
        for ordinal, disposition in enumerate(
            ("exact-nonsmooth", "exact-nonsmooth", "legacy-contract"), start=1
        )
    },
    "src/quiddity/circular_blind_steps:_is_concave:arc:1": "exact-nonsmooth",
    "src/quiddity/circular_blind_steps:_is_convex:arc:1": "exact-nonsmooth",
    "src/quiddity/paired_ramp_steps:_is_concave:arc:1": "exact-nonsmooth",
    "src/quiddity/paired_ramp_steps:_is_convex:arc:1": "exact-nonsmooth",
    "src/quiddity/through_steps:_relation:arc:1": "legacy-contract",
    "src/quiddity/through_steps:_coplanar_region:arc:1": "legacy-contract",
    "src/quiddity/through_steps:_common_terminal:arc:1": "legacy-contract",
    "src/quiddity/through_steps:_common_terminal:arc:2": "legacy-contract",
    "src/quiddity/round_bottom_slots:_cylinder_region:arc:1": "legacy-source",
    "src/quiddity/round_bottom_slots:_cylinder_region:is_any_smooth:1": "any-smooth",
    "src/quiddity/round_bottom_slots:_relation:arc:1": "legacy-contract",
    "src/quiddity/round_bottom_slots:_common_convex_context:arc:1": "legacy-contract",
    "src/quiddity/round_bottom_slots:_coplanar_region:arc:1": "legacy-source",
    "src/quiddity/round_bottom_slots:_coplanar_region:is_any_smooth:1": "any-smooth",
    "src/quiddity/round_bottom_slots:_recognise_one:arc:1": "legacy-contract",
    "src/quiddity/rectangular_blind_slots:_recognise_one:arc:1": "exact-nonsmooth",
    "src/quiddity/_recess_core:_bounded_inner_region:arc:1": "legacy-contract",
    "src/quiddity/_section_passages:_bounded_inner_region:arc:1": "legacy-contract",
    "src/quiddity/_section_passages:_mouth_regions:arc:1": "exact-nonsmooth",
    "src/quiddity/prismatic_pockets:_inner_region:arc:1": "legacy-contract",
    "src/quiddity/prismatic_pockets:_floor_seeded_regions:arc:1": "legacy-contract",
    "src/quiddity/prismatic_pockets:_one_ended_regions:arc:1": "legacy-contract",
    "tools/audit_mfcadpp_component_overlap:_internal_arcs:arc:1": "legacy-contract",
    "tools/audit_mfcadpp_cavity_enclosures:_expand:arc:1": "legacy-contract",
    "tools/audit_mfcadpp_cavity_enclosures:_convex_mouth:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_one_ended_pockets:_mouth_wires:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_oriented_circular_pockets:_one_candidate:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_oriented_circular_pockets:_one_candidate:arc:2": "legacy-contract",
    "tools/audit_mfcadpp_oriented_circular_pockets:_one_candidate:arc:3": "legacy-contract",
    "src/quiddity/_section_recess:_one_obround_candidate:arc:1": (
        "exact-nonsmooth"
    ),
    "src/quiddity/_section_recess:_one_obround_candidate:arc:2": (
        "legacy-contract"
    ),
    "src/quiddity/_section_recess:_one_obround_candidate:arc:3": (
        "legacy-contract"
    ),
    "src/quiddity/_section_recess:_one_polygonal_candidate:arc:1": (
        "exact-nonsmooth"
    ),
    "src/quiddity/_section_recess:_one_polygonal_candidate:arc:2": (
        "legacy-contract"
    ),
    "tools/audit_mfcadpp_floor_interrupted_pockets:_raw_regions:arc:1": "legacy-contract",
    "tools/audit_mfcadpp_floor_interrupted_pockets:_probe_region:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_floor_interrupted_pockets:_probe_region:arc:2": "exact-nonsmooth",
    "tools/audit_mfcadpp_floor_interrupted_pockets:_probe_region:arc:3": "exact-nonsmooth",
    "src/quiddity/experimental_geometry:arc:arc:1": "facade-projection",
    "src/quiddity/experimental_geometry:smooth_side:smooth_side:1": "facade-projection",
    "tests/test_slot_attribution:_fresh_occurrences_one:arc:1": "legacy-contract",
    "tests/test_slot_attribution:_fresh_occurrences_one:arc:2": "legacy-contract",
    "tests/test_slot_attribution:_fresh_occurrences_one:arc:3": "legacy-contract",
    "tests/test_channel_attribution:_bounds_one_void:arc:1": "pair-agreement",
    "tests/test_channel_attribution:_bounds_one_void:arc:2": "pair-agreement",
    "tests/test_channel_attribution:_bounds_one_void:arc:3": "exact-nonsmooth",
    "tests/test_channel_attribution:_bounds_one_void:arc:4": "exact-nonsmooth",
    "tests/test_channel_attribution:_uninterrupted_span:arc:1": "opposed-nonsmooth",
    "tests/test_channel_attribution:_uninterrupted_span:arc:2": "opposed-nonsmooth",
    "src/quiddity/_adjacency:smooth_region:arc:1": "legacy-source",
    "src/quiddity/_adjacency:smooth_region:is_any_smooth:1": "any-smooth",
    "src/quiddity/_adjacency:smooth_side:arc:1": "legacy-source",
    "src/quiddity/_adjacency:smooth_side:is_any_smooth:1": "any-smooth",
    "src/quiddity/_recess_core:_concave_boundary_regions:arc:1": "exact-nonsmooth",
    "src/quiddity/_recess_core:_uninterrupted_long_span:arc:1": "opposed-nonsmooth",
    "src/quiddity/_recess_core:_uninterrupted_long_span:arc:2": "opposed-nonsmooth",
    "src/quiddity/_recess_core:_has_smooth_depth_closure:arc:1": "legacy-source",
    "src/quiddity/_recess_core:_has_smooth_depth_closure:arc:2": "legacy-source",
    "src/quiddity/_recess_core:_has_smooth_depth_closure:arc:3": "legacy-source",
    "src/quiddity/_recess_core:_has_smooth_depth_closure:is_any_smooth:1": "any-smooth",
    "src/quiddity/_recess_core:_has_smooth_depth_closure:is_any_smooth:2": "any-smooth",
    "src/quiddity/_recess_core:_has_smooth_depth_closure:is_any_smooth:3": "any-smooth",
    "src/quiddity/_recess_core:_bounds_one_void:arc:1": "pair-agreement",
    "src/quiddity/_recess_core:_bounds_one_void:arc:2": "pair-agreement",
    "src/quiddity/_blend_view:_native_neutral:arc:1": "legacy-contract",
    "src/quiddity/_blend_view:_native_neutral:smooth_side:1": "side-read",
    "src/quiddity/_blend_view:_classify:smooth_side:1": "side-read",
    "src/quiddity/_blend_view:_classify:arc:1": "legacy-contract",
    "src/quiddity/_blend_view:_classify:arc:2": "legacy-contract",
    "src/quiddity/_blend_view:__init__:arc:1": "legacy-contract",
    "src/quiddity/blends:_circular_proposal:arc:1": "legacy-contract",
    "src/quiddity/blends:_circular_proposal:arc:2": "legacy-contract",
    "src/quiddity/blends:_support_region:arc:1": "legacy-contract",
    "src/quiddity/blends:_toroidal_components:arc:1": "legacy-contract",
    "tools/audit_mfcadpp_through_steps:_arc_name:arc:1": "legacy-contract",
    "tools/audit_mfcadpp_circular_blind_steps:_is_concave:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_circular_blind_steps:_is_convex:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_circular_blind_steps:_arc_name:arc:1": "legacy-contract",
    "tools/audit_mfcadpp_paired_ramp_steps:_is_concave:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_paired_ramp_steps:_is_convex:arc:1": "exact-nonsmooth",
    "tools/audit_mfcadpp_paired_ramp_steps:_arc_name:arc:1": "legacy-contract",
}

# Tests are part of the reviewed reader surface: an assertion may intentionally pin the legacy
# closed value, but it may not accidentally teach production-style code that truthiness or a
# negative inference is meaningful.  One entry covers every occurrence in the test module; the
# AST-derived ordinal makes additions fail visibly.
for _site in (
    "_arcs:arc:1",
    "test_an_arc_reads_the_same_from_either_face:arc:1",
    "test_an_arc_reads_the_same_from_either_face:arc:2",
    "_smooth_pairs:arc:1",
    "test_open_faces_can_be_legacy_smooth_but_side_unproven:arc:1",
    "test_open_faces_can_be_legacy_smooth_but_side_unproven:arc:2",
    "test_failed_differential_enrichment_never_rewrites_legacy_smooth:arc:1",
    "test_non_smooth_and_foreign_side_queries_fail_intentionally:arc:1",
    "test_a_smooth_region_is_maximal_immutable_and_cached_for_each_member:arc:1",
    "test_a_smooth_region_is_maximal_immutable_and_cached_for_each_member:arc:2",
    "test_faces_that_do_not_meet_have_no_arc:arc:1",
    "test_a_pair_meeting_along_several_edges_is_classified_from_all_of_them:arc:1",
    "test_a_shallow_corner_is_not_smooth:arc:1",
    "test_imported_step_geometry_classifies_without_unknowns:arc:1",
    "test_a_repeated_query_does_not_recompute_the_kernel_work:arc:1",
    "test_a_repeated_query_does_not_recompute_the_kernel_work:arc:2",
    "test_a_repeated_query_does_not_recompute_the_kernel_work:arc:3",
    "test_shared_edges_that_disagree_give_no_single_answer:arc:1",
    "test_shared_edges_that_disagree_give_no_single_answer:arc:2",
    "test_a_warm_cache_still_refuses_another_graph_s_nodes:arc:1",
    "test_a_warm_cache_still_refuses_another_graph_s_nodes:arc:2",
    "test_open_topods_solid_cannot_authorize_material_side:arc:1",
    "test_tangent_higher_order_bezier_is_not_a_neutral_continuation:arc:1",
):
    ARC_READER_SITES[f"tests/test_arcs:{_site}"] = "legacy-contract"
for _site in (
    "_smooth_pairs:is_any_smooth:1",
    "test_open_faces_can_be_legacy_smooth_but_side_unproven:is_any_smooth:1",
    "test_open_topods_solid_cannot_authorize_material_side:is_any_smooth:1",
    "test_tangent_higher_order_bezier_is_not_a_neutral_continuation:is_any_smooth:1",
):
    ARC_READER_SITES[f"tests/test_arcs:{_site}"] = "any-smooth"
for _site in (
    "test_external_and_internal_rounds_have_opposite_smooth_sides:smooth_side:1",
    "test_external_and_internal_rounds_have_opposite_smooth_sides:smooth_side:2",
    "test_equivalent_native_surface_splits_are_neutral:smooth_side:1",
    "test_split_native_side_survives_step_round_trip:smooth_side:1",
    "test_smooth_convex_side_is_rigid_transform_and_scale_invariant:smooth_side:1",
    "test_smooth_side_is_symmetric_and_cached_once_per_edge:smooth_side:1",
    "test_smooth_side_is_symmetric_and_cached_once_per_edge:smooth_side:2",
    "test_open_faces_can_be_legacy_smooth_but_side_unproven:smooth_side:1",
    "test_disagreeing_samples_and_shared_edges_are_side_unproven:smooth_side:1",
    "test_failed_differential_enrichment_never_rewrites_legacy_smooth:smooth_side:1",
    "test_non_smooth_and_foreign_side_queries_fail_intentionally:smooth_side:1",
    "test_non_smooth_and_foreign_side_queries_fail_intentionally:smooth_side:2",
    "test_sided_rounds_survive_step_round_trip:smooth_side:1",
    "test_open_smooth_join_remains_unproven_after_step_round_trip:smooth_side:1",
    "keyed_sides:smooth_side:1",
    "test_duplicate_solid_ownership_cannot_authorize_material_side:smooth_side:1",
    "test_ownership_kernel_failures_are_side_unproven:smooth_side:1",
    "test_a_disconnected_second_solid_does_not_poison_owned_sides:smooth_side:1",
    "test_open_topods_solid_cannot_authorize_material_side:smooth_side:1",
    "test_tangent_higher_order_bezier_is_not_a_neutral_continuation:smooth_side:1",
    "test_non_manifold_three_face_edge_is_side_unproven:smooth_side:1",
):
    ARC_READER_SITES[f"tests/test_arcs:{_site}"] = "side-read"


def test_every_arc_reader_has_one_reviewed_disposition() -> None:
    found: set[str] = set()
    paths = [*PACKAGE.glob("*.py"), *(ROOT / "tools").glob("*.py"), *(ROOT / "tests").rglob("*.py")]
    for path in paths:
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        parents = {
            child: parent for parent in ast.walk(tree) for child in ast.iter_child_nodes(parent)
        }
        calls: list[tuple[int, int, str, str]] = []
        for node in ast.walk(tree):
            mechanism = None
            if isinstance(node, ast.Call):
                if isinstance(node.func, ast.Attribute) and node.func.attr in {
                    "arc",
                    "smooth_side",
                }:
                    mechanism = node.func.attr
                elif isinstance(node.func, ast.Name) and node.func.id == "is_any_smooth":
                    mechanism = "is_any_smooth"
            if mechanism is None:
                continue
            owner = node
            function = "module"
            while owner in parents:
                owner = parents[owner]
                if isinstance(owner, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    function = owner.name
                    break
            calls.append((node.lineno, node.col_offset, function, mechanism))
        ordinals: dict[tuple[str, str], int] = {}
        call_nodes = {
            (node.lineno, node.col_offset): node
            for node in ast.walk(tree)
            if isinstance(node, ast.Call)
        }
        for line, column, function, mechanism in sorted(calls):
            base = (function, mechanism)
            ordinals[base] = ordinals.get(base, 0) + 1
            source = path.relative_to(ROOT).with_suffix("").as_posix()
            site = f"{source}:{function}:{mechanism}:{ordinals[base]}"
            found.add(site)
            disposition = ARC_READER_SITES.get(site)
            if disposition is None:
                continue
            call = call_nodes[line, column]
            parent = parents.get(call)
            if mechanism in {"arc", "smooth_side"}:
                assert not isinstance(parent, (ast.If, ast.UnaryOp)), site
            if isinstance(parent, ast.Compare):
                assert not any(isinstance(op, (ast.NotEq, ast.NotIn)) for op in parent.ops), site
            if disposition == "any-smooth":
                assert mechanism == "is_any_smooth", site
            elif disposition == "side-read":
                assert mechanism == "smooth_side", site
            elif disposition == "legacy-source":
                assert (
                    isinstance(parent, ast.Call)
                    and isinstance(parent.func, ast.Name)
                    and parent.func.id == "is_any_smooth"
                ), site
            elif disposition == "exact-nonsmooth":
                comparison = (
                    parents.get(parent)
                    if isinstance(parent, (ast.Set, ast.List, ast.Tuple))
                    else parent
                )
                assert isinstance(comparison, ast.Compare), site
                literals = {
                    value.value
                    for value in [comparison.left, *comparison.comparators]
                    if isinstance(value, ast.Constant) and isinstance(value.value, str)
                }
                assert literals <= {"convex", "concave"} and literals, site
            elif disposition == "opposed-nonsmooth":
                assert isinstance(parent, ast.Call), site
                assert isinstance(parent.func, ast.Name), site
                assert parent.func.id == "is_opposed_nonsmooth", site
            elif disposition == "pair-agreement":
                assert isinstance(parent, ast.Call), site
                assert isinstance(parent.func, ast.Name), site
                assert parent.func.id == "same_arc_kind", site
            elif disposition == "legacy-contract":
                assert isinstance(
                    parent,
                    (ast.Assign, ast.Call, ast.Compare, ast.Expr, ast.SetComp, ast.Subscript),
                ), site
                if isinstance(parent, ast.Call):
                    assert isinstance(parent.func, ast.Name), site
                    assert parent.func.id == "is_any_smooth", site
                if isinstance(parent, ast.Compare):
                    assert all(isinstance(op, (ast.Eq, ast.Is, ast.In)) for op in parent.ops), site

    assert found == set(ARC_READER_SITES)
    assert set(ARC_READER_SITES.values()) <= {
        "any-smooth",
        "exact-nonsmooth",
        "facade-projection",
        "legacy-contract",
        "legacy-source",
        "opposed-nonsmooth",
        "pair-agreement",
        "side-read",
    }
    assert not any(
        site.startswith("src/quiddity/_recess_core:") and disposition == "side-read"
        for site, disposition in ARC_READER_SITES.items()
    )


def test_effective_surface_reader_roster_covers_every_raw_classification() -> None:
    from quiddity._effective_surfaces import (
        SURFACE_READER_ROSTER,
        SURFACE_READER_SITES,
        SurfaceReaderDisposition,
    )

    reader_sites: set[str] = set()
    for path in PACKAGE.glob("*.py"):
        if path.name == "_effective_surfaces.py":
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        adaptor_aliases = {
            alias.asname or alias.name
            for node in ast.walk(tree)
            if isinstance(node, ast.ImportFrom) and node.module == "OCP.BRepAdaptor"
            for alias in node.names
            if alias.name == "BRepAdaptor_Surface"
        }
        parents = {
            child: parent for parent in ast.walk(tree) for child in ast.iter_child_nodes(parent)
        }
        found: list[tuple[int, int, str, str]] = []
        for node in ast.walk(tree):
            kind = None
            if isinstance(node, ast.Call):
                if isinstance(node.func, ast.Name) and node.func.id in adaptor_aliases:
                    kind = "adaptor"
                if isinstance(node.func, ast.Attribute) and node.func.attr == "surface":
                    kind = "graph_surface"
                if isinstance(node.func, ast.Attribute) and node.func.attr == "is_planar":
                    kind = "is_planar"
            if isinstance(node, ast.Attribute) and node.attr == "geom_type":
                kind = "geom_type"
            if kind is None:
                continue
            owner = node
            function = "module"
            while owner in parents:
                owner = parents[owner]
                if isinstance(owner, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    function = owner.name
                    break
            found.append((node.lineno, node.col_offset, function, kind))
        ordinal: dict[tuple[str, str], int] = {}
        for _, _, function, kind in sorted(found):
            base = (function, kind)
            ordinal[base] = ordinal.get(base, 0) + 1
            reader_sites.add(f"{path.stem}:{function}:{kind}:{ordinal[base]}")

    assert reader_sites == set(SURFACE_READER_SITES)
    raw_modules = {site.split(":", 1)[0] for site in reader_sites}
    reviewed_raw = {
        module
        for module, (disposition, _rationale) in SURFACE_READER_ROSTER.items()
        if disposition is not SurfaceReaderDisposition.MIGRATED_EFFECTIVE
    }
    migrated = {
        module
        for module, (disposition, _rationale) in SURFACE_READER_ROSTER.items()
        if disposition is SurfaceReaderDisposition.MIGRATED_EFFECTIVE
    }
    assert raw_modules == reviewed_raw
    assert migrated == {"circular_blind_steps", "pads"}
    pad_source = (PACKAGE / "pads.py").read_text(encoding="utf-8")
    assert "face_surfaces.fact(" in pad_source
    assert "face_surfaces.use(" in pad_source
    circular_source = (PACKAGE / "circular_blind_steps.py").read_text(encoding="utf-8")
    assert "effective.fact(" in circular_source
    assert "effective.use(" in circular_source
    assert all(rationale.strip() for _, rationale in SURFACE_READER_ROSTER.values())
    assert all(rationale.strip() for _, rationale in SURFACE_READER_SITES.values())


def test_reconciler_never_imports_or_calls_discovery() -> None:
    path = PACKAGE / "_reconcile.py"
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    imported = [
        alias.name
        for node in ast.walk(tree)
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
        if alias.name.startswith("recognise_")
    ]
    called = [
        node.func.id if isinstance(node.func, ast.Name) else node.func.attr
        for node in ast.walk(tree)
        if isinstance(node, ast.Call)
        and isinstance(node.func, (ast.Name, ast.Attribute))
        and (
            (isinstance(node.func, ast.Name) and node.func.id.startswith("recognise_"))
            or (isinstance(node.func, ast.Attribute) and node.func.attr.startswith("recognise_"))
        )
    ]
    qualified_references = [
        node.attr
        for node in ast.walk(tree)
        if isinstance(node, ast.Attribute) and node.attr.startswith("recognise_")
    ]
    assert imported == []
    assert called == []
    assert qualified_references == []


def test_recess_reconciler_accepts_completed_records_and_frozen_evidence_only() -> None:
    module = importlib.import_module("quiddity._reconcile")
    hints = typing.get_type_hints(module.reconcile_recesses)

    assert set(hints) == {"slots", "pockets", "prismatic", "passages", "evidence", "return"}
    assert hints["evidence"].__name__ == "EvidenceIndex"


def test_aggregate_phase_functions_have_one_way_capability_boundaries() -> None:
    module = importlib.import_module("quiddity.result")
    expected = {
        "_discover_all": {"context", "ledger", "return"},
        "_reconcile_existing": {"physical", "evidence", "return"},
        "diagnose_residuals": {"reconciliation", "evidence", "return"},
        "_derive_patterns": {"accepted", "return"},
        "_derive_passage_compat": {"inputs", "projection", "return"},
        "_project_result": {"context", "accepted", "derived", "evidence", "return"},
    }
    for name, parameters in expected.items():
        assert set(typing.get_type_hints(getattr(module, name))) == parameters

    run_module = importlib.import_module("quiddity._run")
    context = typing.get_type_hints(run_module.RecognitionContext)
    assert set(context) == {
        "part",
        "face_edges",
        "graph",
        "geometry",
        "surfaces",
        "face_surfaces",
        "cylinders",
        "rotational",
    }
    assert not ({"ledger", "sink", "evidence", "index"} & set(context))

    ledger_type = typing.get_type_hints(module._discover_all)["ledger"]
    assert ledger_type.__name__ == "ClaimLedger"
    registry_module = importlib.import_module("quiddity._registry")
    writer_type = typing.get_type_hints(registry_module.DiscoveryServices)["writer"]
    assert writer_type.__name__ == "EvidenceWriter"
    assert {name for name in dir(writer_type) if not name.startswith("_")} == {
        "add_defining",
        "graph",
        "sink",
    }

    product_fields = set(module.InventoryProduct.__dataclass_fields__)
    assert "reconciliation" in product_fields
    assert "diagnostics" in product_fields
    assert "accepted" not in product_fields and "distinct_steps" not in product_fields


def test_only_result_orchestration_may_create_restricted_completed_inputs() -> None:
    candidate_module = importlib.import_module("quiddity._candidates")
    assert {name for name in dir(candidate_module.CompletedInputs) if not name.startswith("_")} == {
        "occurrences",
        "records",
    }
    assert {
        name for name in dir(candidate_module.CompletedOccurrence) if not name.startswith("_")
    } == {"defining", "record", "solid"}
    callers: list[tuple[str, str]] = []
    constructors: list[str] = []
    for path in PACKAGE.glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))

        class Visitor(ast.NodeVisitor):
            function = "<module>"

            def __init__(self, filename: str) -> None:
                self.filename = filename

            def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
                previous = self.function
                self.function = node.name
                self.generic_visit(node)
                self.function = previous

            visit_AsyncFunctionDef = visit_FunctionDef

            def visit_Call(self, node: ast.Call) -> None:
                if isinstance(node.func, ast.Attribute) and node.func.attr == "restricted_inputs":
                    callers.append((self.filename, self.function))
                if isinstance(node.func, ast.Name) and node.func.id in {
                    "CompletedInputs",
                    "CompletedOccurrence",
                }:
                    constructors.append(self.filename)
                self.generic_visit(node)

        Visitor(path.name).visit(tree)

    assert sorted(callers) == [
        ("_claims.py", "restricted_inputs"),
        ("result.py", "_discover_all"),
    ]
    assert constructors == []


def test_private_section_adapters_are_only_used_by_the_unified_projection() -> None:
    importers: list[str] = []
    for path in PACKAGE.glob("*.py"):
        if path.name in {"_section_adapters.py", "_sections.py"}:
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        if any(
            isinstance(node, ast.ImportFrom)
            and node.module == "quiddity._section_adapters"
            for node in ast.walk(tree)
        ):
            importers.append(path.name)
    assert importers == ["result.py"]


def test_projection_family_bindings_match_the_registry() -> None:
    module = importlib.import_module("quiddity.result")
    source = inspect.getsource(module._project_result)
    tree = ast.parse(textwrap.dedent(source))
    result_call = next(
        node
        for node in ast.walk(tree)
        if isinstance(node, ast.Call)
        and isinstance(node.func, ast.Name)
        and node.func.id == "_LegacyRecognitionResult"
    )
    projected: dict[str, str] = {}
    for keyword in result_call.keywords:
        families = {
            node.attr
            for node in ast.walk(keyword.value)
            if isinstance(node, ast.Attribute)
            and isinstance(node.value, ast.Name)
            and node.value.id == "FamilyId"
        }
        if families:
            assert len(families) == 1, keyword.arg
            projected[typing.cast(str, keyword.arg)] = families.pop()

    registry = importlib.import_module("quiddity._registry")
    expected = {
        definition.result_field: definition.family.name
        for definition in registry.PHYSICAL_DEFINITIONS
    }
    # ADR 0019 intentionally converges multiple independently discovered physical families into
    # this one public result field; its native binding is therefore not visible in the constructor
    # expression inspected above.
    expected.pop("section_recesses")
    assert projected == expected


def test_residual_reducer_cannot_rediscover_or_mutate_geometry() -> None:
    module = importlib.import_module("quiddity._diagnostics")
    hints = typing.get_type_hints(module.diagnose_residuals)
    assert set(hints) == {"reconciliation", "evidence", "return"}

    tree = ast.parse(Path(module.__file__).read_text(encoding="utf-8"))
    imported = {
        alias.name
        for node in ast.walk(tree)
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
    }
    assert not ({"Part", "FaceGraph", "EvidenceSink"} & imported)
    assert not any(
        isinstance(node, ast.Call)
        and isinstance(node.func, (ast.Name, ast.Attribute))
        and (
            (isinstance(node.func, ast.Name) and node.func.id.startswith("recognise_"))
            or (isinstance(node.func, ast.Attribute) and node.func.attr.startswith("recognise_"))
        )
        for node in ast.walk(tree)
    )


def test_all_recess_reconciler_call_sites_pass_completed_passages_and_evidence() -> None:
    roots = (PACKAGE, ROOT / "tools", ROOT / "tests")
    calls = []
    for root in roots:
        for path in sorted(root.glob("*.py")):
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
            calls.extend(
                (path.relative_to(ROOT).as_posix(), node)
                for node in ast.walk(tree)
                if isinstance(node, ast.Call)
                and isinstance(node.func, ast.Name)
                and node.func.id == "reconcile_recesses"
            )

    assert {path for path, _ in calls} == {
        "tests/test_mfcadpp_corpus.py",
    }
    for _path, call in calls:
        assert len(call.args) == 5 and not call.keywords
        assert "passage" in ast.unparse(call.args[3]).lower()
        evidence = ast.unparse(call.args[4]).lower()
        assert "evidence" in evidence or "snapshot_index" in evidence


def test_migrated_discovery_cores_receive_write_only_evidence() -> None:
    for module_name, function_name in (
        ("angled_steps", "_discover_angled_steps"),
        ("passages", "_discover_section_passages"),
    ):
        module = importlib.import_module(f"quiddity.{module_name}")
        hints = typing.get_type_hints(getattr(module, function_name))
        assert "ClaimLedger" not in {getattr(hint, "__name__", "") for hint in hints.values()}
        assert "EvidenceSink" in repr(hints["sink"])

        path = PACKAGE / f"{module_name}.py"
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        function = next(
            node
            for node in tree.body
            if isinstance(node, ast.FunctionDef) and node.name == function_name
        )
        read_names = {"candidate_set", "defining_of", "claims_of", "claims", "ledger"}
        used = {
            node.attr
            for node in ast.walk(function)
            if isinstance(node, ast.Attribute) and node.attr in read_names
        }
        assert used == set()


def _package_import_graph() -> dict[str, set[str]]:
    paths = {path.stem: path for path in PACKAGE.glob("*.py")}
    graph: dict[str, set[str]] = {module: set() for module in paths}
    package = "quiddity"
    prefix = f"{package}."
    for module, path in paths.items():
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and node.module == package:
                # `from quiddity import chamfers` names the module as an alias, not in
                # node.module. Reading only node.module made this form invisible, so a seam or
                # cycle violation written this way passed every check in this file.
                names = [f"{prefix}{alias.name}" for alias in node.names]
            elif isinstance(node, ast.ImportFrom) and node.module:
                names = [node.module]
            elif isinstance(node, ast.Import):
                names = [alias.name for alias in node.names]
            else:
                continue
            for name in names:
                if name.startswith(prefix) and (target := name.removeprefix(prefix)) in paths:
                    graph[module].add(target)
    return graph


def test_runtime_package_does_not_import_draftwright():
    violations = []
    for path in sorted(PACKAGE.glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                names = [alias.name for alias in node.names]
            elif isinstance(node, ast.ImportFrom):
                names = [node.module or ""]
            else:
                continue
            if any(name == "draftwright" or name.startswith("draftwright.") for name in names):
                violations.append(f"{path.name}:{node.lineno}")
    assert violations == []


def test_every_defined_public_recogniser_is_exported_and_snapshotted():
    defined = set()
    for path in sorted(PACKAGE.glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        defined.update(
            node.name
            for node in tree.body
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and node.name.startswith("recognise_")
        )

    exported = {name for name in recognition.__all__ if name.startswith("recognise_")}
    from tools._legacy_recognition import __all__ as retired

    assert exported == defined - set(retired)


def test_module_graph_is_acyclic() -> None:
    graph = _package_import_graph()
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(module: str, path: tuple[str, ...] = ()) -> None:
        assert module not in visiting, " -> ".join((*path, module))
        if module in visited:
            return
        visiting.add(module)
        for dependency in sorted(graph[module]):
            visit(dependency, (*path, module))
        visiting.remove(module)
        visited.add(module)

    for module in sorted(graph):
        visit(module)


def test_internal_module_seams_match_adr_0007() -> None:
    """No module may import outside the seam ADR 0007 allows it.

    A containment check, not equality. The invariant is that nothing reaches across a seam;
    whether a module happens to use every import it is permitted is an implementation detail,
    and requiring exact equality turned "this function no longer needs that helper" into a
    test failure.
    """

    graph = _package_import_graph()

    crossings = {
        module: sorted(graph[module] - allowed)
        for module, allowed in MODULE_SEAM_EDGES.items()
        if graph[module] - allowed
    }
    assert crossings == {}


def test_neutral_blend_view_has_exactly_the_reviewed_consumers() -> None:
    graph = _package_import_graph()
    assert {module for module, dependencies in graph.items() if "_blend_view" in dependencies} == {
        "blends",
        "experimental_geometry",
    }
    assert {
        module for module, dependencies in graph.items() if "experimental_geometry" in dependencies
    } == {"_geometry_evidence", "_run", "pads", "polygonal_bosses"}
    assert not (
        graph["polygonal_bosses"]
        & {"_adjacency", "_analytic_surfaces", "_blend_view", "_effective_surfaces"}
    )


def test_f3b_blend_index_and_view_have_only_reviewed_production_call_sites() -> None:
    def resolver(aliases: dict[str, str]):
        def qualified(node: ast.expr) -> str:
            if isinstance(node, ast.Name):
                return aliases.get(node.id, node.id)
            if isinstance(node, ast.Attribute):
                return f"{qualified(node.value)}.{node.attr}"
            return ""

        return qualified

    def scan_calls(path_name: str, source: str) -> set[tuple[str, str]]:
        tree = ast.parse(source, filename=path_name)
        aliases: dict[str, str] = {}
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and node.module:
                for alias in node.names:
                    aliases[alias.asname or alias.name] = f"{node.module}.{alias.name}"
            elif isinstance(node, ast.Import):
                for alias in node.names:
                    aliases[alias.asname or alias.name.split(".")[0]] = alias.name

        qualified = resolver(aliases)
        guarded_qualified = {
            "quiddity._blend_view.BlendCollapseIndex",
            "quiddity._effective_surfaces.EffectiveSurfaceIndex",
        }
        changed = True
        while changed:
            changed = False
            for node in ast.walk(tree):
                if not isinstance(node, ast.Assign):
                    continue
                resolved = qualified(node.value)
                if resolved not in guarded_qualified:
                    continue
                for target in node.targets:
                    if isinstance(target, ast.Name) and aliases.get(target.id) != resolved:
                        aliases[target.id] = resolved
                        changed = True

        index_names = {
            target.id
            for node in ast.walk(tree)
            if isinstance(node, ast.Assign)
            and isinstance(node.value, ast.Call)
            and qualified(node.value.func) == "quiddity._blend_view.BlendCollapseIndex"
            for target in node.targets
            if isinstance(target, ast.Name)
        }
        view_names = {
            target.id
            for node in ast.walk(tree)
            if isinstance(node, ast.Assign)
            and isinstance(node.value, ast.Call)
            and isinstance(node.value.func, ast.Attribute)
            and isinstance(node.value.func.value, ast.Name)
            and node.value.func.value.id in index_names
            and node.value.func.attr == "view"
            for target in node.targets
            if isinstance(target, ast.Name)
        }
        method_aliases = {
            target.id: node.value.attr
            for node in ast.walk(tree)
            if isinstance(node, ast.Assign)
            and isinstance(node.value, ast.Attribute)
            and (
                (
                    isinstance(node.value.value, ast.Name)
                    and node.value.value.id in index_names
                    and node.value.attr == "view"
                )
                or (
                    isinstance(node.value.value, ast.Name)
                    and node.value.value.id in view_names
                    and node.value.attr == "expand_arc"
                )
                or qualified(node.value)
                in {
                    "quiddity._blend_view.BlendCollapseIndex.view",
                    "quiddity._blend_view.CollapsedGraphView.expand_arc",
                }
            )
            for target in node.targets
            if isinstance(target, ast.Name)
        }
        changed = True
        while changed:
            changed = False
            for node in ast.walk(tree):
                if not isinstance(node, ast.Assign) or not isinstance(node.value, ast.Name):
                    continue
                method = method_aliases.get(node.value.id)
                if method is None:
                    continue
                for target in node.targets:
                    if isinstance(target, ast.Name) and method_aliases.get(target.id) != method:
                        method_aliases[target.id] = method
                        changed = True
        view_names.update(
            target.id
            for node in ast.walk(tree)
            if isinstance(node, ast.Assign)
            and isinstance(node.value, ast.Call)
            and isinstance(node.value.func, ast.Name)
            and method_aliases.get(node.value.func.id) == "view"
            for target in node.targets
            if isinstance(target, ast.Name)
        )
        method_aliases.update(
            {
                target.id: "expand_arc"
                for node in ast.walk(tree)
                if isinstance(node, ast.Assign)
                and isinstance(node.value, ast.Attribute)
                and isinstance(node.value.value, ast.Name)
                and node.value.value.id in view_names
                and node.value.attr == "expand_arc"
                for target in node.targets
                if isinstance(target, ast.Name)
            }
        )
        changed = True
        while changed:
            changed = False
            for node in ast.walk(tree):
                if not isinstance(node, ast.Assign) or not isinstance(node.value, ast.Name):
                    continue
                method = method_aliases.get(node.value.id)
                if method is None:
                    continue
                for target in node.targets:
                    if isinstance(target, ast.Name) and method_aliases.get(target.id) != method:
                        method_aliases[target.id] = method
                        changed = True
        found: set[tuple[str, str]] = set()
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            name = qualified(node.func)
            if name in {
                "quiddity._blend_view.BlendCollapseIndex",
                "quiddity._effective_surfaces.EffectiveSurfaceIndex",
            }:
                found.add((path_name, name.rsplit(".", 1)[-1]))
            elif name in {
                "quiddity._blend_view.BlendCollapseIndex.view",
                "quiddity._blend_view.CollapsedGraphView.expand_arc",
            }:
                owner, method = name.rsplit(".", 2)[-2:]
                found.add((path_name, f"{owner}.{method}"))
            elif (
                isinstance(node.func, ast.Attribute)
                and isinstance(node.func.value, ast.Name)
                and node.func.value.id in index_names
                and node.func.attr == "view"
            ):
                found.add((path_name, "BlendCollapseIndex.view"))
            elif (
                isinstance(node.func, ast.Attribute)
                and isinstance(node.func.value, ast.Name)
                and node.func.value.id in view_names
                and node.func.attr == "expand_arc"
            ):
                found.add((path_name, "CollapsedGraphView.expand_arc"))
            elif isinstance(node.func, ast.Name) and node.func.id in method_aliases:
                owner = (
                    "BlendCollapseIndex"
                    if method_aliases[node.func.id] == "view"
                    else "CollapsedGraphView"
                )
                found.add((path_name, f"{owner}.{method_aliases[node.func.id]}"))
        return found

    calls: set[tuple[str, str]] = set()
    for path in PACKAGE.glob("*.py"):
        calls.update(scan_calls(path.name, path.read_text(encoding="utf-8")))

    guarded_names = {
        "BlendCollapseIndex",
        "CollapsedGraphView",
        "EffectiveSurfaceIndex",
        "FrozenProvenance",
    }
    forbidden_reexports: set[tuple[str, str]] = set()
    exempt = {
        "blends.py",
        "experimental_geometry.py",
        "inspection.py",
        "_run.py",
        "_blend_view.py",
        "_effective_surfaces.py",
    }
    for path in PACKAGE.glob("*.py"):
        if path.name in exempt:
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom):
                forbidden_reexports.update(
                    (path.name, alias.name) for alias in node.names if alias.name in guarded_names
                )
            elif isinstance(node, ast.Attribute) and node.attr in guarded_names:
                forbidden_reexports.add((path.name, node.attr))
    assert forbidden_reexports == set()
    assert calls == {
        ("blends.py", "BlendCollapseIndex"),
        ("blends.py", "EffectiveSurfaceIndex"),
        ("experimental_geometry.py", "BlendCollapseIndex"),
        ("experimental_geometry.py", "BlendCollapseIndex.view"),
        ("experimental_geometry.py", "CollapsedGraphView.expand_arc"),
        ("experimental_geometry.py", "EffectiveSurfaceIndex"),
        ("inspection.py", "EffectiveSurfaceIndex"),
        ("_run.py", "EffectiveSurfaceIndex"),
    }
    mutation_imports = """
from quiddity._blend_view import BlendCollapseIndex, CollapsedGraphView
from quiddity._effective_surfaces import EffectiveSurfaceIndex
"""
    mutations = (
        "index = BlendCollapseIndex(graph, EffectiveSurfaceIndex(graph))\n"
        "view = index.view(selected)\nview.expand_arc(arc)\n",
        "index = BlendCollapseIndex(graph, EffectiveSurfaceIndex(graph))\n"
        "view = BlendCollapseIndex.view(index, selected)\n"
        "CollapsedGraphView.expand_arc(view, arc)\n",
        "Alias = BlendCollapseIndex\nindex = Alias(graph, EffectiveSurfaceIndex(graph))\n"
        "v = index.view\nw = v\nview = w(selected)\ne = view.expand_arc\nf = e\nf(arc)\n",
    )
    for source in mutations:
        found = {name for _path, name in scan_calls("mutation.py", mutation_imports + source)}
        assert {
            "BlendCollapseIndex",
            "BlendCollapseIndex.view",
            "CollapsedGraphView.expand_arc",
            "EffectiveSurfaceIndex",
        } <= found


def test_no_accidental_public_modules() -> None:
    modules = {path.stem for path in PACKAGE.glob("*.py") if path.stem != "__init__"}
    public = {module for module in modules if not module.startswith("_")}

    assert public == PUBLIC_MODULES
    assert set(MODULE_SEAM_EDGES) <= modules


def test_compatibility_facades_preserve_export_identity_and_module_paths() -> None:
    feature_facade = importlib.import_module("quiddity._features")
    recess_facade = importlib.import_module("quiddity.slots")
    implementations = {
        **{
            name: importlib.import_module("quiddity._cylinder_substrate")
            for name in ("analyse_cylinders", "full_cylinders")
        },
        **{
            name: importlib.import_module("quiddity._hole_features")
            for name in (
                "BossRecord",
                "CounterBore",
                "HoleRecord",
                "feature_diameters",
                "recognise_bosses",
                "recognise_holes",
            )
        },
        **{
            name: importlib.import_module("quiddity._hole_patterns")
            for name in (
                "BoltCircle",
                "HoleSpec",
                "LinearArray",
                "RectGrid",
                "recognise_hole_patterns",
            )
        },
    }
    for name, implementation in implementations.items():
        assert getattr(recognition, name) is getattr(feature_facade, name)
        assert getattr(feature_facade, name) is getattr(implementation, name)
        assert getattr(recognition, name).__module__ == "quiddity._features"

    # The property that matters is that a consumer can resolve these annotations after the
    # move, not that they are spelled with particular characters. Comparing the literal strings
    # made `tuple[float, ...] | None` and an equivalent spelling of the same type a test
    # failure, while a genuinely unresolvable annotation would have passed.
    for name in ("Channel", "Pocket", "Slot"):
        hints = typing.get_type_hints(getattr(recess_facade, name))
        assert "width_axis" in hints
    assert typing.get_type_hints(recognition.recognise_slots)

    recess_records = importlib.import_module("quiddity._recess_records")
    recess_features = importlib.import_module("quiddity._recess_features")
    recess_patterns = importlib.import_module("quiddity._recess_patterns")
    for name in ("Slot", "SlotArray", "SlotGrid"):
        assert getattr(recognition, name) is getattr(recess_facade, name)
        assert getattr(recess_facade, name) is getattr(recess_records, name)
        assert getattr(recognition, name).__module__ == "quiddity.slots"
    for name in ("recognise_slots",):
        assert getattr(recognition, name) is getattr(recess_facade, name)
        assert getattr(recess_facade, name) is getattr(recess_features, name)
        assert getattr(recognition, name).__module__ == "quiddity.slots"
    for name in ("recognise_slot_patterns",):
        assert getattr(recognition, name) is getattr(recess_facade, name)
        assert getattr(recess_facade, name) is getattr(recess_patterns, name)
        assert getattr(recognition, name).__module__ == "quiddity.slots"
    for name in (
        "BoltCircle",
        "BossRecord",
        "CounterBore",
        "HoleRecord",
        "HoleSpec",
        "LinearArray",
        "RectGrid",
    ):
        assert getattr(recognition, name).__module__ == "quiddity._features"

    moved_records = (
        "BoltCircle",
        "BossRecord",
        "Channel",
        "CounterBore",
        "HoleRecord",
        "HoleSpec",
        "LinearArray",
        "Pocket",
        "PocketArray",
        "PocketGrid",
        "RectGrid",
        "Slot",
        "SlotArray",
        "SlotGrid",
    )
    for name in moved_records:
        owner = (
            recess_facade if name in {"Channel", "Pocket", "PocketArray", "PocketGrid"}
            else recognition
        )
        assert typing.get_type_hints(getattr(owner, name))


def test_recess_families_keep_one_shared_face_inventory_and_patterns_are_pure() -> None:
    core = ast.parse(
        (PACKAGE / "_recess_core.py").read_text(encoding="utf-8"),
        filename="_recess_core.py",
    )
    functions = {node.name: node for node in core.body if isinstance(node, ast.FunctionDef)}
    for name in ("_slot_proposals_one", "_pocket_proposals_one", "_channel_proposals_one"):
        scans = [
            node
            for node in ast.walk(functions[name])
            if isinstance(node, ast.Call)
            and isinstance(node.func, ast.Name)
            and node.func.id == "_planar_faces"
        ]
        assert len(scans) == 1, name

    for module_name in ("_hole_patterns.py", "_pattern_geometry.py", "_recess_patterns.py"):
        tree = ast.parse((PACKAGE / module_name).read_text(encoding="utf-8"), filename=module_name)
        topology_reads = [
            node.attr
            for node in ast.walk(tree)
            if isinstance(node, ast.Attribute) and node.attr in {"edges", "faces", "solids"}
        ]
        assert topology_reads == [], module_name
