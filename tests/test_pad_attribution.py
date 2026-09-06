"""F5: each RaisedPad owns its accepted top and four perimeter-wall roles."""

from __future__ import annotations

import ast
import copy
import math
from dataclasses import dataclass, replace
from pathlib import Path
from types import SimpleNamespace

import pytest
from build123d import (
    Align,
    Box,
    Compound,
    Cylinder,
    Part,
    Plane,
    Pos,
    RegularPolygon,
    Rot,
    Shell,
    SlotOverall,
    export_step,
    extrude,
    fillet,
    import_step,
)
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_NurbsConvert
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Plane
from OCP.GProp import GProp_GProps

from quiddity import (
    FramedRecognitionResult,
    build_framed_recognition_result,
    recognise_rectangular_pads,
)
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceIndex,
    MaterialSideRefusalReason,
    RefusedSurfaceFact,
    SurfaceKind,
    SurfaceProvenance,
    SurfaceRefusalReason,
    SurfaceUseRefusal,
    effective_faces_for_graph,
    effective_faces_for_part,
)
from quiddity.experimental_geometry import GeometryGraph, GeometryProvenance
from quiddity.pads import (
    _AXIS_INDEX,
    RaisedPad,
    _axial_extent,
    _discover_rectangular_pads,
    _recognise_rectangular_pads_one,
    _record_bounds,
    _tier_suppresses,
    _wall_role,
)
from quiddity.result import _take_inventory

ROOT = Path(__file__).parents[1]


def _qualified_calls(tree: ast.AST) -> list[tuple[str, ast.Call]]:
    aliases: dict[str, str] = {}
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                aliases[alias.asname or alias.name.split(".")[0]] = alias.name
        elif isinstance(node, ast.ImportFrom) and node.module:
            for alias in node.names:
                aliases[alias.asname or alias.name] = f"{node.module}.{alias.name}"

    def qualified(node: ast.expr) -> str:
        if isinstance(node, ast.Name):
            return aliases.get(node.id, node.id)
        if isinstance(node, ast.Attribute):
            return f"{qualified(node.value)}.{node.attr}"
        return ""

    return [(qualified(node.func), node) for node in ast.walk(tree) if isinstance(node, ast.Call)]


def _pad():
    return Box(80, 60, 10) + Pos(0, 0, 7) * Box(30, 20, 4)


def _oriented_pad(rotation):
    stock = rotation * Box(80, 60, 10)
    island = rotation * (Pos(0, 0, 7) * Box(30, 20, 4))
    return stock + island, island


def _assert_record_matches_authored_island(
    record: RaisedPad, island, *, axis: str, direction: int
) -> None:
    bounds = island.bounding_box()
    assert record == RaisedPad(
        round(float(bounds.min.X), 3),
        round(float(bounds.max.X), 3),
        round(float(bounds.min.Y), 3),
        round(float(bounds.max.Y), 3),
        round(float(bounds.min.Z), 3),
        round(float(bounds.max.Z), 3),
        axis,
        direction,
    )


def _assert_signed_five_face_evidence(
    product, record: RaisedPad, *, corner_blended: bool = False
) -> None:
    """Prove the Candidate owns the authored terminal and four perimeter planes."""

    (candidate,) = product.physical.candidate_set(FamilyId.PADS).candidates
    nodes = product.evidence.defining_of(candidate)
    assert len(nodes) == 5
    axis_index = _AXIS_INDEX[record.axis]
    record_bounds = _record_bounds(record)
    terminal = record_bounds[axis_index][1 if record.direction > 0 else 0]
    base = record_bounds[axis_index][0 if record.direction > 0 else 1]
    top_nodes = [
        node for node in nodes if abs(product.context.graph.normal(node)[axis_index]) >= 0.99
    ]
    assert len(top_nodes) == 1
    top = top_nodes[0]
    assert product.context.graph.normal(top)[axis_index] * record.direction >= 0.99
    assert product.context.graph.bounds(top)[axis_index] == pytest.approx((terminal, terminal))

    walls = [node for node in nodes if node is not top]
    assert len(walls) == 4
    transverse = [index for index in range(3) if index != axis_index]
    observed_roles = []
    for wall in walls:
        normal = product.context.graph.normal(wall)
        wall_axis = next(index for index in transverse if abs(normal[index]) >= 0.99)
        cross_axis = next(index for index in transverse if index != wall_axis)
        bounds = product.context.graph.bounds(wall)
        observed_roles.append((wall_axis, round(bounds[wall_axis][0], 3)))
        assert bounds[wall_axis][0] == pytest.approx(bounds[wall_axis][1])
        if corner_blended:
            assert record_bounds[cross_axis][0] < bounds[cross_axis][0]
            assert bounds[cross_axis][1] < record_bounds[cross_axis][1]
        else:
            assert bounds[cross_axis] == pytest.approx(record_bounds[cross_axis])
        assert bounds[axis_index] == pytest.approx(tuple(sorted((base, terminal))))
    expected_roles = [
        (index, round(position, 3)) for index in transverse for position in record_bounds[index]
    ]
    assert sorted(observed_roles) == sorted(expected_roles)


def _blended_pad(radius: float = 1.0, *, corners: int = 4):
    island = Box(30, 20, 4)
    vertical = tuple(edge for edge in island.edges() if abs(float(edge.tangent_at().Z)) > 0.99)
    return Box(80, 60, 10) + Pos(0, 0, 7) * fillet(vertical[:corners], radius)


def _as_nurbs(part) -> Part:
    return Part(BRepBuilderAPI_NurbsConvert(part.wrapped, True).Shape())


def _perforated_pad(radius: float, *, width: float = 30, depth: float = 20):
    island = Box(width, depth, 4) - Pos(0, 0, -2) * Cylinder(radius, 8)
    return Box(80, 60, 10) + Pos(0, 0, 7) * island


def _through_perforated_pad(radius: float):
    solid = Box(80, 60, 10) + Pos(0, 0, 7) * Box(1, 1, 4)
    return solid - Cylinder(radius, 30)


def _four_edge_pads_with_recesses():
    minimum = (Align.MIN, Align.MIN, Align.MIN)
    part = Box(180, 120, 22, align=minimum)
    for x in (15, 125):
        for y in (0, 102):
            part += Pos(x, y, 22) * Box(40, 18, 14, align=minimum)
    for y in (30, 90):
        part -= Pos(35, y, 14) * extrude(Plane.XY * SlotOverall(42, 18), 8)
    for x in (50, 130):
        part -= Pos(x, 60, -1) * Cylinder(10, 24)
    return part


def _wall_fact(
    face: object,
    *,
    normal_axis: float = 1.0,
    axis: str = "x",
    plane: float = 0.0,
    cross_lo: float = -1.0,
    cross_hi: float = 1.0,
    base: float = 0.0,
    top: float = 2.0,
):
    if axis == "x":
        minimum = SimpleNamespace(X=plane, Y=cross_lo, Z=base)
        maximum = SimpleNamespace(X=plane, Y=cross_hi, Z=top)
        normal = SimpleNamespace(X=normal_axis, Y=0.0, Z=0.0)
    else:
        minimum = SimpleNamespace(X=cross_lo, Y=plane, Z=base)
        maximum = SimpleNamespace(X=cross_hi, Y=plane, Z=top)
        normal = SimpleNamespace(X=0.0, Y=normal_axis, Z=0.0)
    bounds = SimpleNamespace(min=minimum, max=maximum)
    return face, bounds, normal


@dataclass(frozen=True)
class _ExpectedPad:
    record: RaisedPad
    faces: tuple[object, ...]


def _fresh_expected(part, *, tol: float | None = None) -> list[_ExpectedPad]:
    """Rebuild pad occurrences from fresh topology before Candidate inspection."""

    threshold = 0.2 if tol is None else tol
    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    occurrences: list[_ExpectedPad] = []
    for solid in sources:
        bb = solid.bounding_box()
        tops = []
        vertical = []
        for face in solid.faces():
            surface = BRepAdaptor_Surface(face.wrapped)
            if surface.GetType() != GeomAbs_Plane:
                continue
            try:
                normal = face.normal_at()
            except Exception:  # noqa: BLE001 - independently skip degenerate topology
                continue
            bounds = face.bounding_box()
            if normal.Z >= 0.99:
                dx = bounds.max.X - bounds.min.X
                dy = bounds.max.Y - bounds.min.Y
                properties = GProp_GProps()
                BRepGProp.SurfaceProperties_s(face.wrapped, properties)
                full_x = (
                    bb.min.X + threshold >= bounds.min.X and bb.max.X - threshold <= bounds.max.X
                )
                full_y = (
                    bb.min.Y + threshold >= bounds.min.Y and bb.max.Y - threshold <= bounds.max.Y
                )
                if (
                    dx > threshold
                    and dy > threshold
                    and bb.min.Z + threshold < bounds.max.Z
                    and abs(properties.Mass() - dx * dy)
                    <= max(threshold * threshold, 0.005 * dx * dy)
                    and not full_x
                    and not full_y
                ):
                    tops.append(
                        (
                            round(bounds.min.X, 3),
                            round(bounds.max.X, 3),
                            round(bounds.min.Y, 3),
                            round(bounds.max.Y, 3),
                            round(bounds.max.Z, 3),
                            face,
                        )
                    )
            if abs(normal.Z) <= 0.01:
                vertical.append((face, bounds, normal))

        raw_regions = [RaisedPad(x0, x1, y0, y1, z1, z1) for x0, x1, y0, y1, z1, _ in tops]
        proposals = []
        for x0, x1, y0, y1, z1, top in tops:
            role_specs = (
                ("x", x0, y0, y1),
                ("x", x1, y0, y1),
                ("y", y0, x0, x1),
                ("y", y1, x0, x1),
            )
            selected = []
            for axis, position, lo, hi in role_specs:
                matches = []
                for face, bounds, normal in vertical:
                    component = abs(normal.X) if axis == "x" else abs(normal.Y)
                    plane_position = (
                        (bounds.min.X + bounds.max.X) / 2
                        if axis == "x"
                        else (bounds.min.Y + bounds.max.Y) / 2
                    )
                    cross_lo = bounds.min.Y if axis == "x" else bounds.min.X
                    cross_hi = bounds.max.Y if axis == "x" else bounds.max.X
                    if (
                        component >= 0.99
                        and abs(plane_position - position) <= threshold
                        and abs(bounds.max.Z - z1) <= threshold
                        and z1 - threshold > bounds.min.Z
                        and cross_lo <= lo + threshold
                        and cross_hi >= hi - threshold
                    ):
                        matches.append((float(bounds.min.Z), face))
                if not matches:
                    selected = []
                    break
                base = max(item[0] for item in matches)
                maxima = [face for candidate_base, face in matches if candidate_base == base]
                unique = []
                for face in maxima:
                    if not any(face.wrapped.IsSame(other.wrapped) for other in unique):
                        unique.append(face)
                if len(unique) != 1:
                    selected = []
                    break
                selected.append((base, unique[0]))
            if len(selected) != 4:
                continue
            record = RaisedPad(x0, x1, y0, y1, round(max(base for base, _ in selected), 3), z1)
            touches_tier = any(
                abs(other.z1 - record.z0) <= threshold
                and min(record.x1, other.x1) - max(record.x0, other.x0) >= -threshold
                and min(record.y1, other.y1) - max(record.y0, other.y0) >= -threshold
                for other in raw_regions
            )
            if not touches_tier:
                proposals.append(_ExpectedPad(record, (top, *(face for _, face in selected))))

        grouped: dict[RaisedPad, list[_ExpectedPad]] = {}
        for proposal in proposals:
            grouped.setdefault(proposal.record, []).append(proposal)
        for record, group in grouped.items():
            reference = group[0].faces
            assert all(
                len(proposal.faces) == len(reference)
                and all(
                    actual.wrapped.IsSame(expected.wrapped)
                    for actual, expected in zip(proposal.faces, reference, strict=True)
                )
                for proposal in group
            )
            occurrences.append(_ExpectedPad(record, group[0].faces))
    occurrences.sort(key=lambda occurrence: occurrence.record)
    return occurrences


def _claim(part, **kwargs):
    expected = _fresh_expected(part, **kwargs)
    ledger = ClaimLedger(FaceGraph(part))
    public = recognise_rectangular_pads(part, **kwargs)
    assert [record.to_dict() for record in public] == [
        occurrence.record.to_dict() for occurrence in expected
    ]
    records = _discover_rectangular_pads(part, writer=ledger.writer, **kwargs)
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    candidates = ledger.candidate_set(FamilyId.PADS).candidates
    assert len(candidates) == len(records)
    assert all(
        candidate.record is record for candidate, record in zip(candidates, records, strict=True)
    )
    for occurrence, candidate in zip(expected, candidates, strict=True):
        expected_nodes = frozenset(ledger.graph.require_node(face) for face in occurrence.faces)
        assert ledger.defining_of(candidate) == expected_nodes
    return records, candidates, ledger


def _assert_role(record, candidate, ledger) -> None:
    defining = ledger.defining_of(candidate)
    assert len(defining) == 5
    assert ledger.graph.common_valid_solid(defining) is not None
    surface_uses = candidate.evidence.surfaces
    assert {use.node for use in surface_uses} == defining
    assert all(use.surface.kind is SurfaceKind.PLANE for use in surface_uses)
    material = [use.material_side for use in surface_uses if use.material_side is not None]
    assert len(material) == 1
    assert material[0].node in defining and material[0].outward[2] > 0.999
    assert len(material[0].sample_points) >= 2
    top = [
        node
        for node in defining
        if ledger.graph.is_planar(node) and ledger.graph.normal(node)[2] > 0.999
    ]
    walls = [node for node in defining if node not in top]
    assert len(top) == 1 and len(walls) == 4
    top_bounds = ledger.graph.bounds(top[0])
    assert top_bounds[0] == pytest.approx((record.x0, record.x1), abs=0.001)
    assert top_bounds[1] == pytest.approx((record.y0, record.y1), abs=0.001)
    assert top_bounds[2][1] == pytest.approx(record.z1, abs=0.001)
    assert max(ledger.graph.bounds(node)[2][0] for node in walls) == pytest.approx(
        record.z0, abs=0.001
    )
    assert all(
        ledger.graph.is_planar(node) and abs(ledger.graph.normal(node)[2]) < 1e-4 for node in walls
    )


def test_simple_pad_has_exact_top_and_four_wall_roles() -> None:
    (record,), (candidate,), ledger = _claim(_pad())
    _assert_role(record, candidate, ledger)


def test_separate_edge_pads_may_share_merged_stock_wall_faces() -> None:
    records, candidates, ledger = _claim(_four_edge_pads_with_recesses())

    assert len(records) == len(candidates) == 4
    defining = [ledger.defining_of(candidate) for candidate in candidates]
    assert any(
        not left.isdisjoint(right) for left in defining for right in defining if left != right
    )
    assert all(ledger.graph.common_valid_solid(nodes) is not None for nodes in defining)


def test_equal_records_on_coincident_valid_solids_remain_distinct() -> None:
    original = _pad()
    part = Compound([original, copy.deepcopy(original)])
    records, candidates, ledger = _claim(part)
    assert len(records) == 2 and records[0] == records[1] and records[0] is not records[1]
    first, second = (ledger.defining_of(candidate) for candidate in candidates)
    assert first.isdisjoint(second)
    assert ledger.graph.common_valid_solid(first) != ledger.graph.common_valid_solid(second)
    for record, candidate in zip(records, candidates, strict=True):
        _assert_role(record, candidate, ledger)


@pytest.mark.parametrize("unequal", [False, True])
def test_disjoint_same_solid_pad_occurrences_keep_exact_roles(unequal: bool) -> None:
    base = Box(140, 80, 10)
    left = Pos(-40, 0, 7) * Box(24, 18, 4)
    right = Pos(40, 0, 7.5 if unequal else 7) * Box(30 if unequal else 24, 18, 5 if unequal else 4)
    records, candidates, ledger = _claim(base + left + right)
    assert len(records) == 2
    if not unequal:
        assert records[0].z0 == records[1].z0 and records[0].z1 == records[1].z1
    for record, candidate in zip(records, candidates, strict=True):
        _assert_role(record, candidate, ledger)
    assert ledger.defining_of(candidates[0]).isdisjoint(ledger.defining_of(candidates[1]))


def test_intervening_levels_do_not_change_body_local_pad_base() -> None:
    base = Box(120, 70, 8)
    wall = Pos(-50, 0, 20) * Box(10, 70, 32)
    lower_step = Pos(10, 0, 8) * Box(45, 50, 8)
    raised_pad = Pos(30, 0, 20) * Box(24, 18, 16)
    (record,), (candidate,), ledger = _claim(base + wall + lower_step + raised_pad)
    assert record == RaisedPad(18, 42, -9, 9, 12, 28)
    _assert_role(record, candidate, ledger)


def test_tier_context_suppresses_upper_tiny_pad_but_retains_lower_pad() -> None:
    base = Box(80, 60, 10)
    lower = Pos(0, 0, 7) * Box(30, 20, 4)
    upper = Pos(0, 0, 11) * Box(1, 1, 4)
    (record,), (candidate,), ledger = _claim(base + lower + upper)
    assert record == RaisedPad(-15, 15, -10, 10, 5, 9)
    _assert_role(record, candidate, ledger)


def test_sloped_support_keeps_current_highest_wall_base_semantics() -> None:
    support = Rot(0, 8, 0) * Box(80, 60, 10)
    part = support + Pos(0, 0, 7) * Box(20, 20, 4)
    (record,), (candidate,), ledger = _claim(part)
    assert record == RaisedPad(-10, 10, -10, 10, 6.455, 9)
    _assert_role(record, candidate, ledger)


def test_later_body_failure_leaves_family_empty(monkeypatch) -> None:
    part = Compound([Pos(-60, 0, 0) * _pad(), Pos(60, 0, 0) * _pad()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    owners = tuple(
        dict.fromkeys(
            owner for node in ledger.graph.nodes if (owner := original((node,))) is not None
        )
    )
    assert len(owners) == 2

    def fail_second(nodes):
        defining = tuple(nodes)
        owner = original(defining)
        return None if len(defining) == 5 and owner is owners[1] else owner

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_rectangular_pads(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_equal_value_role_permutation_refuses_before_publication(monkeypatch) -> None:
    import quiddity.pads as module

    part = _pad()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._recognise_rectangular_pads_one

    def permuted(source, *, tol, face_surfaces, **orientation):
        proposals = original(source, tol=tol, face_surfaces=face_surfaces, **orientation)
        if orientation.get("axis") != "z" or orientation.get("axis_sign") != 1:
            return proposals
        (proposal,) = proposals
        roles = proposal.wall_roles
        return [
            proposal,
            module._PadProposal(
                proposal.record,
                proposal.top_face,
                (roles[1], roles[0], roles[2], roles[3]),
            ),
        ]

    monkeypatch.setattr(module, "_recognise_rectangular_pads_one", permuted)
    with pytest.raises(ValueError, match="ambiguous defining occurrences"):
        _discover_rectangular_pads(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_distinct_pad_values_cannot_reuse_one_defining_top(monkeypatch) -> None:
    import quiddity.pads as module

    part = _pad()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._recognise_rectangular_pads_one

    def reused_top(source, *, tol, face_surfaces, **orientation):
        proposals = original(source, tol=tol, face_surfaces=face_surfaces, **orientation)
        if orientation.get("axis") != "z" or orientation.get("axis_sign") != 1:
            return proposals
        (proposal,) = proposals
        record = proposal.record
        return [
            proposal,
            module._PadProposal(
                RaisedPad(
                    record.x0,
                    record.x1,
                    record.y0,
                    record.y1,
                    record.z0 - 1.0,
                    record.z1,
                ),
                proposal.top_face,
                proposal.wall_roles,
            ),
        ]

    monkeypatch.setattr(module, "_recognise_rectangular_pads_one", reused_top)
    with pytest.raises(ValueError, match="share a defining top face"):
        _discover_rectangular_pads(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_repeated_shallow_wrappers_collapse_to_same_ordered_roles(monkeypatch) -> None:
    import quiddity.pads as module

    part = _pad()
    original = module._recognise_rectangular_pads_one

    def repeated(source, *, tol, face_surfaces, **orientation):
        proposals = original(source, tol=tol, face_surfaces=face_surfaces, **orientation)
        if orientation.get("axis") != "z" or orientation.get("axis_sign") != 1:
            return proposals
        (proposal,) = proposals
        wrapped = module._PadProposal(
            proposal.record,
            copy.copy(proposal.top_face),
            tuple((copy.copy(role[0]),) for role in proposal.wall_roles),
        )
        return [proposal, wrapped]

    monkeypatch.setattr(module, "_recognise_rectangular_pads_one", repeated)
    (record,), (candidate,), ledger = _claim(part)
    _assert_role(record, candidate, ledger)


def test_late_binding_failure_leaves_family_empty(monkeypatch) -> None:
    part = Compound([Pos(-60, 0, 0) * _pad(), Pos(60, 0, 0) * _pad()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.require_node
    calls = 0

    def fail_later(face):
        nonlocal calls
        calls += 1
        if calls > 5:
            raise ValueError("later Pad binding failed")
        return original(face)

    monkeypatch.setattr(ledger.graph, "require_node", fail_later)
    with pytest.raises(ValueError, match="later Pad binding failed"):
        _discover_rectangular_pads(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_foreign_writer_refuses_before_publication() -> None:
    foreign = ClaimLedger(FaceGraph(Pos(200, 0, 0) * _pad()))
    with pytest.raises(ValueError):
        _discover_rectangular_pads(_pad(), writer=foreign.writer)
    assert foreign.candidate_set(FamilyId.PADS).candidates == ()


def test_foreign_surface_query_refuses_before_pad_discovery() -> None:
    part = _pad()
    ledger = ClaimLedger(FaceGraph(part))
    foreign_query = effective_faces_for_graph(FaceGraph(part))

    with pytest.raises(ValueError, match="different runs"):
        _discover_rectangular_pads(
            part,
            writer=ledger.writer,
            face_surfaces=foreign_query,
        )


def test_foreign_geometry_refuses_before_pad_discovery() -> None:
    part = _pad()
    ledger = ClaimLedger(FaceGraph(part))
    foreign_graph = FaceGraph(part)
    foreign_surfaces = EffectiveSurfaceIndex(foreign_graph)
    foreign_geometry = GeometryGraph._from_graph(foreign_graph, foreign_surfaces)

    with pytest.raises(ValueError, match="different runs"):
        _discover_rectangular_pads(part, writer=ledger.writer, geometry=foreign_geometry)


def test_pad_top_without_material_certificate_remains_suppression_only() -> None:
    part = _pad()
    delegate = effective_faces_for_graph(FaceGraph(part))

    class MissingMaterialCertificate:
        @property
        def run_token(self):
            return delegate.run_token

        def fact(self, face):
            return delegate.fact(face)

        def use(self, face, *, material_side=False):
            return delegate.use(face, material_side=False)

    assert (
        _discover_rectangular_pads(
            part,
            face_surfaces=MissingMaterialCertificate(),
        )
        == []
    )


def test_pad_surface_refusal_during_evidence_issuance_is_atomic() -> None:
    part = _pad()
    ledger = ClaimLedger(FaceGraph(part))
    delegate = effective_faces_for_graph(ledger.graph)
    material_calls: dict[object, int] = {}

    class LateRefusal:
        @property
        def run_token(self):
            return delegate.run_token

        def fact(self, face):
            return delegate.fact(face)

        def use(self, face, *, material_side=False):
            node = ledger.graph.require_node(face)
            if material_side:
                material_calls[node] = material_calls.get(node, 0) + 1
                if material_calls[node] > 1:
                    return SurfaceUseRefusal(node, MaterialSideRefusalReason.SURFACE_UNAVAILABLE)
            return delegate.use(face, material_side=material_side)

    with pytest.raises(ValueError, match="provenance became unavailable"):
        _discover_rectangular_pads(
            part,
            writer=ledger.writer,
            face_surfaces=LateRefusal(),
        )
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_supported_transforms_preserve_writer_lifecycle() -> None:
    original = _pad()
    for part in (
        Pos(17, -9, 4) * original,
        Rot(0, 0, 90) * original,
        original.mirror(Plane.YZ),
        original.scale(0.2),
        original.scale(5),
    ):
        records, candidates, ledger = _claim(part)
        assert records
        for record, candidate in zip(records, candidates, strict=True):
            _assert_role(record, candidate, ledger)


def test_step_round_trip_preserves_pad_role_correspondence(tmp_path) -> None:
    source_records, source_candidates, source_ledger = _claim(_pad())
    target = tmp_path / "pad.step"
    assert export_step(_pad(), target)
    imported_records, imported_candidates, imported_ledger = _claim(import_step(target))
    assert [record.to_dict() for record in imported_records] == [
        record.to_dict() for record in source_records
    ]
    assert [len(source_ledger.defining_of(candidate)) for candidate in source_candidates] == [5]
    assert [len(imported_ledger.defining_of(candidate)) for candidate in imported_candidates] == [5]


def test_reversed_face_traversal_preserves_pad_occurrence_roles(monkeypatch) -> None:
    part = Compound([Pos(-60, 0, 0) * _pad(), Pos(60, 0, 0) * _pad()])
    baseline = [record.to_dict() for record in recognise_rectangular_pads(part)]
    solid_type = type(part.solids()[0])
    original = solid_type.faces

    def reversed_faces(self):
        faces = original(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(solid_type, "faces", reversed_faces)
    records, _candidates, _ledger = _claim(part)
    assert [record.to_dict() for record in records] == baseline


def test_reversed_vertical_face_orientation_preserves_unsigned_wall_roles(monkeypatch) -> None:
    part = _pad()
    solid_type = type(part)
    original = solid_type.faces

    def reversed_vertical(self):
        faces = original(self)
        return type(faces)(
            type(face)(face.wrapped.Reversed()) if abs(face.normal_at().Z) <= 0.01 else face
            for face in faces
        )

    monkeypatch.setattr(solid_type, "faces", reversed_vertical)
    (record,), (candidate,), ledger = _claim(part)
    _assert_role(record, candidate, ledger)


def test_open_shell_refuses_both_entry_points_without_material_side_authority() -> None:
    shell = Shell(_pad().faces())
    assert recognise_rectangular_pads(shell) == []
    ledger = ClaimLedger(FaceGraph(shell))
    assert _discover_rectangular_pads(shell, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize(
    "part",
    [
        Box(80, 60, 10),
        Box(80, 60, 10) + Pos(0, 0, 7) * Box(80, 20, 4),
        Box(80, 60, 10) - Pos(0, 0, 8) * Box(30, 20, 4),
    ],
)
def test_stock_ledge_and_recess_issue_no_pad(part) -> None:
    assert recognise_rectangular_pads(part) == []
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_rectangular_pads(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_custom_tolerance_preserves_full_lifecycle() -> None:
    records, candidates, ledger = _claim(_pad(), tol=0.1)
    assert records
    _assert_role(records[0], candidates[0], ledger)


@pytest.mark.parametrize(
    ("radius", "accepted"),
    [
        (math.sqrt(3 / math.pi) * 0.99, True),
        (math.sqrt(3 / math.pi), True),
        (math.sqrt(3 / math.pi) * 1.01, False),
    ],
)
def test_top_area_deficit_boundary_preserves_current_semantics(radius, accepted) -> None:
    part = _perforated_pad(radius)
    if accepted:
        records, candidates, ledger = _claim(part)
        assert len(records) == 1
        _assert_role(records[0], candidates[0], ledger)
    else:
        assert recognise_rectangular_pads(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_rectangular_pads(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize(
    ("radius", "accepted"),
    [
        (math.sqrt(0.04 / math.pi) * 0.99, True),
        (
            math.nextafter(math.nextafter(math.sqrt(0.04 / math.pi), 0.0), 0.0),
            True,
        ),
        (math.sqrt(0.04 / math.pi), False),
    ],
)
def test_absolute_area_floor_boundary_preserves_current_semantics(radius, accepted) -> None:
    part = _through_perforated_pad(radius)
    if accepted:
        records, candidates, ledger = _claim(part)
        assert len(records) == 1
        _assert_role(records[0], candidates[0], ledger)
    else:
        assert recognise_rectangular_pads(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_rectangular_pads(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_stock_envelope_wall_remains_defining_below_local_base() -> None:
    part = Box(80, 60, 10) + Pos(25, 0, 7) * Box(30, 20, 4)
    (record,), (candidate,), ledger = _claim(part)
    defining = ledger.defining_of(candidate)
    walls = [node for node in defining if abs(ledger.graph.normal(node)[2]) <= 0.01]
    assert record.z0 == 5
    assert min(ledger.graph.bounds(node)[2][0] for node in walls) == pytest.approx(
        part.bounding_box().min.Z
    )
    assert max(ledger.graph.bounds(node)[2][0] for node in walls) == pytest.approx(record.z0)


@pytest.mark.parametrize(("height", "accepted"), [(0.199, False), (0.2, False), (0.201, True)])
def test_absolute_height_threshold_is_strict(height: float, accepted: bool) -> None:
    part = Box(80, 60, 10) + Pos(0, 0, 5 + height / 2) * Box(30, 20, height)
    if accepted:
        records, candidates, ledger = _claim(part)
        assert len(records) == 1
        _assert_role(records[0], candidates[0], ledger)
    else:
        assert recognise_rectangular_pads(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_rectangular_pads(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize(("width", "accepted"), [(0.199, False), (0.2, False), (0.201, True)])
@pytest.mark.parametrize("axis", ["x", "y"])
def test_footprint_width_threshold_is_strict(width: float, accepted: bool, axis: str) -> None:
    island = Box(width, 2, 2) if axis == "x" else Box(2, width, 2)
    part = Box(20, 20, 2) + Pos(0, 0, 2) * island
    if accepted:
        records, candidates, ledger = _claim(part)
        assert len(records) == 1
        _assert_role(records[0], candidates[0], ledger)
    else:
        assert recognise_rectangular_pads(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_rectangular_pads(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize(("width", "accepted"), [(19.598, True), (19.6, False), (19.602, False)])
@pytest.mark.parametrize("axis", ["x", "y"])
def test_full_span_margin_boundary_is_inclusive(width: float, accepted: bool, axis: str) -> None:
    island = Box(width, 2, 2) if axis == "x" else Box(2, width, 2)
    part = Box(20, 20, 2) + Pos(0, 0, 2) * island
    if accepted:
        records, candidates, ledger = _claim(part)
        assert len(records) == 1
        _assert_role(records[0], candidates[0], ledger)
    else:
        assert recognise_rectangular_pads(part) == []
        ledger = ClaimLedger(FaceGraph(part))
        assert _discover_rectangular_pads(part, writer=ledger.writer) == []
        assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_tied_signed_axis_readings_refuse_without_iteration_order_preference() -> None:
    part = Box(20, 20, 2) + Pos(0, 0, 2) * Box(19.6, 2, 2)
    surfaces = effective_faces_for_part(part)
    positive = _recognise_rectangular_pads_one(
        part, tol=None, face_surfaces=surfaces, axis="x", axis_sign=1
    )
    negative = _recognise_rectangular_pads_one(
        part, tol=None, face_surfaces=surfaces, axis="x", axis_sign=-1
    )

    assert len(positive) == len(negative) == 1
    assert positive[0].record.direction == 1
    assert negative[0].record.direction == -1
    assert _record_bounds(positive[0].record) == _record_bounds(negative[0].record)
    assert recognise_rectangular_pads(part) == []
    assert recognise_rectangular_pads(Rot(0, 90, 0) * part) == []


@pytest.mark.parametrize(
    ("changes", "accepted"),
    [
        ({"normal_axis": 0.99}, True),
        ({"normal_axis": 0.9899}, False),
        ({"plane": 0.25}, True),
        ({"plane": 0.2501}, False),
        ({"top": 2.25}, True),
        ({"top": 2.2501}, False),
        ({"base": 1.7499}, True),
        ({"base": 1.75}, False),
        ({"cross_lo": -0.75}, True),
        ({"cross_lo": -0.7499}, False),
        ({"cross_hi": 0.75}, True),
        ({"cross_hi": 0.7499}, False),
    ],
)
@pytest.mark.parametrize("axis", ["x", "y"])
def test_wall_role_predicate_boundaries(
    changes: dict[str, float], accepted: bool, axis: str
) -> None:
    face = object()
    result = _wall_role(
        [_wall_fact(face, axis=axis, **changes)],
        axis=axis,
        pos=0,
        lo=-1,
        hi=1,
        top=2,
        tol=0.25,
    )
    assert (result is not None) is accepted
    if accepted:
        assert result is not None and result[1] == (face,)


def test_wall_role_selects_every_exact_maximal_base_tie() -> None:
    low, high, tied = object(), object(), object()
    result = _wall_role(
        [
            _wall_fact(low, base=0),
            _wall_fact(high, base=1),
            _wall_fact(tied, base=1),
        ],
        axis="x",
        pos=0,
        lo=-1,
        hi=1,
        top=2,
        tol=0.2,
    )
    assert result == (1, (high, tied))


def test_one_wall_can_match_opposed_roles_at_exact_twice_tolerance() -> None:
    face = object()
    facts = [_wall_fact(face)]
    left = _wall_role(facts, axis="x", pos=-0.25, lo=-1, hi=1, top=2, tol=0.25)
    right = _wall_role(facts, axis="x", pos=0.25, lo=-1, hi=1, top=2, tol=0.25)
    assert left is not None and right is not None
    assert left[1] == right[1] == (face,)


@pytest.mark.parametrize(
    ("z_delta", "plan_gap", "accepted"),
    [
        (0.25, 0.25, True),
        (0.2501, 0.25, False),
        (0.25, 0.2501, False),
    ],
)
def test_tier_suppression_boundaries(z_delta: float, plan_gap: float, accepted: bool) -> None:
    pad = RaisedPad(0, 1, 0, 1, 2, 3)
    region = RaisedPad(1 + plan_gap, 2 + plan_gap, 0, 1, 2 + z_delta, 2 + z_delta)
    assert _tier_suppresses(pad, region, tol=0.25) is accepted


def test_small_pad_on_large_part_remains_attributed() -> None:
    part = Box(10_000, 10_000, 10) + Pos(0, 0, 6) * Box(1, 1, 2)
    (record,), (candidate,), ledger = _claim(part)
    _assert_role(record, candidate, ledger)


@pytest.mark.parametrize("tol", [-0.1, float("nan"), float("inf")])
def test_existing_invalid_tolerance_behavior_remains_empty(tol: float) -> None:
    part = _pad()
    assert recognise_rectangular_pads(part, tol=tol) == []
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_rectangular_pads(part, tol=tol, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize(
    "part",
    [Rot(8, 0, 0) * _pad(), Box(80, 60, 10) + Pos(0, 0, 7) * Cylinder(8, 4)],
)
def test_non_z_and_curved_top_shapes_issue_no_pad(part) -> None:
    assert recognise_rectangular_pads(part) == []
    ledger = ClaimLedger(FaceGraph(part))
    assert _discover_rectangular_pads(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize("mode", ["role_alias", "tied_maximum", "deep_top", "stale_top"])
def test_invalid_role_snapshots_refuse_before_publication(monkeypatch, mode: str) -> None:
    import quiddity.pads as module

    part = _pad()
    ledger = ClaimLedger(FaceGraph(part))
    original = module._recognise_rectangular_pads_one

    def changed(source, *, tol, face_surfaces, **orientation):
        proposals = original(source, tol=tol, face_surfaces=face_surfaces, **orientation)
        if orientation.get("axis") != "z" or orientation.get("axis_sign") != 1:
            return proposals
        (proposal,) = proposals
        roles = proposal.wall_roles
        if mode == "role_alias":
            roles = (roles[0], roles[0], roles[2], roles[3])
        elif mode == "tied_maximum":
            roles = ((roles[0][0], roles[1][0]), roles[1], roles[2], roles[3])
        top = proposal.top_face
        if mode in {"deep_top", "stale_top"}:
            top = copy.deepcopy(top)
            if mode == "stale_top":
                top = Pos(1, 0, 0) * top
        return [module._PadProposal(proposal.record, top, roles)]

    monkeypatch.setattr(module, "_recognise_rectangular_pads_one", changed)
    with pytest.raises(ValueError):
        _discover_rectangular_pads(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_cross_occurrence_role_reuse_refuses_before_publication(monkeypatch) -> None:
    import quiddity.pads as module

    part = Compound([Pos(-60, 0, 0) * _pad(), Pos(60, 0, 0) * _pad()])
    ledger = ClaimLedger(FaceGraph(part))
    original = module._recognise_rectangular_pads_one
    first_roles = None

    def reused(source, *, tol, face_surfaces, **orientation):
        nonlocal first_roles
        proposals = original(source, tol=tol, face_surfaces=face_surfaces, **orientation)
        if orientation.get("axis") != "z" or orientation.get("axis_sign") != 1:
            return proposals
        if first_roles is None:
            first_roles = proposals[0].wall_roles
            return proposals
        proposal = proposals[0]
        return [module._PadProposal(proposal.record, proposal.top_face, first_roles)]

    monkeypatch.setattr(module, "_recognise_rectangular_pads_one", reused)
    with pytest.raises(ValueError):
        _discover_rectangular_pads(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize(
    ("rotation", "axis", "direction"),
    [
        (Rot(0, 0, 0), "z", 1),
        (Rot(180, 0, 0), "z", -1),
        (Rot(0, 90, 0), "x", 1),
        (Rot(0, -90, 0), "x", -1),
        (Rot(-90, 0, 0), "y", 1),
        (Rot(90, 0, 0), "y", -1),
    ],
)
def test_signed_principal_pads_preserve_authored_parameter_fidelity(
    rotation, axis: str, direction: int
) -> None:
    part, island = _oriented_pad(rotation)

    (record,) = recognise_rectangular_pads(part)

    _assert_record_matches_authored_island(record, island, axis=axis, direction=direction)
    product = _take_inventory(part)
    assert product.result.pads == (record,)
    _assert_signed_five_face_evidence(product, record)


@pytest.mark.parametrize(
    "rotation",
    [Rot(180, 0, 0), Rot(0, 90, 0), Rot(0, -90, 0), Rot(-90, 0, 0), Rot(90, 0, 0)],
)
def test_signed_principal_pad_aggregate_has_one_owner(rotation) -> None:
    part, _island = _oriented_pad(rotation)

    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.PADS).candidates

    assert len(product.result.pads) == len(candidates) == 1
    assert candidates[0].record is product.result.pads[0]
    assert len(product.evidence.defining_of(candidates[0])) == 5


def test_equal_oriented_pads_remain_distinct_and_body_local() -> None:
    left, _ = _oriented_pad(Rot(0, 90, 0))
    right = Pos(0, 100, 0) * left
    part = Compound([left, right])

    records = recognise_rectangular_pads(part)

    assert len(records) == 2
    assert {record.axis for record in records} == {"x"}
    assert {record.direction for record in records} == {1}
    assert records[0].y1 < records[1].y0


def test_pad_survives_arbitrary_rigid_motion_through_framed_aggregate() -> None:
    minimum_z = (Align.CENTER, Align.CENTER, Align.MIN)
    source = Box(100, 70, 12, align=minimum_z)
    source += Pos(18, -10, 12) * Box(28, 16, 8, align=minimum_z)
    moved = Pos(17, -23, 9) * Rot(31, 47, 13) * source

    framed = build_framed_recognition_result(moved, rotational=False)

    assert isinstance(framed, FramedRecognitionResult)
    (record,) = framed.result.pads
    assert record.axis in "xyz"
    assert record.direction in {-1, 1}
    axial = _record_bounds(record)[_AXIS_INDEX[record.axis]]
    transverse = [
        span
        for index, span in enumerate(_record_bounds(record))
        if index != _AXIS_INDEX[record.axis]
    ]
    assert axial[1] - axial[0] == pytest.approx(8, abs=1e-3)
    assert sorted(hi - lo for lo, hi in transverse) == pytest.approx([16, 28], abs=1e-3)
    ledger = ClaimLedger(FaceGraph(framed.part))
    direct = recognise_rectangular_pads(framed.part)
    attributed = _discover_rectangular_pads(framed.part, writer=ledger.writer)
    candidates = ledger.candidate_set(FamilyId.PADS).candidates
    assert direct == attributed == [record]
    assert len(candidates) == 1
    assert len(ledger.defining_of(candidates[0])) == 5


def test_signed_principal_pad_step_round_trip_preserves_record(tmp_path: Path) -> None:
    part, _island = _oriented_pad(Rot(0, -90, 0))
    path = tmp_path / "negative-x-pad.step"
    export_step(part, path)

    imported = import_step(path)

    assert recognise_rectangular_pads(imported) == recognise_rectangular_pads(part)


def test_private_core_has_one_production_writer_caller_and_three_record_paths() -> None:
    core_sites: list[tuple[str, ast.Call]] = []
    constructors: list[tuple[str, ast.Call]] = []
    for path in (ROOT / "src/quiddity").glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for qualified, call in _qualified_calls(tree):
            if qualified.endswith("._discover_rectangular_pads") or qualified == (
                "_discover_rectangular_pads"
            ):
                core_sites.append((path.name, call))
            if qualified.endswith(".RaisedPad") or qualified == "RaisedPad":
                constructors.append((path.name, call))

    assert {path for path, _call in core_sites} == {"pads.py", "_registry.py"}
    registry_call = next(call for path, call in core_sites if path == "_registry.py")
    keywords = {keyword.arg: keyword.value for keyword in registry_call.keywords}
    writer = keywords["writer"]
    assert isinstance(writer, ast.Attribute) and writer.attr == "writer"
    assert isinstance(writer.value, ast.Name) and writer.value.id == "s"
    assert [(path, len(call.args)) for path, call in constructors] == [("pads.py", 0)]


def test_terminal_inventory_retains_nonempty_pad_identity() -> None:
    product = _take_inventory(_pad())
    candidates = product.physical.candidate_set(FamilyId.PADS).candidates
    assert len(candidates) == len(product.result.pads) == 1
    assert candidates[0].record is product.result.pads[0]
    assert len(product.evidence.defining_of(candidates[0])) == 5


@pytest.mark.parametrize("radius", [0.5, 1.0, 2.0])
def test_complete_corner_blend_cycle_preserves_exact_pad_and_evidence(radius) -> None:
    sharp = recognise_rectangular_pads(_pad())
    part = _blended_pad(radius)

    assert [record.to_dict() for record in recognise_rectangular_pads(part)] == [
        record.to_dict() for record in sharp
    ]
    product = _take_inventory(part)
    assert [record.to_dict() for record in product.result.pads] == [
        record.to_dict() for record in sharp
    ]
    (candidate,) = product.physical.candidate_set(FamilyId.PADS).candidates
    defining = product.evidence.defining_of(candidate)
    assert len(defining) == 5
    assert all(product.context.graph.is_planar(node) for node in defining)
    assert all(product.context.surfaces.fact(node).kind is SurfaceKind.PLANE for node in defining)


@pytest.mark.parametrize(
    "rotation",
    [Rot(180, 0, 0), Rot(0, 90, 0), Rot(0, -90, 0), Rot(-90, 0, 0), Rot(90, 0, 0)],
)
def test_complete_corner_blend_cycle_is_signed_principal_axis_covariant(rotation) -> None:
    sharp, _island = _oriented_pad(rotation)
    rounded = rotation * _blended_pad()

    assert recognise_rectangular_pads(rounded) == recognise_rectangular_pads(sharp)
    product = _take_inventory(rounded)
    assert product.result.pads == tuple(recognise_rectangular_pads(sharp))
    _assert_signed_five_face_evidence(product, product.result.pads[0], corner_blended=True)


@pytest.mark.parametrize(
    "rotation",
    [Rot(0, 0, 0), Rot(180, 0, 0), Rot(0, 90, 0), Rot(0, -90, 0), Rot(90, 0, 0)],
)
def test_full_span_step_remains_negative_across_signed_principal_axes(rotation) -> None:
    full_span = Box(80, 60, 10) + Pos(0, 0, 7) * Box(80, 20, 4)

    assert recognise_rectangular_pads(rotation * full_span) == []


@pytest.mark.parametrize("rotation", [Rot(0, 90, 0), Rot(90, 0, 0)])
def test_rotated_non_pad_controls_remain_negative_or_independently_owned(rotation) -> None:
    pocket = Box(80, 60, 10) - Pos(0, 0, 3) * Box(30, 20, 4)
    polygonal = Box(80, 60, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(10, 6), 4)
    detached = Compound([Box(80, 60, 10), Pos(0, 0, 20) * Box(30, 20, 4)])
    staircase = Box(80, 60, 10) + Pos(0, 0, 7) * Box(30, 20, 4) + Pos(0, 0, 11) * Box(10, 8, 4)

    assert recognise_rectangular_pads(rotation * pocket) == []
    assert recognise_rectangular_pads(rotation * polygonal) == []
    assert recognise_rectangular_pads(rotation * detached) == []
    records = recognise_rectangular_pads(rotation * staircase)
    assert len(records) == 1
    assert _axial_extent(records[0]) == pytest.approx(4)


def test_partial_corner_blend_cycle_does_not_select_a_subset() -> None:
    assert recognise_rectangular_pads(_blended_pad(corners=3)) == []


def test_duplicate_corner_chain_refuses_ambiguous_cycle(monkeypatch) -> None:
    part = _blended_pad()
    graph = FaceGraph(part)
    surfaces = EffectiveSurfaceIndex(graph)
    query = effective_faces_for_graph(graph, surfaces)
    geometry = GeometryGraph._from_graph(graph, surfaces)
    original = GeometryGraph.blend_facts

    def duplicated(self):
        chains = original(self)
        return (*chains, chains[0])

    monkeypatch.setattr(GeometryGraph, "blend_facts", duplicated)
    assert _discover_rectangular_pads(part, face_surfaces=query, geometry=geometry) == []


@pytest.mark.parametrize("mode", ("concave", "multi-node"))
def test_ineligible_corner_chain_is_not_selected(monkeypatch, mode: str) -> None:
    part = _blended_pad()
    graph = FaceGraph(part)
    surfaces = EffectiveSurfaceIndex(graph)
    query = effective_faces_for_graph(graph, surfaces)
    geometry = GeometryGraph._from_graph(graph, surfaces)
    original = GeometryGraph.blend_facts

    def ineligible(self):
        chains = list(original(self))
        first = chains[0]
        if mode == "concave":
            chains[0] = replace(first, side="concave")
        else:
            extra = next(
                geometry.ref(face)
                for face in part.faces()
                if geometry.ref(face) not in first.supports[0]
            )
            chains[0] = replace(first, supports=(first.supports[0] | {extra}, first.supports[1]))
        return tuple(chains)

    monkeypatch.setattr(GeometryGraph, "blend_facts", ineligible)
    assert _discover_rectangular_pads(part, face_surfaces=query, geometry=geometry) == []


def test_non_cylindrical_corner_chain_is_not_selected(monkeypatch) -> None:
    part = _blended_pad()
    graph = FaceGraph(part)
    surfaces = EffectiveSurfaceIndex(graph)
    query = effective_faces_for_graph(graph, surfaces)
    geometry = GeometryGraph._from_graph(graph, surfaces)
    target = next(iter(geometry.blend_facts()[0].blend_faces))
    original = GeometryGraph.surface_fact

    def planar(self, ref):
        fact = original(self, ref)
        return replace(fact, kind=SurfaceKind.PLANE) if ref is target else fact

    monkeypatch.setattr(GeometryGraph, "surface_fact", planar)
    assert _discover_rectangular_pads(part, face_surfaces=query, geometry=geometry) == []


def test_unexplained_rounded_top_area_is_not_selected(monkeypatch) -> None:
    import quiddity.pads as module

    monkeypatch.setattr(module, "_surface_area", lambda _face: 0.0)
    assert recognise_rectangular_pads(_blended_pad()) == []


@pytest.mark.parametrize("mode", ("cycle", "chain"))
def test_invalid_collapsed_bridge_shape_refuses_atomically(monkeypatch, mode: str) -> None:
    part = _blended_pad()
    ledger = ClaimLedger(FaceGraph(part))
    surfaces = EffectiveSurfaceIndex(ledger.graph)
    query = effective_faces_for_graph(ledger.graph, surfaces)
    geometry = GeometryGraph._from_graph(ledger.graph, surfaces)
    original = GeometryGraph.collapsed_bridges

    def invalid(self, selected):
        bridges = original(self, selected)
        if mode == "cycle":
            return bridges[:-1]
        return (replace(bridges[0], supports=()), *bridges[1:])

    monkeypatch.setattr(GeometryGraph, "collapsed_bridges", invalid)
    with pytest.raises(ValueError, match="no unique logical bridge"):
        _discover_rectangular_pads(
            part, writer=ledger.writer, face_surfaces=query, geometry=geometry
        )
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


def test_corrupted_pad_blend_expansion_refuses_before_publication(monkeypatch) -> None:
    part = _blended_pad()
    ledger = ClaimLedger(FaceGraph(part))
    surfaces = EffectiveSurfaceIndex(ledger.graph)
    query = effective_faces_for_graph(ledger.graph, surfaces)
    geometry = GeometryGraph._from_graph(ledger.graph, surfaces)
    original = GeometryGraph.collapsed_bridges

    def corrupted(self, selected):
        bridges = original(self, selected)
        first = bridges[0]
        return (
            replace(first, provenance=GeometryProvenance(frozenset(), first.provenance.boundary)),
            *bridges[1:],
        )

    monkeypatch.setattr(GeometryGraph, "collapsed_bridges", corrupted)
    with pytest.raises(ValueError, match="lost original provenance"):
        _discover_rectangular_pads(
            part,
            writer=ledger.writer,
            face_surfaces=query,
            geometry=geometry,
        )
    assert ledger.candidate_set(FamilyId.PADS).candidates == ()


@pytest.mark.parametrize("radius", (0.1, 0.2, 0.5, 2.0))
def test_rounded_perforated_top_cannot_borrow_corner_cycle(radius: float) -> None:
    part = _blended_pad() - Cylinder(radius, 30)

    assert recognise_rectangular_pads(part) == []
    product = _take_inventory(part)
    assert product.result.pads == ()
    assert product.physical.candidate_set(FamilyId.PADS).candidates == ()


def test_blended_pad_survives_step_roundtrip(tmp_path) -> None:
    path = tmp_path / "blended-pad.step"
    export_step(_blended_pad(), path)
    imported = import_step(path)
    assert [record.to_dict() for record in recognise_rectangular_pads(imported)] == [
        record.to_dict() for record in recognise_rectangular_pads(_pad())
    ]


def test_nurbs_conversion_recovers_pad_standalone_and_aggregate() -> None:
    native = [record.to_dict() for record in recognise_rectangular_pads(_pad())]
    converted = _as_nurbs(_pad())

    assert [record.to_dict() for record in recognise_rectangular_pads(converted)] == native

    product = _take_inventory(converted)
    assert [record.to_dict() for record in product.result.pads] == native
    (candidate,) = product.physical.candidate_set(FamilyId.PADS).candidates
    defining = product.evidence.defining_of(candidate)
    assert len(defining) == 5
    facts = tuple(product.context.surfaces.fact(node) for node in defining)
    assert all(
        isinstance(fact, AnalyticSurfaceFact) and fact.provenance is SurfaceProvenance.RECOVERED
        for fact in facts
    )
    uses = candidate.evidence.surfaces
    assert len(uses) == 5
    assert all(use.surface.provenance is SurfaceProvenance.RECOVERED for use in uses)
    material = [use.material_side for use in uses if use.material_side is not None]
    assert len(material) == 1
    top_use = next(use for use in uses if use.material_side is not None)
    assert top_use.surface.certificate is not None
    assert top_use.node is material[0].node
    assert material[0].outward == pytest.approx((0.0, 0.0, 1.0))


@pytest.mark.parametrize(
    ("part", "expected"),
    [
        (
            Box(80, 60, 10) + Pos(0, 0, 7) * Box(30, 20, 4) + Pos(0, 0, 11) * Box(1, 1, 4),
            1,
        ),
        (Box(80, 60, 10) + Pos(25, 0, 7) * Box(30, 20, 4), 1),
        (
            Compound([Pos(-60, 0, 0) * _pad(), Pos(60, 0, 0) * _pad()]),
            2,
        ),
        (Shell(_pad().faces()), 0),
    ],
)
def test_nurbs_pad_adversaries_preserve_tiers_envelope_and_ownership(part, expected) -> None:
    converted = _as_nurbs(part)
    native_records = recognise_rectangular_pads(part)
    converted_records = recognise_rectangular_pads(converted)
    assert len(native_records) == len(converted_records) == expected
    assert [record.to_dict() for record in converted_records] == [
        record.to_dict() for record in native_records
    ]
    assert [record.to_dict() for record in _take_inventory(converted).result.pads] == [
        record.to_dict() for record in converted_records
    ]


def test_nurbs_pad_refuses_ambiguous_recovery_in_both_entry_points(monkeypatch) -> None:
    import quiddity._effective_surfaces as surfaces

    converted = _as_nurbs(_pad())

    def ambiguous(self, node):
        return RefusedSurfaceFact(node, SurfaceRefusalReason.AMBIGUOUS_PRIMITIVE)

    monkeypatch.setattr(surfaces.EffectiveSurfaceIndex, "fact", ambiguous)
    assert recognise_rectangular_pads(converted) == []
    assert _take_inventory(converted).result.pads == ()


@pytest.mark.parametrize("part_factory", (_pad, _blended_pad))
def test_pad_refuses_disagreeing_material_samples_in_both_entry_points(
    monkeypatch, part_factory
) -> None:
    import quiddity._effective_surfaces as surfaces

    monkeypatch.setattr(
        surfaces._EffectiveFaceSurfaces,
        "_certify_plane",
        lambda _self, _node, _surface: MaterialSideRefusalReason.SAMPLES_DISAGREE,
    )
    assert recognise_rectangular_pads(part_factory()) == []
    assert _take_inventory(part_factory()).result.pads == ()


def test_refused_lower_tier_cannot_introduce_the_upper_pad(monkeypatch) -> None:
    import quiddity._effective_surfaces as surfaces

    original = surfaces._EffectiveFaceSurfaces._certify_plane

    def refuse_lower(self, node, surface):
        if surface.kind is SurfaceKind.PLANE and abs(surface.parameters[3] - 9.0) <= 1e-9:
            return MaterialSideRefusalReason.SAMPLES_DISAGREE
        return original(self, node, surface)

    monkeypatch.setattr(surfaces._EffectiveFaceSurfaces, "_certify_plane", refuse_lower)
    part = Box(80, 60, 10) + Pos(0, 0, 7) * Box(30, 20, 4) + Pos(0, 0, 11) * Box(1, 1, 4)

    assert recognise_rectangular_pads(part) == []
    assert _take_inventory(part).result.pads == ()


@pytest.mark.parametrize(
    "part",
    [
        Box(80, 60, 10),
        Box(80, 60, 10) - Pos(0, 0, 8) * Box(30, 20, 4),
        Box(80, 60, 10) + Pos(0, 0, 7) * Box(80, 20, 4),
    ],
)
def test_nurbs_conversion_does_not_turn_stock_ledges_or_recesses_into_pads(part) -> None:
    assert recognise_rectangular_pads(_as_nurbs(part)) == []
