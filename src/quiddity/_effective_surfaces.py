# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Run-owned effective analytic facts without replacing original topology.

This is the neutral F1 seam from epic 0004.  Every lookup is keyed by the exact
``FaceNode`` issued by one ``FaceGraph`` and every returned value retains that node.
The original face remains authoritative for topology, orientation and evidence.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from enum import Enum
from typing import Protocol, TypeAlias

import OCP
from build123d import Vertex
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_Copy
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.GeomAbs import (
    GeomAbs_BezierSurface,
    GeomAbs_BSplineSurface,
    GeomAbs_Cone,
    GeomAbs_Cylinder,
    GeomAbs_Plane,
    GeomAbs_Sphere,
    GeomAbs_Torus,
)
from OCP.gp import gp_Cone, gp_Cylinder, gp_Pln, gp_Pnt, gp_Sphere, gp_Vec
from OCP.ShapeAnalysis import ShapeAnalysis_CanonicalRecognition, ShapeAnalysis_Surface
from OCP.Standard import Standard_Failure
from OCP.TopAbs import TopAbs_IN, TopAbs_OUT
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS

from quiddity._adjacency import FaceGraph, FaceNode, GraphRunToken, SolidRef
from quiddity._analytic_surfaces import SurfaceKind as SurfaceKind
from quiddity._analytic_surfaces import native_primitive, validated_parameters
from quiddity._geometry import COORD_FLOOR
from quiddity._typing import FaceLike, Part

_RECOVERY_REL = 1e-6
_SUPPORTED_OCCT_CERTIFICATE_VERSIONS = frozenset({"7.9.3.1"})
_CERTIFICATE_AUTHORITY = "OCCT ShapeAnalysis_CanonicalRecognition face maximum-distance contract"
_MATERIAL_SIDE_AUTHORITY = "original-face triangulation and OCCT closed-solid side probes"
_MATERIAL_PROBE_REL = 1e-4
_MATERIAL_PROBE_CAP = 0.02
_MATERIAL_CLASSIFIER_TOLERANCE = COORD_FLOOR
_MATERIAL_MIN_SAMPLES = 2
_MATERIAL_MAX_SAMPLES = 4


class SurfaceProvenance(Enum):
    NATIVE = "native"
    RECOVERED = "recovered"


class OrientationCapability(Enum):
    NATIVE_ORIENTED = "native-oriented"
    RECOVERED_UNORIENTED = "recovered-unoriented"


class SurfaceRefusalReason(Enum):
    UNSUPPORTED_KIND = "unsupported-kind"
    UNSUPPORTED_TORUS_RECOVERY = "unsupported-torus-recovery"
    FIT_UNAVAILABLE = "fit-unavailable"
    INVALID_INPUT = "invalid-input"
    INVALID_RESULT = "invalid-result"
    RESIDUAL_EXCEEDED = "residual-exceeded"
    AMBIGUOUS_PRIMITIVE = "ambiguous-primitive"
    UNSUPPORTED_OCCT_CONTRACT = "unsupported-occt-contract"


class SurfaceReaderDisposition(Enum):
    RAW_TOPOLOGY = "raw-topology"
    PENDING_MIGRATION = "pending-migration"
    ORIENTATION_DEFERRED = "orientation-deferred"
    TORUS_DEFERRED = "torus-deferred"
    MIGRATED_EFFECTIVE = "migrated-effective"


class MaterialSideRefusalReason(Enum):
    """Closed reasons why a surface cannot acquire a material-side certificate."""

    SURFACE_UNAVAILABLE = "surface-unavailable"
    UNSUPPORTED_PRIMITIVE = "unsupported-primitive"
    OWNER_UNPROVEN = "owner-unproven"
    SAMPLE_UNAVAILABLE = "sample-unavailable"
    SAMPLE_NEAR_BOUNDARY = "sample-near-boundary"
    DIFFERENTIAL_DEGENERATE = "differential-degenerate"
    PROBE_INDETERMINATE = "probe-indeterminate"
    SAMPLES_DISAGREE = "samples-disagree"


# Complete baseline roster of modules making face-surface classification decisions. Tests derive
# the source-side set independently so adding another raw reader fails visibly. The rationale is
# mandatory; a disposition is not permission to leave an undocumented acceptance path forever.
SURFACE_READER_ROSTER: dict[str, tuple[SurfaceReaderDisposition, str]] = {
    "_open_channel_section": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "open support projection requires original planar source patches",
    ),
    "_adjacency": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "base graph caches original surface/topology facts; it cannot import this layer",
    ),
    "_body_geometry": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "private F6 descriptor serializes graph-authorized original analytic boundaries",
    ),
    "angled_steps": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge geom_type validates a split terminal boundary, not a face surface",
    ),
    "paired_ramp_steps": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "shared edge geom_type validates the original linear ramp intersection",
    ),
    "through_steps": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge geom_type validates rectangular boundaries and their complete linear seam",
    ),
    "circular_blind_steps": (
        SurfaceReaderDisposition.MIGRATED_EFFECTIVE,
        "planar membership and native/recovered cylinder provenance use the run-owned query",
    ),
    "blends": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "ADR 0013 authorizes native torus parameters, UV extent and oriented differential",
    ),
    "_bevel": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar bevel family gate"),
    "_cylinder_substrate": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "native compatibility fast path stays raw; non-native cylinders use effective facts",
    ),
    "_hole_features": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "hole termination distinguishes cones and toroidal blends",
    ),
    "_recess_core": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar recess boundary gate"),
    "_recess_faces": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "planar wall normals participate in material-side geometry",
    ),
    "_rings": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar ring membership gate"),
    "_section_passages": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F4b neutral line-wall ring and planar membership grammar",
    ),
    "chamfers": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "cone and neighbouring-plane directions participate in the family predicate",
    ),
    "countersinks": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "cone/cylinder axes and opening direction participate in the family predicate",
    ),
    "fillets": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "toroidal fillets are outside the four-primitive F1 seam",
    ),
    "inspection": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "supported F7 inspection owns its bounded trimmed-surface anchor projection",
    ),
    "experimental_geometry": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "experimental graph facade projects the graph-owned planar query",
    ),
    "frames": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "part-frame inference reads original analytic direction evidence",
    ),
    "flats": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar flat family gate"),
    "grooves": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "groove evidence includes conical and toroidal surfaces",
    ),
    "levels": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar level and step gates"),
    "plates": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar plate family gate"),
    "pads": (
        SurfaceReaderDisposition.MIGRATED_EFFECTIVE,
        "plane membership uses run-owned effective facts; top side uses a separate certificate",
    ),
    "polygonal_bosses": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "planar polygonal-side membership gate",
    ),
    "prismatic_pockets": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "original planar opening, wall and floor roles in the bounded cavity proof",
    ),
    "_section_recess_geometry": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "section-recess recognition intentionally requires audited original-face anatomy",
    ),
    "_entry_treatments": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "finite treatment cells require original planar bevel and stock footprints",
    ),
    "_corner_section": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "corner projection requires original rectangular faces and an original exterior mouth",
    ),
    "profiled_bores": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar profile-face gate"),
    "repeating_profiles": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge geom_type classifies profile boundary curves, not face surfaces",
    ),
    "round_bottom_slots": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "native cylinder and planar U-section grammar; recovered surfaces are not accepted",
    ),
    "rectangular_blind_slots": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "native planar rectangular U-section grammar; recovered surfaces are not accepted",
    ),
    "edge_open_prismatic_recesses": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "truthful open-chain recognition requires original planar walls and straight edges",
    ),
    "edge_open_circular_recesses": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "truthful open-chain recognition requires original planar, cylindrical and edge curves",
    ),
}

# Function/role/ordinal identities freeze every decision without depending on source line numbers.
# Every site has its own disposition and rationale, including mixed modules whose reads cannot be
# truthfully covered by one module-level label.
SURFACE_READER_SITES: dict[str, tuple[SurfaceReaderDisposition, str]] = {
    "_open_channel_section:_supports:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "verify complete source-face support area for each physical wall patch",
    ),
    "_open_channel_section:prove_open_channel:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "derive the opposed wall planes from original defining evidence",
    ),
    "_corner_section:prove_corner_section:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "prove rectangular physical floor and wall faces before issuing an open chain",
    ),
    "_corner_section:prove_corner_section:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "prove exterior planar mouth faces rather than assuming the opposite end is open",
    ),
    "_section_recess_geometry:_cylinder:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser reads native cylinder parameters from the audited obround wall",
    ),
    "_section_recess_geometry:_edge_sweep:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser requires the original semicircular floor boundary",
    ),
    "_section_recess_geometry:_one_obround_candidate:graph_surface:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser classifies the two original cylindrical walls",
    ),
    "_section_recess_geometry:_one_obround_candidate:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser classifies the two original planar walls",
    ),
    "_section_recess_geometry:_one_obround_candidate:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser requires an original planar mouth",
    ),
    "_section_recess_geometry:_candidates:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser seeds only from an original planar floor",
    ),
    "_section_recess_geometry:_polygonal_section:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser requires the observed polygonal floor boundary to be straight",
    ),
    "_section_recess_geometry:_mixed_floor_section:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "require an observed native mixed line/circle floor boundary",
    ),
    "_entry_treatments:prove_entry_treatments:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "prove native planar stock termination of a finite entry treatment",
    ),
    "_entry_treatments:prove_entry_treatments:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "require an observed native planar bevel before finite cell reconstruction",
    ),
    "_section_recess_geometry:_mixed_floor_section:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "encode the original circular edge sweep rather than a chord",
    ),
    "_section_recess_geometry:_one_mixed_candidate:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "require observed planar stock termination for exact swept supports",
    ),
    "_section_recess_geometry:_one_polygonal_candidate:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser requires original planar polygonal wall supports",
    ),
    "_section_recess_geometry:_one_polygonal_candidate:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "recogniser requires one original planar exterior mouth",
    ),
    "edge_open_circular_recesses:_segment:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "retain the exact physical floor-to-wall boundary curve kind",
    ),
    "edge_open_circular_recesses:_segment:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "project an original circular boundary without reconstructing a missing arc",
    ),
    "edge_open_circular_recesses:recognise_edge_open_circular_pockets:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "require the two physically present original cylindrical supports",
    ),
    "edge_open_circular_recesses:recognise_edge_open_circular_pockets:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "verify the alternating original planar/cylindrical chain",
    ),
    "edge_open_circular_recesses:recognise_edge_open_circular_pockets:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "floor seed is deliberately restricted to an original planar face",
    ),
    "edge_open_circular_recesses:recognise_edge_open_circular_pockets:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "straight wall supports are deliberately restricted to original planar faces",
    ),
    "edge_open_prismatic_recesses:_shared_segment:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "require exact physical straight floor-to-wall boundary segments",
    ),
    "edge_open_prismatic_recesses:recognise_edge_open_prismatic_recesses:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "floor seed is deliberately restricted to an original planar face",
    ),
    "edge_open_prismatic_recesses:recognise_edge_open_prismatic_recesses:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "wall supports are deliberately restricted to original planar faces",
    ),
    "blends:_native_torus:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "read native torus parameters for the ADR 0013 circular-path contract",
    ),
    "blends:_covers_complete_circle:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "read the native torus UV domain to require a complete circular path",
    ),
    "blends:_torus_side:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "read the oriented native torus differential to derive material-side curvature",
    ),
    "rectangular_blind_slots:_recognise_one:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "cheap graph-owned native planar concave-neighbour gate",
    ),
    "rectangular_blind_slots:_recognise_one:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "graph-owned native planar cap-region gate",
    ),
    "round_bottom_slots:_cylinder_surface:adaptor:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "native exact-cylinder grammar for the bounded U-section family",
    ),
    "round_bottom_slots:_principal_rectangle:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "original boundary-line grammar for a hole-free floor region",
    ),
    "round_bottom_slots:_boundary_runs:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "original boundary line/circle grammar refusal",
    ),
    "round_bottom_slots:_boundary_runs:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "original boundary line/circle run grouping",
    ),
    "round_bottom_slots:_boundary_runs:geom_type:3": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "original boundary line-run continuity proof",
    ),
    "round_bottom_slots:_boundary_runs:geom_type:4": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "original boundary circle-run continuity proof",
    ),
    "round_bottom_slots:_recognise_one:graph_surface:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "cheap graph-owned native-cylinder cap-neighbour gate",
    ),
    "round_bottom_slots:_recognise_one:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "graph-owned native planar side-region gate",
    ),
    "_adjacency:surface:adaptor:1": (SurfaceReaderDisposition.RAW_TOPOLOGY, "base surface cache"),
    "_adjacency:is_planar:graph_surface:1": (SurfaceReaderDisposition.RAW_TOPOLOGY, "base query"),
    "_adjacency:_normal_at:adaptor:1": (SurfaceReaderDisposition.RAW_TOPOLOGY, "base normal"),
    "_adjacency:_native_continuation:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F2 original native analytic continuation fact",
    ),
    "_adjacency:_native_continuation:adaptor:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F2 paired original native analytic continuation fact",
    ),
    "_adjacency:_normal_curvature:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F2 original-surface second fundamental form",
    ),
    "_adjacency:frame_points_outward:adaptor:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "original-face material-side query waits for F2",
    ),
    "_adjacency:axis_aligned_axis:adaptor:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "primitive-axis query",
    ),
    "_body_geometry:_edge_geometry:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 bounded analytic edge-kind label",
    ),
    "_body_geometry:_edge_geometry:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 bounded line grammar gate",
    ),
    "_body_geometry:_edge_geometry:geom_type:3": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 bounded circle grammar gate",
    ),
    "_body_geometry:_edge_geometry:geom_type:4": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 circle-radius projection gate",
    ),
    "_body_geometry:matching_boundary_for_solid:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 schema-three canonical cylinder pcurve gauge",
    ),
    "_body_geometry:_face_geometry:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 graph-authorized plane/cylinder parameter projection",
    ),
    "_body_geometry:_face_geometry:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 bounded plane grammar gate",
    ),
    "_body_geometry:_face_geometry:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 bounded cylinder grammar gate",
    ),
    "_body_geometry:_face_geometry:geom_type:3": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F6 unsupported-surface refusal label",
    ),
    "_section_passages:_canonical_run:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F4b bounded straight junction-edge grammar",
    ),
    "_section_passages:_line_section:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F4b two-mouth fallback requires an original straight-edged polygon",
    ),
    "_section_passages:_mouth_regions:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F4b opening faces must be original planes",
    ),
    "_section_passages:_mouth_regions:is_planar:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F4b two-mouth fallback is limited to original planar wall seeds",
    ),
    "_section_passages:_wall_run:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F4b nonparallel-end run proof is limited to original planar walls",
    ),
    "_section_passages:section_ring_proposals:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "F4b original planar wall-cycle membership",
    ),
    "inspection:_surface_anchor:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "bounded inspection anchor over the same graph-owned original face",
    ),
    "experimental_geometry:is_planar:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "facade projection of the graph-owned planar query",
    ),
    "frames:infer_part_frame:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "part-frame inference reads original plane and cylinder direction evidence",
    ),
    "_bevel:classify_bevel:adaptor:1": (SurfaceReaderDisposition.PENDING_MIGRATION, "plane gate"),
    "_cylinder_substrate:analyse_cylinders:adaptor:1": (
        SurfaceReaderDisposition.MIGRATED_EFFECTIVE,
        "native fast path plus run-owned recovered cylinder and radial-side query",
    ),
    "_hole_features:_classify_end_uncached:adaptor:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "end plane/sphere/cylinder classification uses oriented topology",
    ),
    "_hole_features:_classify_end_uncached:adaptor:2": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "neighbour plane/cylinder classification uses oriented topology",
    ),
    "_hole_features:_shared_transition:adaptor:1": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "cone-or-torus transition rule includes unsupported torus",
    ),
    "_recess_core:_uninterrupted_long_span:is_planar:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "planar recess boundary gate",
    ),
    "_recess_core:_has_smooth_depth_closure:is_planar:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "existing AAG surface-kind gate for a curved Slot depth closure seed",
    ),
    "_recess_core:_has_smooth_depth_closure:is_planar:2": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "existing AAG surface-kind gate while expanding one curved closure region",
    ),
    "_recess_core:_bounds_one_void:is_planar:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "planar recess boundary gate",
    ),
    "_recess_faces:_is_wall:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge curve kind, not a face surface",
    ),
    "_recess_faces:_planar_faces:is_planar:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "planar wall normal participates in material-side geometry",
    ),
    "_recess_faces:_cylinder_faces:adaptor:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "cylindrical recess boundary gate",
    ),
    "_rings:rings:is_planar:1": (SurfaceReaderDisposition.PENDING_MIGRATION, "planar ring gate"),
    "prismatic_pockets:_one_ended_regions:is_planar:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "principal-axis cavity opening must be an original plane",
    ),
    "prismatic_pockets:_one_ended_regions:is_planar:2": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "wall-cycle reconstruction uses original planar supports",
    ),
    "prismatic_pockets:_floor_section:geom_type:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "straight original floor boundary gate",
    ),
    "prismatic_pockets:_floor_seeded_regions:is_planar:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "principal-plane intact floor seed",
    ),
    "prismatic_pockets:_floor_seeded_regions:is_planar:2": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "original planar wall support at each floor edge",
    ),
    "angled_steps:_effective_linear_sides:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge curve kind, not a face surface",
    ),
    "paired_ramp_steps:_candidate:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "shared edge curve kind, not a face surface",
    ),
    "paired_ramp_steps:_read_ramp:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "the defining ramp must remain one original planar face with exact graph provenance",
    ),
    "paired_ramp_steps:_axis_terminal:is_planar:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "the contract requires one original planar terminal; recovered regions are out of scope",
    ),
    "through_steps:_four_principal_runs:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge curve kind validates a rectangular boundary",
    ),
    "through_steps:_shared_run_is_complete:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge curve kind validates a complete linear defining seam",
    ),
    "chamfers:recognise_chamfers:adaptor:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "cone family gate uses oriented neighbours",
    ),
    "chamfers:recognise_chamfers:adaptor:2": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "cone parameter read uses oriented frame",
    ),
    "chamfers:recognise_chamfers:adaptor:3": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "neighbour plane direction uses oriented frame",
    ),
    "countersinks:cone_rims:adaptor:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "cone rim direction uses oriented frame",
    ),
    "countersinks:_discover_countersinks:adaptor:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "cylinder direction uses oriented frame",
    ),
    "countersinks:_discover_countersinks:adaptor:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "original cone axis is paired with the original face material-side normal",
    ),
    "fillets:_discover_fillets:adaptor:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "analytic anchor uses oriented frame",
    ),
    "fillets:_discover_fillets:adaptor:2": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "torus family gate",
    ),
    "fillets:_discover_fillets:adaptor:3": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "torus parameter read",
    ),
    "fillets:_discover_fillets:adaptor:4": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "neighbour plane/sphere rule uses oriented topology",
    ),
    "flats:_discover_flats:adaptor:1": (SurfaceReaderDisposition.PENDING_MIGRATION, "plane gate"),
    "grooves:_cone_joins:adaptor:1": (
        SurfaceReaderDisposition.ORIENTATION_DEFERRED,
        "cone-axis join uses oriented frame",
    ),
    "grooves:transition:adaptor:1": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "torus transition parameter read",
    ),
    "grooves:_torus_joined:adaptor:1": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "torus adjacency family gate",
    ),
    "grooves:recognise_grooves:geom_type:1": (
        SurfaceReaderDisposition.TORUS_DEFERRED,
        "torus family applicability gate",
    ),
    "levels:_face_level_proposals_one:adaptor:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "planar face-level gate",
    ),
    "levels:_riser_proposals_one:adaptor:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "planar riser gate",
    ),
    "plates:_plate_proposals:adaptor:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "planar plate inventory gate",
    ),
    "plates:_plate_proposals:adaptor:2": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "plate normal/offset read",
    ),
    "plates:_oriented_cross_area:adaptor:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "original planar support location establishes a body-envelope direction",
    ),
    "polygonal_bosses:_cap_coordinate:is_planar:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "principal-axis planar cap gate",
    ),
    "polygonal_bosses:_principal_side_faces:is_planar:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "principal-axis planar side gate",
    ),
    "profiled_bores:principal_boundary_plane:geom_type:1": (
        SurfaceReaderDisposition.PENDING_MIGRATION,
        "planar profile face gate",
    ),
    "profiled_bores:lateral:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "Double-D lateral wall component plane/cylinder gate",
    ),
    "profiled_bores:lateral:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "Double-D lateral wall role geometry",
    ),
    "profiled_bores:lateral:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "Double-D planar chord-wall branch",
    ),
    "profiled_bores:support:adaptor:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "Double-D logical wall support identity",
    ),
    "profiled_bores:support:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "Double-D plane/cylinder support branch",
    ),
    "profiled_bores:double_d_profile:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "line boundary edge gate",
    ),
    "profiled_bores:double_d_profile:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "circle boundary edge gate",
    ),
    "repeating_profiles:_sample_wire:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge curve kind extraction",
    ),
    "repeating_profiles:_sample_wire:geom_type:2": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "edge curve kind fallback",
    ),
    "repeating_profiles:_common_circle_centre:geom_type:1": (
        SurfaceReaderDisposition.RAW_TOPOLOGY,
        "circular boundary edge proof",
    ),
}


@dataclass(frozen=True, slots=True)
class RecoveryCertificate:
    occt_version: str
    authority: str
    maximum_distance_bound: float


@dataclass(frozen=True, slots=True)
class AnalyticSurfaceFact:
    node: FaceNode
    kind: SurfaceKind
    provenance: SurfaceProvenance
    orientation: OrientationCapability
    parameters: tuple[float, ...]
    requested_tolerance: float
    kernel_reported_gap: float
    certificate: RecoveryCertificate | None


@dataclass(frozen=True, slots=True)
class RefusedSurfaceFact:
    node: FaceNode
    reason: SurfaceRefusalReason


EffectiveSurfaceFact: TypeAlias = AnalyticSurfaceFact | RefusedSurfaceFact


@dataclass(frozen=True, slots=True)
class MaterialSideCertificate:
    """Independent proof of which candidate normal points out of one solid.

    ``candidate_outward_sign`` applies to the primitive's candidate normal at every retained
    sample.  A plane has one global candidate direction, retained in ``outward`` for compatibility;
    for a cylinder ``outward`` is the first retained sample normal and ``outward_samples`` carries
    every position-dependent radial normal.  Consumers must use the sign, not that sample vector,
    to classify a cylinder as internal or external.
    """

    node: FaceNode
    solid: SolidRef
    outward: tuple[float, float, float]
    candidate_outward_sign: int
    outward_samples: tuple[tuple[float, float, float], ...]
    sample_points: tuple[tuple[float, float, float], ...]
    probe_distance: float
    classifier_tolerance: float
    original_orientation: int
    authority: str


@dataclass(frozen=True, slots=True)
class SurfaceUseRefusal:
    """A closed refusal to issue one provenance-bearing surface dependency."""

    node: FaceNode
    reason: MaterialSideRefusalReason


@dataclass(frozen=True, slots=True)
class _SurfaceUseSnapshot:
    node: FaceNode
    surface: AnalyticSurfaceFact
    material_side: MaterialSideCertificate | None
    surface_state: tuple[object, ...]
    material_state: tuple[object, ...] | None


class SurfaceUse:
    """Opaque, run-issued surface provenance retained by Candidate evidence."""

    __slots__ = ("__authority",)

    def __init__(self) -> None:
        raise TypeError("surface uses are issued by an effective-face query")

    @property
    def node(self) -> FaceNode:
        return _surface_use_authority(self)._validate(self).node

    @property
    def surface(self) -> AnalyticSurfaceFact:
        return _surface_use_authority(self)._validate(self).surface

    @property
    def material_side(self) -> MaterialSideCertificate | None:
        return _surface_use_authority(self)._validate(self).material_side


SurfaceUseResult: TypeAlias = SurfaceUse | SurfaceUseRefusal


def _surface_state(surface: AnalyticSurfaceFact) -> tuple[object, ...]:
    recovery = surface.certificate
    return (
        surface.node,
        surface.kind,
        surface.provenance,
        surface.orientation,
        surface.parameters,
        surface.requested_tolerance,
        surface.kernel_reported_gap,
        None
        if recovery is None
        else (
            recovery.occt_version,
            recovery.authority,
            recovery.maximum_distance_bound,
        ),
    )


def _material_state(certificate: MaterialSideCertificate) -> tuple[object, ...]:
    return (
        certificate.node,
        certificate.solid,
        certificate.outward,
        certificate.candidate_outward_sign,
        certificate.outward_samples,
        certificate.sample_points,
        certificate.probe_distance,
        certificate.classifier_tolerance,
        certificate.original_orientation,
        certificate.authority,
    )


class EffectiveSurfaceQuery(Protocol):
    @property
    def run_token(self) -> GraphRunToken: ...

    def fact(self, node: FaceNode) -> EffectiveSurfaceFact: ...


class EffectiveFaceSurfaceQuery(Protocol):
    """Restricted face-keyed projection for explicitly migrated family consumers."""

    @property
    def run_token(self) -> GraphRunToken: ...

    def fact(self, face: FaceLike) -> EffectiveSurfaceFact: ...

    def use(self, face: FaceLike, *, material_side: bool = False) -> SurfaceUseResult: ...


def cylinder_surface_dependency(
    effective: EffectiveFaceSurfaceQuery, face: FaceLike
) -> SurfaceUseResult:
    """Issue the provenance needed to consume a native or recovered cylinder.

    Native topology already supplies orientation, while a recovered primitive needs the
    independently certified material side. Hole and circular-step discovery share this neutral
    distinction; neither family owns it.
    """

    fact = effective.fact(face)
    recovered = (
        isinstance(fact, AnalyticSurfaceFact) and fact.provenance is SurfaceProvenance.RECOVERED
    )
    return effective.use(face, material_side=recovered)


def _surface_use_authority(surface_use: SurfaceUse) -> _EffectiveFaceSurfaces:
    authority = getattr(surface_use, "_SurfaceUse__authority", None)
    if not isinstance(authority, _EffectiveFaceSurfaces):
        raise ValueError("surface-use authority was mutated")
    return authority


def _validate_surface_use(surface_use: SurfaceUse, graph: FaceGraph) -> _SurfaceUseSnapshot:
    """Validate one opaque surface dependency against the Candidate's graph run."""

    authority = _surface_use_authority(surface_use)
    if authority._graph is not graph or authority.run_token is not graph.run_token:
        raise ValueError("surface use belongs to another graph run")
    return authority._validate(surface_use)


def _triangle_samples(
    face: FaceLike, *, probe_distance: float
) -> tuple[tuple[float, float, float], ...] | MaterialSideRefusalReason:
    """Choose deterministic interior triangle centroids with explicit trim clearance."""

    try:
        # Meshing a TopoDS_Face attaches triangulation to that face.  Recognition
        # is observational, so work on an independent geometry copy and leave the
        # graph-owned input topology (and any caller-owned mesh state) untouched.
        mesh_face = TopoDS.Face_s(BRepBuilderAPI_Copy(face.wrapped, True, False).Shape())
        BRepMesh_IncrementalMesh(
            mesh_face,
            max(probe_distance, COORD_FLOOR),
            False,
            0.5,
            False,
        )
        location = TopLoc_Location()
        triangulation = BRep_Tool.Triangulation_s(mesh_face, location)
        if triangulation is None:
            return MaterialSideRefusalReason.SAMPLE_UNAVAILABLE
        transform = location.Transformation()
        nodes = tuple(
            triangulation.Node(at).Transformed(transform)
            for at in range(1, triangulation.NbNodes() + 1)
        )
        candidates: list[tuple[float, tuple[float, float, float]]] = []
        for at in range(1, triangulation.NbTriangles() + 1):
            indices = triangulation.Triangle(at).Get()
            points = tuple(nodes[index - 1] for index in indices)
            ab = gp_Vec(points[0], points[1])
            ac = gp_Vec(points[0], points[2])
            area2 = float(ab.Crossed(ac).Magnitude())
            if not math.isfinite(area2) or area2 <= COORD_FLOOR * COORD_FLOOR:
                continue
            point = tuple(
                math.fsum(getattr(vertex, axis)() for vertex in points) / 3.0
                for axis in ("X", "Y", "Z")
            )
            candidates.append((area2, (point[0], point[1], point[2])))
        candidates.sort(key=lambda item: (-item[0], item[1]))
        if not candidates:
            return MaterialSideRefusalReason.SAMPLE_UNAVAILABLE
        cleared = []
        edges = tuple(face.edges())
        for _area, point in candidates:
            vertex = Vertex(*point)
            clearance = min((float(vertex.distance_to(edge)) for edge in edges), default=0.0)
            if math.isfinite(clearance) and clearance >= 4.0 * probe_distance:
                cleared.append(point)
            if len(cleared) == _MATERIAL_MAX_SAMPLES:
                break
        if len(cleared) < _MATERIAL_MIN_SAMPLES:
            return MaterialSideRefusalReason.SAMPLE_NEAR_BOUNDARY
        return tuple(cleared)
    except (Standard_Failure, RuntimeError, ValueError):
        return MaterialSideRefusalReason.SAMPLE_UNAVAILABLE


def _regular_surface_differential(
    face: FaceLike,
    point: tuple[float, float, float],
    direction: tuple[float, float, float],
) -> bool:
    """Require a non-degenerate original differential aligned with a candidate normal."""

    try:
        uv = ShapeAnalysis_Surface(BRep_Tool.Surface_s(face.wrapped)).ValueOfUV(
            gp_Pnt(*point), COORD_FLOOR
        )
        here, along_u, along_v = gp_Pnt(), gp_Vec(), gp_Vec()
        BRepAdaptor_Surface(face.wrapped).D1(uv.X(), uv.Y(), here, along_u, along_v)
        projection_gap = math.sqrt(
            math.fsum(
                (actual - expected) ** 2
                for actual, expected in zip((here.X(), here.Y(), here.Z()), point, strict=True)
            )
        )
        if not math.isfinite(projection_gap) or projection_gap > recovery_tolerance(face):
            return False
        cross = along_u.Crossed(along_v)
        magnitude = float(cross.Magnitude())
        if not math.isfinite(magnitude) or magnitude <= COORD_FLOOR * COORD_FLOOR:
            return False
        cross.Scale(1.0 / magnitude)
        alignment = abs(
            cross.X() * direction[0] + cross.Y() * direction[1] + cross.Z() * direction[2]
        )
        return math.isfinite(alignment) and alignment >= 1.0 - 1e-9
    except (Standard_Failure, RuntimeError, ValueError):
        return False


# Kept as a private compatibility alias for tests and downstream source users while the authority
# widens from its original plane-only contract.
_regular_plane_differential = _regular_surface_differential


def _candidate_normal(
    surface: AnalyticSurfaceFact,
    point: tuple[float, float, float],
) -> tuple[float, float, float] | None:
    """Return the primitive's canonical local normal, without assigning material side."""

    if surface.kind is SurfaceKind.PLANE:
        return (surface.parameters[0], surface.parameters[1], surface.parameters[2])
    if surface.kind is not SurfaceKind.CYLINDER:
        return None
    axis_point = surface.parameters[:3]
    axis_direction = surface.parameters[3:6]
    relative = tuple(
        coordinate - origin for coordinate, origin in zip(point, axis_point, strict=True)
    )
    along = math.fsum(
        coordinate * direction
        for coordinate, direction in zip(relative, axis_direction, strict=True)
    )
    radial = tuple(
        coordinate - along * direction
        for coordinate, direction in zip(relative, axis_direction, strict=True)
    )
    magnitude = math.sqrt(math.fsum(component * component for component in radial))
    radius = surface.parameters[6]
    if (
        not math.isfinite(magnitude)
        or magnitude <= COORD_FLOOR
        or abs(magnitude - radius) > max(surface.requested_tolerance, COORD_FLOOR)
    ):
        return None
    return tuple(component / magnitude for component in radial)  # type: ignore[return-value]


def _regular_cylinder_sample(
    face: FaceLike,
    seed: tuple[float, float, float],
    surface: AnalyticSurfaceFact,
    *,
    probe_distance: float,
) -> tuple[tuple[float, float, float], tuple[float, float, float]] | None:
    """Project a mesh seed to the original curved face and prove its recovered radial normal."""

    try:
        uv = ShapeAnalysis_Surface(BRep_Tool.Surface_s(face.wrapped)).ValueOfUV(
            gp_Pnt(*seed), max(probe_distance, COORD_FLOOR)
        )
        here, along_u, along_v = gp_Pnt(), gp_Vec(), gp_Vec()
        BRepAdaptor_Surface(face.wrapped).D1(uv.X(), uv.Y(), here, along_u, along_v)
        point = (here.X(), here.Y(), here.Z())
        direction = _candidate_normal(surface, point)
        if direction is None or not _regular_surface_differential(face, point, direction):
            return None
        clearance = min(
            (float(Vertex(*point).distance_to(edge)) for edge in face.edges()), default=0.0
        )
        if not math.isfinite(clearance) or clearance < 4.0 * probe_distance:
            return None
        return point, direction
    except (Standard_Failure, RuntimeError, ValueError):
        return None


class _EffectiveFaceSurfaces:
    """Bind effective facts and issued use-provenance to exact original faces."""

    def __init__(self, graph: FaceGraph, surfaces: EffectiveSurfaceQuery) -> None:
        if graph.run_token is not surfaces.run_token:
            raise ValueError("face graph and effective surfaces belong to different runs")
        self._graph = graph
        self._surfaces = surfaces
        self._uses: dict[tuple[FaceNode, bool], SurfaceUseResult] = {}
        self._issued: dict[int, tuple[SurfaceUse, _SurfaceUseSnapshot]] = {}

    @property
    def run_token(self) -> GraphRunToken:
        return self._graph.run_token

    def fact(self, face: FaceLike) -> EffectiveSurfaceFact:
        return self._surfaces.fact(self._graph.require_node(face))

    def use(self, face: FaceLike, *, material_side: bool = False) -> SurfaceUseResult:
        """Issue an immutable dependency, certifying plane material side when requested."""

        node = self._graph.require_node(face)
        key = (node, material_side)
        cached = self._uses.get(key)
        if cached is not None:
            return cached
        surface = self._surfaces.fact(node)
        if not isinstance(surface, AnalyticSurfaceFact):
            result: SurfaceUseResult = SurfaceUseRefusal(
                node, MaterialSideRefusalReason.SURFACE_UNAVAILABLE
            )
        elif material_side:
            certificate = (
                self._certify_plane(node, surface)
                if surface.kind is SurfaceKind.PLANE
                else self._certify_material_side(node, surface)
            )
            if isinstance(certificate, MaterialSideRefusalReason):
                result = SurfaceUseRefusal(node, certificate)
            else:
                result = self._issue(node, surface, certificate)
        else:
            result = self._issue(node, surface, None)
        self._uses[key] = result
        return result

    def _issue(
        self,
        node: FaceNode,
        surface: AnalyticSurfaceFact,
        material_side: MaterialSideCertificate | None,
    ) -> SurfaceUse:
        use = object.__new__(SurfaceUse)
        object.__setattr__(use, "_SurfaceUse__authority", self)
        snapshot = _SurfaceUseSnapshot(
            node,
            surface,
            material_side,
            _surface_state(surface),
            _material_state(material_side) if material_side is not None else None,
        )
        self._issued[id(use)] = (use, snapshot)
        return use

    def _validate(self, use: SurfaceUse) -> _SurfaceUseSnapshot:
        issued = self._issued.get(id(use))
        if issued is None or issued[0] is not use:
            raise ValueError("surface use was not issued by this query")
        snapshot = issued[1]
        current = self._surfaces.fact(snapshot.node)
        if (
            current is not snapshot.surface
            or current.node is not snapshot.node
            or _surface_state(current) != snapshot.surface_state
        ):
            raise ValueError("surface use no longer matches its issued fact")
        certificate = snapshot.material_side
        if certificate is not None and (
            certificate.node is not snapshot.node
            or self._graph.common_valid_solid((snapshot.node,)) is not certificate.solid
            or _material_state(certificate) != snapshot.material_state
            or int(self._graph.face(snapshot.node).wrapped.Orientation())
            != certificate.original_orientation
        ):
            raise ValueError("material-side certificate no longer matches its graph owner")
        return snapshot

    def _certify_material_side(
        self, node: FaceNode, surface: AnalyticSurfaceFact
    ) -> MaterialSideCertificate | MaterialSideRefusalReason:
        if surface.kind not in (SurfaceKind.PLANE, SurfaceKind.CYLINDER):
            return MaterialSideRefusalReason.UNSUPPORTED_PRIMITIVE
        solid = self._graph.common_valid_solid((node,))
        if solid is None:
            return MaterialSideRefusalReason.OWNER_UNPROVEN
        face = self._graph.face(node)
        try:
            nominal = recovery_nominal(face)
        except ValueError:
            return MaterialSideRefusalReason.SAMPLE_UNAVAILABLE
        probe_distance = min(
            _MATERIAL_PROBE_CAP,
            max(_MATERIAL_PROBE_REL * nominal, 10.0 * COORD_FLOOR),
        )
        samples = _triangle_samples(face, probe_distance=probe_distance)
        if isinstance(samples, MaterialSideRefusalReason):
            return samples
        signs: list[int] = []
        certified_samples: list[tuple[float, float, float]] = []
        candidate_normals: list[tuple[float, float, float]] = []
        try:
            classifier = BRepClass3d_SolidClassifier(self._graph.solid_shape(solid).wrapped)
            for point in samples:
                if surface.kind is SurfaceKind.PLANE:
                    direction = _candidate_normal(surface, point)
                    if direction is None or not _regular_plane_differential(face, point, direction):
                        return MaterialSideRefusalReason.DIFFERENTIAL_DEGENERATE
                    certified_point = point
                else:
                    cylinder_sample = _regular_cylinder_sample(
                        face, point, surface, probe_distance=probe_distance
                    )
                    if cylinder_sample is None:
                        # A triangulation seed can project onto a periodic seam even though other
                        # retained interior seeds are well clear.  The same bounded minimum-sample
                        # rule applies after projection; one seam seed is not authority to refuse
                        # an otherwise independently proved curved side.
                        continue
                    certified_point, direction = cylinder_sample
                certified_samples.append(certified_point)
                candidate_normals.append(direction)
                states = []
                for sign in (1.0, -1.0):
                    classifier.Perform(
                        gp_Pnt(
                            *(
                                coordinate + sign * probe_distance * axis
                                for coordinate, axis in zip(certified_point, direction, strict=True)
                            )
                        ),
                        _MATERIAL_CLASSIFIER_TOLERANCE,
                    )
                    states.append(classifier.State())
                if tuple(states) == (TopAbs_OUT, TopAbs_IN):
                    signs.append(1)
                elif tuple(states) == (TopAbs_IN, TopAbs_OUT):
                    signs.append(-1)
                else:
                    return MaterialSideRefusalReason.PROBE_INDETERMINATE
        except (Standard_Failure, RuntimeError, ValueError):
            return MaterialSideRefusalReason.PROBE_INDETERMINATE
        if len(signs) < _MATERIAL_MIN_SAMPLES:
            return MaterialSideRefusalReason.DIFFERENTIAL_DEGENERATE
        if len(set(signs)) != 1:
            return MaterialSideRefusalReason.SAMPLES_DISAGREE
        outward_sign = signs[0]
        outward_samples = tuple(
            (
                outward_sign * direction[0],
                outward_sign * direction[1],
                outward_sign * direction[2],
            )
            for direction in candidate_normals
        )
        return MaterialSideCertificate(
            node=node,
            solid=solid,
            outward=outward_samples[0],
            candidate_outward_sign=outward_sign,
            outward_samples=outward_samples,
            sample_points=tuple(certified_samples),
            probe_distance=probe_distance,
            classifier_tolerance=_MATERIAL_CLASSIFIER_TOLERANCE,
            original_orientation=int(face.wrapped.Orientation()),
            authority=_MATERIAL_SIDE_AUTHORITY,
        )

    def _certify_plane(
        self, node: FaceNode, surface: AnalyticSurfaceFact
    ) -> MaterialSideCertificate | MaterialSideRefusalReason:
        """Compatibility seam for the original plane-only certificate tests."""

        return self._certify_material_side(node, surface)


def effective_faces_for_graph(
    graph: FaceGraph, surfaces: EffectiveSurfaceQuery | None = None
) -> EffectiveFaceSurfaceQuery:
    """Issue the restricted family query for an existing run-owned graph."""

    effective = EffectiveSurfaceIndex(graph) if surfaces is None else surfaces
    return _EffectiveFaceSurfaces(graph, effective)


def effective_faces_for_part(part: Part) -> EffectiveFaceSurfaceQuery:
    """Issue one short-lived query for a standalone recogniser invocation."""

    return effective_faces_for_graph(FaceGraph(part))


def _physical_boundary_length(face) -> float:
    """Physical trim perimeter, excluding seams and degenerate representation edges."""

    return math.fsum(
        edge.length
        for edge in face.edges()
        if not BRep_Tool.IsClosed_s(edge.wrapped, face.wrapped)
        and not BRep_Tool.Degenerated_s(edge.wrapped)
    )


def recovery_nominal(face) -> float:
    """Rigid-transform and seam-invariant controlling length for one trimmed face."""

    area = float(face.area)
    perimeter = float(_physical_boundary_length(face))
    if not math.isfinite(area) or area <= 0.0:
        raise ValueError("analytic recovery requires a finite positive trimmed-face area")
    if not math.isfinite(perimeter) or perimeter < 0.0:
        raise ValueError("analytic recovery requires a finite nonnegative trim perimeter")
    area_scale = math.sqrt(area)
    return min(area_scale, 2.0 * area / perimeter) if perimeter > 0.0 else area_scale


def recovery_tolerance(face) -> float:
    """ADR 0008 F1 same-geometry tolerance, fixed before corpus measurement."""

    return _RECOVERY_REL * recovery_nominal(face) + COORD_FLOOR


_NATIVE_KINDS = {
    GeomAbs_Plane: SurfaceKind.PLANE,
    GeomAbs_Cylinder: SurfaceKind.CYLINDER,
    GeomAbs_Cone: SurfaceKind.CONE,
    GeomAbs_Sphere: SurfaceKind.SPHERE,
}


class EffectiveSurfaceIndex:
    """Lazy one-fact-per-original-node analytic view for one graph."""

    def __init__(self, graph: FaceGraph) -> None:
        self._graph = graph
        self._facts: dict[FaceNode, EffectiveSurfaceFact] = {}

    @property
    def run_token(self) -> GraphRunToken:
        return self._graph.run_token

    def fact(self, node: FaceNode) -> EffectiveSurfaceFact:
        if not self._graph.owns(node):
            raise ValueError(f"{node!r} was not issued by this effective surface graph")
        found = self._facts.get(node)
        if found is None:
            found = self._derive(node)
            self._facts[node] = found
        return found

    def oriented_fact(self, node: FaceNode) -> AnalyticSurfaceFact:
        found = self.fact(node)
        if not isinstance(found, AnalyticSurfaceFact):
            raise ValueError(f"surface is unavailable: {found.reason.value}")
        if found.orientation is OrientationCapability.RECOVERED_UNORIENTED:
            raise ValueError("ORIENTATION_UNPROVEN")
        return found

    def _derive(self, node: FaceNode) -> EffectiveSurfaceFact:
        face = self._graph.face(node)
        try:
            adaptor = BRepAdaptor_Surface(face.wrapped)
            kind = adaptor.GetType()
        except (Standard_Failure, RuntimeError, ValueError):
            return RefusedSurfaceFact(node, SurfaceRefusalReason.INVALID_INPUT)
        native = _NATIVE_KINDS.get(kind)
        if native is not None:
            try:
                parameters = validated_parameters(native, native_primitive(adaptor, native))
            except (AttributeError, Standard_Failure, RuntimeError, ValueError):
                return RefusedSurfaceFact(node, SurfaceRefusalReason.INVALID_RESULT)
            return AnalyticSurfaceFact(
                node=node,
                kind=native,
                provenance=SurfaceProvenance.NATIVE,
                orientation=OrientationCapability.NATIVE_ORIENTED,
                parameters=parameters,
                requested_tolerance=0.0,
                kernel_reported_gap=0.0,
                certificate=None,
            )
        if kind == GeomAbs_Torus:
            return RefusedSurfaceFact(node, SurfaceRefusalReason.UNSUPPORTED_TORUS_RECOVERY)
        if kind in (GeomAbs_BSplineSurface, GeomAbs_BezierSurface):
            return self._recover(node, face)
        return RefusedSurfaceFact(node, SurfaceRefusalReason.UNSUPPORTED_KIND)

    def _recover(self, node: FaceNode, face) -> EffectiveSurfaceFact:
        occt_version = getattr(OCP, "__version__", "")
        if occt_version not in _SUPPORTED_OCCT_CERTIFICATE_VERSIONS:
            return RefusedSurfaceFact(node, SurfaceRefusalReason.UNSUPPORTED_OCCT_CONTRACT)
        try:
            tolerance = recovery_tolerance(face)
        except ValueError:
            return RefusedSurfaceFact(node, SurfaceRefusalReason.INVALID_INPUT)

        attempts = (
            (SurfaceKind.PLANE, "IsPlane", gp_Pln()),
            (SurfaceKind.CYLINDER, "IsCylinder", gp_Cylinder()),
            (SurfaceKind.CONE, "IsCone", gp_Cone()),
            (SurfaceKind.SPHERE, "IsSphere", gp_Sphere()),
        )
        passed: list[tuple[SurfaceKind, tuple[float, ...], float]] = []
        unavailable = False
        invalid = False
        exceeded = False
        for analytic_kind, method, primitive in attempts:
            try:
                recogniser = ShapeAnalysis_CanonicalRecognition(face.wrapped)
                accepted = bool(getattr(recogniser, method)(tolerance, primitive))
                status = recogniser.GetStatus()
                gap = float(recogniser.GetGap())
            except (Standard_Failure, RuntimeError, ValueError):
                unavailable = True
                continue
            if not accepted:
                continue
            if status != 0 or not math.isfinite(gap) or gap < 0.0:
                invalid = True
                continue
            if gap > tolerance:
                exceeded = True
                continue
            try:
                parameters = validated_parameters(analytic_kind, primitive)
            except (AttributeError, Standard_Failure, RuntimeError, ValueError):
                invalid = True
                continue
            passed.append((analytic_kind, parameters, gap))

        if len(passed) != 1:
            if len(passed) > 1:
                return RefusedSurfaceFact(node, SurfaceRefusalReason.AMBIGUOUS_PRIMITIVE)
            if invalid:
                return RefusedSurfaceFact(node, SurfaceRefusalReason.INVALID_RESULT)
            if exceeded:
                return RefusedSurfaceFact(node, SurfaceRefusalReason.RESIDUAL_EXCEEDED)
            return RefusedSurfaceFact(node, SurfaceRefusalReason.FIT_UNAVAILABLE)
        if unavailable or invalid:
            return RefusedSurfaceFact(
                node,
                SurfaceRefusalReason.INVALID_RESULT
                if invalid
                else SurfaceRefusalReason.FIT_UNAVAILABLE,
            )
        analytic_kind, parameters, gap = passed[0]
        return AnalyticSurfaceFact(
            node=node,
            kind=analytic_kind,
            provenance=SurfaceProvenance.RECOVERED,
            orientation=OrientationCapability.RECOVERED_UNORIENTED,
            parameters=parameters,
            requested_tolerance=tolerance,
            kernel_reported_gap=gap,
            certificate=RecoveryCertificate(
                occt_version=occt_version,
                authority=_CERTIFICATE_AUTHORITY,
                maximum_distance_bound=tolerance,
            ),
        )
