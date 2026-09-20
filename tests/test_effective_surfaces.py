import math
from dataclasses import replace

import pytest
from build123d import (
    Axis,
    Box,
    Compound,
    Cone,
    Cylinder,
    Face,
    Part,
    Plane,
    Sphere,
    Torus,
    export_step,
    import_step,
)
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace, BRepBuilderAPI_NurbsConvert
from OCP.BRepTools import BRepTools
from OCP.Geom import Geom_BezierSurface, Geom_RectangularTrimmedSurface
from OCP.GeomAbs import GeomAbs_Cylinder
from OCP.GeomConvert import GeomConvert
from OCP.gp import gp_Ax3, gp_Cylinder, gp_Dir, gp_Pnt, gp_Pnt2d
from OCP.Standard import Standard_Failure, Standard_TypeMismatch
from OCP.TColgp import TColgp_Array2OfPnt
from OCP.TopAbs import TopAbs_IN, TopAbs_OUT
from OCP.TopLoc import TopLoc_Location

from quiddity._adjacency import FaceGraph
from quiddity._analytic_surfaces import validated_parameters
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceIndex,
    MaterialSideRefusalReason,
    OrientationCapability,
    SurfaceKind,
    SurfaceProvenance,
    SurfaceRefusalReason,
    SurfaceUse,
    SurfaceUseRefusal,
    effective_faces_for_graph,
    recovery_nominal,
    recovery_tolerance,
)
from quiddity._geometry import COORD_FLOOR


def test_recovery_nominal_is_width_controlled_and_rotation_invariant() -> None:
    face = max(Box(100, 1, 1).faces(), key=lambda item: item.area)
    moved = face.rotate(Axis.Z, 45)

    assert recovery_nominal(face) == pytest.approx(recovery_nominal(moved))
    assert recovery_nominal(face) == pytest.approx(2 * 100 / 202)
    assert recovery_tolerance(face) == pytest.approx(1e-6 * recovery_nominal(face) + COORD_FLOOR)


def test_periodic_seams_do_not_enter_the_physical_boundary_nominal() -> None:
    cylinder_side = max(Cylinder(10, 20).faces(), key=lambda face: face.area)
    sphere = Sphere(10).faces()[0]

    assert recovery_nominal(cylinder_side) == pytest.approx(20.0)
    assert recovery_nominal(sphere) == pytest.approx(math.sqrt(400 * math.pi))


def test_effective_surface_index_keeps_original_node_identity() -> None:
    graph = FaceGraph(Box(1, 2, 3))
    index = EffectiveSurfaceIndex(graph)
    node = graph.nodes[0]

    first = index.fact(node)
    assert index.fact(node) is first
    assert isinstance(first, AnalyticSurfaceFact)
    assert first.node is node
    assert first.kind is SurfaceKind.PLANE
    assert first.orientation is OrientationCapability.NATIVE_ORIENTED
    assert index.oriented_fact(node) is first


def test_effective_surface_index_rejects_foreign_nodes() -> None:
    left = EffectiveSurfaceIndex(FaceGraph(Box(1, 1, 1)))
    foreign = FaceGraph(Box(1, 1, 1)).nodes[0]

    with pytest.raises(ValueError, match="not issued"):
        left.fact(foreign)


def test_plane_material_side_is_a_separate_cached_closed_solid_certificate() -> None:
    part = Box(10, 8, 4)
    graph = FaceGraph(part)
    query = effective_faces_for_graph(graph)
    top = max(part.faces(), key=lambda face: face.center().Z)

    first = query.use(top, material_side=True)
    assert isinstance(first, SurfaceUse)
    assert query.use(top, material_side=True) is first
    assert first.surface.orientation is OrientationCapability.NATIVE_ORIENTED
    assert first.material_side is not None
    assert first.material_side.node is graph.require_node(top)
    assert first.material_side.outward == pytest.approx((0.0, 0.0, 1.0))
    assert len(first.material_side.sample_points) >= 2
    assert first.material_side.probe_distance > first.material_side.classifier_tolerance

    object.__setattr__(first.material_side, "outward", (0.0, 0.0, -1.0))
    with pytest.raises(ValueError, match="no longer matches"):
        _ = first.material_side


def test_surface_uses_are_opaque_and_detect_forgery_or_changed_facts(monkeypatch) -> None:
    with pytest.raises(TypeError, match="issued by"):
        SurfaceUse()

    forged = object.__new__(SurfaceUse)
    with pytest.raises(ValueError, match="authority was mutated"):
        _ = forged.node

    part = Box(10, 8, 4)
    graph = FaceGraph(part)
    query = effective_faces_for_graph(graph)
    top = max(part.faces(), key=lambda face: face.center().Z)
    issued = query.use(top)
    assert isinstance(issued, SurfaceUse)
    original = issued.surface

    unissued = object.__new__(SurfaceUse)
    object.__setattr__(
        unissued,
        "_SurfaceUse__authority",
        issued._SurfaceUse__authority,
    )
    with pytest.raises(ValueError, match="not issued by this query"):
        _ = unissued.node

    monkeypatch.setattr(
        EffectiveSurfaceIndex,
        "fact",
        lambda _self, _node: replace(original, kernel_reported_gap=1.0),
    )
    with pytest.raises(ValueError, match="no longer matches its issued fact"):
        _ = issued.surface


def test_face_query_returns_a_surface_refusal_for_an_unavailable_fact() -> None:
    torus = Torus(10, 2)
    query = effective_faces_for_graph(FaceGraph(torus))

    refused = query.use(torus.faces()[0])

    assert isinstance(refused, SurfaceUseRefusal)
    assert refused.reason is MaterialSideRefusalReason.SURFACE_UNAVAILABLE


def test_material_side_sampling_does_not_attach_a_mesh_to_the_input_face() -> None:
    part = Box(10, 8, 4)
    top = max(part.faces(), key=lambda face: face.center().Z)
    BRepTools.Clean_s(part.wrapped)
    assert BRep_Tool.Triangulation_s(top.wrapped, TopLoc_Location()) is None

    result = effective_faces_for_graph(FaceGraph(part)).use(top, material_side=True)

    assert isinstance(result, SurfaceUse)
    assert BRep_Tool.Triangulation_s(top.wrapped, TopLoc_Location()) is None


def test_material_side_sampling_refuses_uncleared_or_failed_samples(monkeypatch) -> None:
    import quiddity._effective_surfaces as module

    top = max(Box(10, 8, 4).faces(), key=lambda face: face.center().Z)
    monkeypatch.setattr(module.Vertex, "distance_to", lambda _self, _edge: 0.0)
    assert (
        module._triangle_samples(top, probe_distance=1e-3)
        is MaterialSideRefusalReason.SAMPLE_NEAR_BOUNDARY
    )

    monkeypatch.setattr(
        module.Vertex,
        "distance_to",
        lambda _self, _edge: (_ for _ in ()).throw(RuntimeError("distance failure")),
    )
    assert (
        module._triangle_samples(top, probe_distance=1e-3)
        is MaterialSideRefusalReason.SAMPLE_UNAVAILABLE
    )


def test_material_side_sampling_refuses_face_downcast_type_mismatch(monkeypatch) -> None:
    import quiddity._effective_surfaces as module

    top = max(Box(10, 8, 4).faces(), key=lambda face: face.center().Z)

    class TypeMismatchTopoDS:
        @staticmethod
        def Face_s(_shape):
            raise Standard_TypeMismatch("TopoDS::Face")

    monkeypatch.setattr(module, "TopoDS", TypeMismatchTopoDS)

    assert (
        module._triangle_samples(top, probe_distance=1e-3)
        is MaterialSideRefusalReason.SAMPLE_UNAVAILABLE
    )


def test_material_side_sampling_refuses_missing_or_degenerate_triangulation(monkeypatch) -> None:
    import quiddity._effective_surfaces as module

    top = max(Box(10, 8, 4).faces(), key=lambda face: face.center().Z)
    monkeypatch.setattr(module.BRep_Tool, "Triangulation_s", lambda *_args: None)
    assert (
        module._triangle_samples(top, probe_distance=1e-3)
        is MaterialSideRefusalReason.SAMPLE_UNAVAILABLE
    )

    monkeypatch.undo()

    class DegenerateTriangle:
        def Get(self):
            return (1, 2, 3)

    class DegenerateTriangulation:
        def NbNodes(self):
            return 3

        def NbTriangles(self):
            return 1

        def Node(self, _at):
            return gp_Pnt(0.0, 0.0, 0.0)

        def Triangle(self, _at):
            return DegenerateTriangle()

    monkeypatch.setattr(
        module.BRep_Tool,
        "Triangulation_s",
        lambda *_args: DegenerateTriangulation(),
    )
    assert (
        module._triangle_samples(top, probe_distance=1e-3)
        is MaterialSideRefusalReason.SAMPLE_UNAVAILABLE
    )


def test_plane_differential_refuses_uv_that_does_not_recover_the_sample(monkeypatch) -> None:
    import quiddity._effective_surfaces as module

    class WrongUV:
        def __init__(self, _surface) -> None:
            pass

        def ValueOfUV(self, _point, _tolerance):
            return gp_Pnt2d(1_000_000.0, 1_000_000.0)

    part = Box(10, 8, 4)
    top = max(part.faces(), key=lambda face: face.center().Z)
    centre = top.center()
    monkeypatch.setattr(module, "ShapeAnalysis_Surface", WrongUV)

    assert not module._regular_plane_differential(
        top, (centre.X, centre.Y, centre.Z), (0.0, 0.0, 1.0)
    )


def test_plane_differential_refuses_degenerate_or_failed_kernel_reads(monkeypatch) -> None:
    import quiddity._effective_surfaces as module

    top = max(Box(10, 8, 4).faces(), key=lambda face: face.center().Z)
    centre = top.center()

    class ZeroDifferential:
        def __init__(self, _shape) -> None:
            pass

        def D1(self, _u, _v, point, _along_u, _along_v) -> None:
            point.SetCoord(centre.X, centre.Y, centre.Z)

    monkeypatch.setattr(module, "BRepAdaptor_Surface", ZeroDifferential)
    assert not module._regular_plane_differential(
        top, (centre.X, centre.Y, centre.Z), (0.0, 0.0, 1.0)
    )

    monkeypatch.setattr(
        module,
        "ShapeAnalysis_Surface",
        lambda _surface: (_ for _ in ()).throw(RuntimeError("projection failure")),
    )
    assert not module._regular_plane_differential(
        top, (centre.X, centre.Y, centre.Z), (0.0, 0.0, 1.0)
    )


def test_cylinder_material_side_certifies_external_and_internal_radial_polarity() -> None:
    cylinder = Cylinder(4, 10)
    cylinder_query = effective_faces_for_graph(FaceGraph(cylinder))
    curved = max(cylinder.faces(), key=lambda face: face.area)
    external = cylinder_query.use(curved, material_side=True)
    assert isinstance(external, SurfaceUse)
    assert external.material_side is not None
    assert external.material_side.outward == external.material_side.outward_samples[0]
    assert external.material_side.candidate_outward_sign == 1
    assert len(external.material_side.outward_samples) >= 2

    bored = Box(12, 12, 10) - Cylinder(2, 10)
    bore = next(
        face
        for face in bored.faces()
        if BRepAdaptor_Surface(face.wrapped).GetType() == GeomAbs_Cylinder
    )
    internal = effective_faces_for_graph(FaceGraph(bored)).use(bore, material_side=True)
    assert isinstance(internal, SurfaceUse)
    assert internal.material_side is not None
    assert internal.material_side.candidate_outward_sign == -1


@pytest.mark.parametrize(
    ("part", "expected_sign"),
    [
        (Cylinder(4, 10).rotate(Axis.X, 37), 1),
        ((Box(12, 12, 10) - Cylinder(2, 10)).rotate(Axis.Y, 29), -1),
    ],
)
def test_exact_converted_cylinder_material_side_is_transform_invariant(part, expected_sign) -> None:
    converted = Part(BRepBuilderAPI_NurbsConvert(part.wrapped, True).Shape())
    graph = FaceGraph(converted)
    query = effective_faces_for_graph(graph)
    curved = next(
        graph.face(node)
        for node in graph.nodes
        if isinstance(query.fact(graph.face(node)), AnalyticSurfaceFact)
        and query.fact(graph.face(node)).kind is SurfaceKind.CYLINDER
    )

    issued = query.use(curved, material_side=True)

    assert isinstance(issued, SurfaceUse)
    assert issued.surface.provenance is SurfaceProvenance.RECOVERED
    assert issued.material_side is not None
    assert issued.material_side.candidate_outward_sign == expected_sign
    assert len(issued.material_side.outward_samples) >= 2


def test_material_side_refuses_faces_without_one_closed_owner(monkeypatch) -> None:

    open_face = Face.make_rect(10, 8)
    open_query = effective_faces_for_graph(FaceGraph(open_face))
    unowned = open_query.use(open_face, material_side=True)
    assert isinstance(unowned, SurfaceUseRefusal)
    assert unowned.reason is MaterialSideRefusalReason.OWNER_UNPROVEN

    cylinder_side = max(Cylinder(4, 10).faces(), key=lambda face: face.area)
    recovered_open = _as_bspline_face(cylinder_side)
    recovered_query = effective_faces_for_graph(FaceGraph(recovered_open))
    recovered_unowned = recovered_query.use(recovered_open, material_side=True)
    assert isinstance(recovered_unowned, SurfaceUseRefusal)
    assert recovered_unowned.reason is MaterialSideRefusalReason.OWNER_UNPROVEN

    ambiguous = Cylinder(4, 10)
    ambiguous_graph = FaceGraph(ambiguous)
    monkeypatch.setattr(ambiguous_graph, "common_valid_solid", lambda _nodes: None)
    ambiguous_query = effective_faces_for_graph(ambiguous_graph)
    ambiguous_curved = max(ambiguous.faces(), key=lambda face: face.area)
    ambiguous_use = ambiguous_query.use(ambiguous_curved, material_side=True)
    assert isinstance(ambiguous_use, SurfaceUseRefusal)
    assert ambiguous_use.reason is MaterialSideRefusalReason.OWNER_UNPROVEN


def test_material_side_kernel_and_differential_failures_refuse(monkeypatch) -> None:
    import quiddity._effective_surfaces as module

    part = Box(10, 8, 4)
    top = max(part.faces(), key=lambda face: face.center().Z)
    monkeypatch.setattr(module, "_regular_plane_differential", lambda *_args: False)
    degenerate = effective_faces_for_graph(FaceGraph(part)).use(top, material_side=True)
    assert isinstance(degenerate, SurfaceUseRefusal)
    assert degenerate.reason is MaterialSideRefusalReason.DIFFERENTIAL_DEGENERATE

    monkeypatch.undo()

    class FailedClassifier:
        def __init__(self, _solid) -> None:
            raise RuntimeError("classifier failure")

    monkeypatch.setattr(module, "BRepClass3d_SolidClassifier", FailedClassifier)
    failed = effective_faces_for_graph(FaceGraph(part)).use(top, material_side=True)
    assert isinstance(failed, SurfaceUseRefusal)
    assert failed.reason is MaterialSideRefusalReason.PROBE_INDETERMINATE


def test_material_side_propagates_nominal_and_sampling_refusals(monkeypatch) -> None:
    import quiddity._effective_surfaces as module

    part = Box(10, 8, 4)
    top = max(part.faces(), key=lambda face: face.center().Z)
    monkeypatch.setattr(
        module,
        "recovery_nominal",
        lambda _face: (_ for _ in ()).throw(ValueError("invalid nominal")),
    )
    invalid = effective_faces_for_graph(FaceGraph(part)).use(top, material_side=True)
    assert isinstance(invalid, SurfaceUseRefusal)
    assert invalid.reason is MaterialSideRefusalReason.SAMPLE_UNAVAILABLE

    monkeypatch.undo()
    monkeypatch.setattr(
        module,
        "_triangle_samples",
        lambda _face, *, probe_distance: MaterialSideRefusalReason.SAMPLE_NEAR_BOUNDARY,
    )
    uncleared = effective_faces_for_graph(FaceGraph(part)).use(top, material_side=True)
    assert isinstance(uncleared, SurfaceUseRefusal)
    assert uncleared.reason is MaterialSideRefusalReason.SAMPLE_NEAR_BOUNDARY


@pytest.mark.parametrize(
    ("states", "reason"),
    [
        ((TopAbs_OUT, TopAbs_OUT), MaterialSideRefusalReason.PROBE_INDETERMINATE),
        (
            (TopAbs_OUT, TopAbs_IN, TopAbs_IN, TopAbs_OUT),
            MaterialSideRefusalReason.SAMPLES_DISAGREE,
        ),
    ],
)
def test_material_side_refuses_indeterminate_or_disagreeing_probes(
    monkeypatch, states, reason
) -> None:
    import quiddity._effective_surfaces as module

    class ScriptedClassifier:
        def __init__(self, _solid) -> None:
            self.states = iter(states)
            self.state = None

        def Perform(self, _point, _tolerance) -> None:
            self.state = next(self.states)

        def State(self):
            return self.state

    part = Box(10, 8, 4)
    top = max(part.faces(), key=lambda face: face.center().Z)
    centre = top.center()
    samples = (
        (centre.X - 1.0, centre.Y, centre.Z),
        (centre.X + 1.0, centre.Y, centre.Z),
    )
    monkeypatch.setattr(module, "_triangle_samples", lambda *_args, **_kwargs: samples)
    monkeypatch.setattr(module, "_regular_plane_differential", lambda *_args: True)
    monkeypatch.setattr(module, "BRepClass3d_SolidClassifier", ScriptedClassifier)

    refused = effective_faces_for_graph(FaceGraph(part)).use(top, material_side=True)

    assert isinstance(refused, SurfaceUseRefusal)
    assert refused.reason is reason


def _as_bspline_face(face: Face) -> Face:
    adaptor = BRepAdaptor_Surface(face.wrapped)
    surface = BRep_Tool.Surface_s(face.wrapped)
    trimmed = Geom_RectangularTrimmedSurface(
        surface,
        adaptor.FirstUParameter(),
        adaptor.LastUParameter(),
        adaptor.FirstVParameter(),
        adaptor.LastVParameter(),
    )
    bspline = GeomConvert.SurfaceToBSplineSurface_s(trimmed)
    made = BRepBuilderAPI_MakeFace(
        bspline,
        adaptor.FirstUParameter(),
        adaptor.LastUParameter(),
        adaptor.FirstVParameter(),
        adaptor.LastVParameter(),
        1e-7,
    )
    return Face(made.Face())


def test_exact_bspline_plane_recovers_as_unoriented_original_node() -> None:
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))
    node = graph.nodes[0]
    fact = EffectiveSurfaceIndex(graph).fact(node)

    assert isinstance(fact, AnalyticSurfaceFact)
    assert fact.node is node
    assert fact.kind is SurfaceKind.PLANE
    assert fact.provenance is SurfaceProvenance.RECOVERED
    assert fact.orientation is OrientationCapability.RECOVERED_UNORIENTED
    assert fact.kernel_reported_gap == 0.0
    assert fact.requested_tolerance > 0.0
    assert fact.certificate is not None
    assert fact.certificate.occt_version == "7.9.3.1"
    assert fact.certificate.maximum_distance_bound == fact.requested_tolerance

    with pytest.raises(ValueError, match="ORIENTATION_UNPROVEN"):
        EffectiveSurfaceIndex(graph).oriented_fact(node)


@pytest.mark.parametrize(
    ("native", "kind"),
    [
        (max(Cylinder(5, 12).faces(), key=lambda face: face.area), SurfaceKind.CYLINDER),
        (max(Cone(6, 3, 12).faces(), key=lambda face: face.area), SurfaceKind.CONE),
        (Sphere(7).faces()[0], SurfaceKind.SPHERE),
    ],
)
def test_exact_bspline_curved_primitives_recover_without_orientation(
    native: Face, kind: SurfaceKind
) -> None:
    graph = FaceGraph(_as_bspline_face(native))
    node = graph.nodes[0]
    fact = EffectiveSurfaceIndex(graph).fact(node)

    assert isinstance(fact, AnalyticSurfaceFact)
    assert fact.node is node
    assert fact.kind is kind
    assert fact.provenance is SurfaceProvenance.RECOVERED
    assert fact.orientation is OrientationCapability.RECOVERED_UNORIENTED
    assert fact.parameters


def test_multiple_passing_fits_refuse_instead_of_using_call_order(monkeypatch) -> None:
    class AmbiguousRecognition:
        def __init__(self, _shape) -> None:
            pass

        def IsPlane(self, _tolerance, _result) -> bool:
            return True

        def IsCylinder(self, _tolerance, _result) -> bool:
            return True

        def IsCone(self, _tolerance, _result) -> bool:
            return False

        def IsSphere(self, _tolerance, _result) -> bool:
            return False

        def GetStatus(self) -> int:
            return 0

        def GetGap(self) -> float:
            return 0.0

    monkeypatch.setattr(
        "quiddity._effective_surfaces.ShapeAnalysis_CanonicalRecognition",
        AmbiguousRecognition,
    )
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))
    fact = EffectiveSurfaceIndex(graph).fact(graph.nodes[0])

    assert fact.reason is SurfaceRefusalReason.AMBIGUOUS_PRIMITIVE


def test_kernel_errors_fail_closed(monkeypatch) -> None:
    class FailedRecognition:
        def __init__(self, _shape) -> None:
            pass

        def IsPlane(self, _tolerance, _result) -> bool:
            raise RuntimeError("kernel failure")

        IsCylinder = IsPlane
        IsCone = IsPlane
        IsSphere = IsPlane

    monkeypatch.setattr(
        "quiddity._effective_surfaces.ShapeAnalysis_CanonicalRecognition",
        FailedRecognition,
    )
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))
    fact = EffectiveSurfaceIndex(graph).fact(graph.nodes[0])

    assert fact.reason is SurfaceRefusalReason.FIT_UNAVAILABLE


def test_recogniser_constructor_errors_fail_closed(monkeypatch) -> None:
    class FailedConstruction:
        def __init__(self, _shape) -> None:
            raise RuntimeError("constructor failure")

    monkeypatch.setattr(
        "quiddity._effective_surfaces.ShapeAnalysis_CanonicalRecognition",
        FailedConstruction,
    )
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.FIT_UNAVAILABLE
    )


def test_invalid_accepted_primitive_fails_closed(monkeypatch) -> None:
    class InvalidPlaneRecognition:
        def __init__(self, _shape) -> None:
            pass

        def IsPlane(self, _tolerance, result) -> bool:
            result.SetLocation(gp_Pnt(float("nan"), 0, 0))
            return True

        def IsCylinder(self, _tolerance, _result) -> bool:
            return False

        IsCone = IsCylinder
        IsSphere = IsCylinder

        def GetStatus(self) -> int:
            return 0

        def GetGap(self) -> float:
            return 0.0

    monkeypatch.setattr(
        "quiddity._effective_surfaces.ShapeAnalysis_CanonicalRecognition",
        InvalidPlaneRecognition,
    )
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.INVALID_RESULT
    )


def test_over_tolerance_success_uses_residual_refusal(monkeypatch) -> None:
    class ExcessiveGapRecognition:
        def __init__(self, _shape) -> None:
            pass

        def IsPlane(self, _tolerance, _result) -> bool:
            return True

        def IsCylinder(self, _tolerance, _result) -> bool:
            return False

        IsCone = IsCylinder
        IsSphere = IsCylinder

        def GetStatus(self) -> int:
            return 0

        def GetGap(self) -> float:
            return 1.0

    monkeypatch.setattr(
        "quiddity._effective_surfaces.ShapeAnalysis_CanonicalRecognition",
        ExcessiveGapRecognition,
    )
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.RESIDUAL_EXCEEDED
    )


def test_unpinned_occt_version_disables_recovery_globally(monkeypatch) -> None:
    monkeypatch.setattr("quiddity._effective_surfaces.OCP.__version__", "future")
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.UNSUPPORTED_OCCT_CONTRACT
    )


@pytest.mark.parametrize(
    "native",
    [
        max(Box(10, 5, 2).faces(), key=lambda face: face.area),
        max(Cylinder(5, 12).faces(), key=lambda face: face.area),
        max(Cone(6, 3, 12).faces(), key=lambda face: face.area),
        Sphere(7).faces()[0],
    ],
)
def test_native_and_exact_bspline_use_the_same_canonical_parameters(native: Face) -> None:
    native_graph = FaceGraph(native)
    recovered_graph = FaceGraph(_as_bspline_face(native))
    native_fact = EffectiveSurfaceIndex(native_graph).fact(native_graph.nodes[0])
    recovered_fact = EffectiveSurfaceIndex(recovered_graph).fact(recovered_graph.nodes[0])

    assert isinstance(native_fact, AnalyticSurfaceFact)
    assert isinstance(recovered_fact, AnalyticSurfaceFact)
    assert recovered_fact.kind is native_fact.kind
    assert recovered_fact.parameters == pytest.approx(native_fact.parameters, abs=1e-9)


def test_locally_bumped_bezier_surface_fails_closed() -> None:
    poles = TColgp_Array2OfPnt(1, 3, 1, 3)
    for u in range(1, 4):
        for v in range(1, 4):
            poles.SetValue(u, v, gp_Pnt(u - 1, v - 1, 0.2 if (u, v) == (2, 2) else 0.0))
    made = BRepBuilderAPI_MakeFace(Geom_BezierSurface(poles), 1e-7)
    graph = FaceGraph(Face(made.Face()))
    fact = EffectiveSurfaceIndex(graph).fact(graph.nodes[0])

    assert fact.reason is SurfaceRefusalReason.FIT_UNAVAILABLE


def test_cone_parameters_retain_apex_position_along_the_axis() -> None:
    lower = max(Cone(6, 3, 12).faces(), key=lambda face: face.area)
    upper = lower.translate((0, 0, 5))
    lower_graph = FaceGraph(lower)
    upper_graph = FaceGraph(upper)
    lower_fact = EffectiveSurfaceIndex(lower_graph).fact(lower_graph.nodes[0])
    upper_fact = EffectiveSurfaceIndex(upper_graph).fact(upper_graph.nodes[0])

    assert isinstance(lower_fact, AnalyticSurfaceFact)
    assert isinstance(upper_fact, AnalyticSurfaceFact)
    assert upper_fact.parameters[2] - lower_fact.parameters[2] == pytest.approx(5.0)
    assert upper_fact.parameters != lower_fact.parameters


@pytest.mark.parametrize(
    "transformed",
    [
        max(Cylinder(5, 12).faces(), key=lambda face: face.area).rotate(
            Axis((0, 0, 0), (1, 1, 0)), 37
        ),
        max(Cylinder(5, 12).faces(), key=lambda face: face.area).mirror(Plane.YZ),
        max(Cylinder(5, 12).faces(), key=lambda face: face.area).scale(3),
    ],
)
def test_transformed_exact_bspline_keeps_native_parameters(transformed: Face) -> None:
    native_graph = FaceGraph(transformed)
    recovered_graph = FaceGraph(_as_bspline_face(transformed))
    native = EffectiveSurfaceIndex(native_graph).fact(native_graph.nodes[0])
    recovered = EffectiveSurfaceIndex(recovered_graph).fact(recovered_graph.nodes[0])

    assert isinstance(native, AnalyticSurfaceFact)
    assert isinstance(recovered, AnalyticSurfaceFact)
    assert recovered.kind is native.kind
    assert recovered.parameters == pytest.approx(native.parameters, abs=1e-8)


def test_step_round_trip_retains_recovery_and_original_imported_node(tmp_path) -> None:
    source = _as_bspline_face(max(Box(10, 5, 2).faces(), key=lambda face: face.area))
    path = tmp_path / "bspline-plane.step"
    assert export_step(source, path)
    imported = import_step(path)
    graph = FaceGraph(imported)
    node = graph.nodes[0]
    fact = EffectiveSurfaceIndex(graph).fact(node)

    assert isinstance(fact, AnalyticSurfaceFact)
    assert fact.node is node
    assert fact.kind is SurfaceKind.PLANE
    assert fact.provenance is SurfaceProvenance.RECOVERED


def test_real_huge_radius_patch_refuses_plane_cylinder_ambiguity() -> None:
    cylinder = gp_Cylinder(gp_Ax3(gp_Pnt(), gp_Dir(0, 0, 1)), 1e8)
    patch = Face(BRepBuilderAPI_MakeFace(cylinder, 0, 1e-7, 0, 1).Face())
    graph = FaceGraph(_as_bspline_face(patch))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.AMBIGUOUS_PRIMITIVE
    )


def test_equivalent_periodic_seam_parameterisations_have_one_nominal() -> None:
    first_axis = gp_Ax3(gp_Pnt(), gp_Dir(0, 0, 1), gp_Dir(1, 0, 0))
    second_axis = gp_Ax3(gp_Pnt(), gp_Dir(0, 0, 1), gp_Dir(0, 1, 0))
    first = Face(BRepBuilderAPI_MakeFace(gp_Cylinder(first_axis, 10), 0, 2 * math.pi, 0, 20).Face())
    second = Face(
        BRepBuilderAPI_MakeFace(gp_Cylinder(second_axis, 10), 0, 2 * math.pi, 0, 20).Face()
    )

    assert recovery_nominal(first) == pytest.approx(recovery_nominal(second))
    assert recovery_tolerance(first) == pytest.approx(recovery_tolerance(second))


def test_split_equal_surfaces_keep_distinct_original_nodes() -> None:
    left = _as_bspline_face(max(Box(5, 5, 1).faces(), key=lambda face: face.area))
    right = left.translate((5, 0, 0))
    graph = FaceGraph(Compound(children=[left, right]))
    first = EffectiveSurfaceIndex(graph).fact(graph.nodes[0])
    second = EffectiveSurfaceIndex(graph).fact(graph.nodes[1])

    assert isinstance(first, AnalyticSurfaceFact)
    assert isinstance(second, AnalyticSurfaceFact)
    assert first.node is graph.nodes[0]
    assert second.node is graph.nodes[1]
    assert first.node is not second.node


def test_torus_recovery_is_explicitly_unsupported() -> None:
    graph = FaceGraph(Torus(10, 2))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.UNSUPPORTED_TORUS_RECOVERY
    )


def test_standard_failure_during_native_primitive_read_refuses(monkeypatch) -> None:
    def fail(_kind, _primitive):
        raise Standard_Failure("primitive read failure")

    monkeypatch.setattr("quiddity._effective_surfaces.validated_parameters", fail)
    graph = FaceGraph(Box(1, 1, 1))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.INVALID_RESULT
    )


def test_standard_failure_during_recovered_primitive_read_refuses(monkeypatch) -> None:
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))

    def fail(_kind, _primitive):
        raise Standard_Failure("primitive read failure")

    monkeypatch.setattr("quiddity._effective_surfaces.validated_parameters", fail)

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.INVALID_RESULT
    )


def test_exact_planar_bezier_recovers_with_canonical_zero() -> None:
    poles = TColgp_Array2OfPnt(1, 2, 1, 2)
    for u in range(1, 3):
        for v in range(1, 3):
            poles.SetValue(u, v, gp_Pnt(u - 1, v - 1, 0))
    made = BRepBuilderAPI_MakeFace(Geom_BezierSurface(poles), 1e-7)
    graph = FaceGraph(Face(made.Face()))
    fact = EffectiveSurfaceIndex(graph).fact(graph.nodes[0])

    assert isinstance(fact, AnalyticSurfaceFact)
    assert fact.kind is SurfaceKind.PLANE
    assert fact.provenance is SurfaceProvenance.RECOVERED
    assert all(math.copysign(1.0, value) > 0.0 for value in fact.parameters if value == 0.0)


@pytest.mark.parametrize("area", [0.0, float("nan")])
def test_recovery_nominal_refuses_invalid_trimmed_area(area: float) -> None:
    class InvalidFace:
        pass

    face = InvalidFace()
    face.area = area
    face.edges = lambda: ()

    with pytest.raises(ValueError, match="finite positive"):
        recovery_nominal(face)


@pytest.mark.parametrize("perimeter", [-1.0, float("nan")])
def test_recovery_nominal_refuses_invalid_trim_perimeter(monkeypatch, perimeter: float) -> None:
    monkeypatch.setattr(
        "quiddity._effective_surfaces._physical_boundary_length",
        lambda _face: perimeter,
    )
    face = max(Box(1, 1, 1).faces(), key=lambda item: item.area)

    with pytest.raises(ValueError, match="finite nonnegative"):
        recovery_nominal(face)


def test_oriented_query_reports_an_unavailable_surface() -> None:
    graph = FaceGraph(Torus(10, 2))

    with pytest.raises(ValueError, match="unsupported-torus-recovery"):
        EffectiveSurfaceIndex(graph).oriented_fact(graph.nodes[0])


def test_face_query_refuses_foreign_graphs_and_faces() -> None:
    graph = FaceGraph(Box(1, 1, 1))
    foreign = FaceGraph(Box(2, 2, 2))

    with pytest.raises(ValueError, match="different runs"):
        effective_faces_for_graph(graph, EffectiveSurfaceIndex(foreign))

    query = effective_faces_for_graph(graph)
    with pytest.raises(ValueError, match="different part"):
        query.fact(foreign.face(foreign.nodes[0]))


@pytest.mark.parametrize("failure", [Standard_Failure("adaptor failure"), RuntimeError("failure")])
def test_adaptor_failures_are_closed_invalid_inputs(monkeypatch, failure: Exception) -> None:
    class FailedAdaptor:
        def __init__(self, _shape) -> None:
            raise failure

    monkeypatch.setattr("quiddity._effective_surfaces.BRepAdaptor_Surface", FailedAdaptor)
    graph = FaceGraph(Box(1, 1, 1))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.INVALID_INPUT
    )


def test_unknown_native_surface_kind_is_explicitly_unsupported(monkeypatch) -> None:
    class UnknownAdaptor:
        def __init__(self, _shape) -> None:
            pass

        def GetType(self) -> int:
            return 999

    monkeypatch.setattr("quiddity._effective_surfaces.BRepAdaptor_Surface", UnknownAdaptor)
    graph = FaceGraph(Box(1, 1, 1))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.UNSUPPORTED_KIND
    )


def test_one_valid_fit_plus_kernel_failures_refuses_the_partial_result(monkeypatch) -> None:
    class PartialRecognition:
        def __init__(self, _shape) -> None:
            pass

        def IsPlane(self, _tolerance, _result) -> bool:
            return True

        def IsCylinder(self, _tolerance, _result) -> bool:
            raise RuntimeError("cylinder fit failed")

        IsCone = IsCylinder
        IsSphere = IsCylinder

        def GetStatus(self) -> int:
            return 0

        def GetGap(self) -> float:
            return 0.0

    monkeypatch.setattr(
        "quiddity._effective_surfaces.ShapeAnalysis_CanonicalRecognition",
        PartialRecognition,
    )
    native = max(Box(10, 5, 2).faces(), key=lambda face: face.area)
    graph = FaceGraph(_as_bspline_face(native))

    assert (
        EffectiveSurfaceIndex(graph).fact(graph.nodes[0]).reason
        is SurfaceRefusalReason.FIT_UNAVAILABLE
    )


class _InvalidRadius:
    def Location(self):
        return gp_Pnt()

    def Radius(self) -> float:
        return 0.0


def test_nonpositive_sphere_radius_is_not_an_analytic_fact() -> None:
    with pytest.raises(ValueError, match="radius must be positive"):
        validated_parameters(SurfaceKind.SPHERE, _InvalidRadius())
