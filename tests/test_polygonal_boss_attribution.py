"""F5: Polygonal Bosses own exactly their six original vertical side faces."""

from __future__ import annotations

import ast
import copy
import math
from collections import Counter
from dataclasses import dataclass, replace
from pathlib import Path

import pytest
from build123d import (
    Box,
    Compound,
    Cylinder,
    Plane,
    Polygon,
    Pos,
    RegularPolygon,
    Rot,
    export_step,
    extrude,
    fillet,
    import_step,
)

import quiddity.polygonal_bosses as polygonal_module
from quiddity import (
    FramedRecognitionResult,
    PolygonalBoss,
    build_framed_recognition_result,
    recognise_fillets,
    recognise_polygonal_bosses,
)
from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._blend_view import (
    BlendCollapseIndex,
    CollapsedGraphView,
    FrozenProvenance,
)
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._effective_surfaces import AnalyticSurfaceFact, EffectiveSurfaceIndex
from quiddity._geometry import AXIS_ALIGNED_COS
from quiddity.experimental_geometry import GeometryGraph
from quiddity.polygonal_bosses import _discover_polygonal_bosses
from quiddity.result import _take_inventory

ROOT = Path(__file__).parents[1]
_ANGLE_TOL = math.radians(2)


@dataclass(frozen=True)
class _Expected:
    record: PolygonalBoss
    sides: tuple
    context: tuple


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


def _attached(*, x: float = 0.0, radius: float = 20.0, scale: float = 1.0):
    plate = Box(100 * scale, 80 * scale, 10 * scale)
    prism = Pos(x, 0, 5 * scale) * extrude(RegularPolygon(radius * scale, 6), 30 * scale)
    return plate + prism


def _blend_interrupted_attached():
    prism = extrude(RegularPolygon(20, 6), 30)
    vertical = [edge for edge in prism.edges() if abs(float(edge.tangent_at().Z)) > 0.99]
    return Box(100, 80, 10) + Pos(0, 0, 5) * fillet(vertical, 2)


def _partially_blend_interrupted_attached():
    prism = extrude(RegularPolygon(20, 6), 30)
    vertical = [edge for edge in prism.edges() if abs(float(edge.tangent_at().Z)) > 0.99]
    return Box(100, 80, 10) + Pos(0, 0, 5) * fillet(vertical[:5], 2)


def _independent_prismatic_fillet():
    box = Box(40, 30, 20)
    vertical = [edge for edge in box.edges() if abs(float(edge.tangent_at().Z)) > 0.99]
    return fillet(vertical, 2)


class _ReversedFacesPart:
    def __init__(self, part) -> None:
        self._part = part

    def faces(self):
        return list(reversed(self._part.faces()))

    def solids(self):
        return list(reversed(self._part.solids()))

    def __getattr__(self, name):
        return getattr(self._part, name)


def _irregular_hexagon(radius=20.0, height=30.0):
    points = []
    for index in range(6):
        angle = 2 * math.pi * index / 6
        scale = 1.35 if index % 2 == 0 else 1.0
        points.append((radius * scale * math.cos(angle), radius * scale * math.sin(angle)))
    return extrude(Polygon(*points), height)


def _claim(part, **kwargs):
    ledger = ClaimLedger(FaceGraph(part))
    public = recognise_polygonal_bosses(part, **kwargs)
    records = _discover_polygonal_bosses(
        part, graph=GeometryGraph._from_graph(ledger.graph), writer=ledger.writer, **kwargs
    )
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    candidates = ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates
    assert len(candidates) == len(records)
    assert all(
        candidate.record is record for candidate, record in zip(candidates, records, strict=True)
    )
    return records, candidates, ledger


def _fresh_expected(part, *, tol=0.2, angle_tol=_ANGLE_TOL) -> list[_Expected]:
    occurrences = []
    for solid in part.solids():
        fresh = FaceGraph(solid)
        vertical = []
        for node in fresh.nodes:
            if not fresh.is_planar(node):
                continue
            normal = fresh.normal(node)
            lo, hi = fresh.bounds(node)[2]
            if normal is not None and abs(normal[2]) <= 0.02 and hi - lo > tol:
                vertical.append(node)

        unseen = set(vertical)
        components = []
        while unseen:
            seed = unseen.pop()
            component = {seed}
            frontier = [seed]
            while frontier:
                node = frontier.pop()
                lo, hi = fresh.bounds(node)[2]
                for other in tuple(unseen & set(fresh.neighbours(node))):
                    other_lo, other_hi = fresh.bounds(other)[2]
                    if abs(lo - other_lo) <= tol and abs(hi - other_hi) <= tol:
                        unseen.remove(other)
                        component.add(other)
                        frontier.append(other)
            components.append(tuple(component))

        for component in components:
            if len(component) != 6:
                continue
            component_set = set(component)
            if any(len(set(fresh.neighbours(node)) & component_set) != 2 for node in component):
                continue
            ordered = tuple(
                sorted(
                    component,
                    key=lambda node: math.atan2(fresh.normal(node)[1], fresh.normal(node)[0]),
                )
            )
            normals = [fresh.normal(node) for node in ordered]
            angles = [math.atan2(normal[1], normal[0]) % (2 * math.pi) for normal in normals]
            gaps = [(angles[(index + 1) % 6] - angles[index]) % (2 * math.pi) for index in range(6)]
            if any(abs(gap - math.pi / 3) > angle_tol for gap in gaps):
                continue
            if any(
                normals[index][0] * normals[index + 3][0]
                + normals[index][1] * normals[index + 3][1]
                > -math.cos(angle_tol)
                for index in range(3)
            ):
                continue
            centres = [fresh.face(node).center() for node in ordered]
            offsets = [
                normal[0] * point.X + normal[1] * point.Y
                for normal, point in zip(normals, centres, strict=True)
            ]
            midplanes = [
                (
                    normals[index][0],
                    normals[index][1],
                    (offsets[index] - offsets[index + 3]) / 2,
                )
                for index in range(3)
            ]
            sxx = sum(nx * nx for nx, _ny, _offset in midplanes)
            sxy = sum(nx * ny for nx, ny, _offset in midplanes)
            syy = sum(ny * ny for _nx, ny, _offset in midplanes)
            bx = sum(nx * offset for nx, _ny, offset in midplanes)
            by = sum(ny * offset for _nx, ny, offset in midplanes)
            determinant = sxx * syy - sxy * sxy
            cx = (bx * syy - by * sxy) / determinant
            cy = (sxx * by - sxy * bx) / determinant
            supports = [
                offset - normal[0] * cx - normal[1] * cy
                for normal, offset in zip(normals, offsets, strict=True)
            ]
            across_values = [supports[index] + supports[index + 3] for index in range(3)]
            across = sum(across_values) / 3
            if (
                min(supports) <= tol
                or max(abs(value - across) for value in across_values) > tol
                or max(abs(value - across / 2) for value in supports) > tol
            ):
                continue
            wall_lo = sum(fresh.bounds(node)[2][0] for node in component) / 6
            wall_hi = sum(fresh.bounds(node)[2][1] for node in component) / 6

            def cap(
                *,
                upper,
                component=component,
                component_set=component_set,
                fresh=fresh,
                wall_hi=wall_hi,
                wall_lo=wall_lo,
            ):
                boundary = []
                for side in component:
                    choices = []
                    for other in set(fresh.neighbours(side)) - component_set:
                        lo, hi = fresh.bounds(other)[2]
                        if abs((lo if upper else hi) - (wall_hi if upper else wall_lo)) <= tol:
                            choices.append(other)
                    if len(choices) != 1:
                        return None
                    boundary.append(choices[0])
                boundary_set = set(boundary)
                candidates = (
                    boundary_set
                    if len(boundary_set) == 1
                    else set.intersection(*(set(fresh.neighbours(node)) for node in boundary_set))
                    - component_set
                    - boundary_set
                )
                valid = []
                for node in candidates:
                    if not fresh.is_planar(node):
                        continue
                    normal = fresh.normal(node)
                    lo, hi = fresh.bounds(node)[2]
                    z = (lo + hi) / 2
                    if (
                        normal is not None
                        and normal[2] >= AXIS_ALIGNED_COS
                        and hi - lo <= tol
                        and (z >= wall_hi - tol if upper else z <= wall_lo + tol)
                    ):
                        valid.append((node, z))
                if len(valid) != 1:
                    return None
                return valid[0][1], tuple(boundary_set | {valid[0][0]})

            lower, upper = cap(upper=False), cap(upper=True)
            if lower is None or upper is None or upper[0] - lower[0] <= tol:
                continue
            record = PolygonalBoss(
                axis="z",
                center=(round(cx, 4), round(cy, 4), round((lower[0] + upper[0]) / 2, 4)),
                side_count=6,
                across_flats=round(across, 4),
                base=round(lower[0], 4),
                top=round(upper[0], 4),
                flat_directions=tuple(
                    (round(normal[0], 3), round(normal[1], 3), 0.0) for normal in normals
                ),
                flat_centres=tuple(
                    (round(point.X, 3), round(point.Y, 3), round(point.Z, 3)) for point in centres
                ),
            )
            occurrences.append(
                _Expected(
                    record,
                    tuple(fresh.face(node) for node in ordered),
                    tuple(fresh.face(node) for node in set(lower[1]) | set(upper[1])),
                )
            )
    return sorted(occurrences, key=lambda occurrence: occurrence.record)


def _assert_six_side_role(part, occurrence_index, record, candidate, ledger, **kwargs) -> None:
    expected_occurrence = _fresh_expected(part, **kwargs)[occurrence_index]
    assert expected_occurrence.record == record
    expected = frozenset(ledger.graph.require_node(face) for face in expected_occurrence.sides)
    assert len(expected) == 6
    assert ledger.graph.common_valid_solid(expected) is not None
    defining = ledger.defining_of(candidate)
    assert defining == expected
    context = frozenset(ledger.graph.require_node(face) for face in expected_occurrence.context)
    assert context and defining.isdisjoint(context)
    assert all(abs(ledger.graph.normal(node)[2]) < 0.02 for node in defining)
    constituent = ledger.snapshot_index().constituent_of(candidate)
    terminal = constituent - defining
    axis_index = "xyz".index(record.axis)
    assert len(constituent) == 7 and defining < constituent
    assert len(terminal) == 1 and terminal <= context
    assert abs(ledger.graph.normal(next(iter(terminal)))[axis_index]) > 0.99


def _assert_record_matches_original_side_evidence(
    record, candidate, evidence, *, graph=None
) -> None:
    """Check the public 3-D evidence against original faces without reusing discovery."""

    axis_index = "xyz".index(record.axis)
    graph = evidence.graph if graph is None else graph
    defining = evidence.defining_of(candidate)
    assert len(defining) == 6
    assert graph.common_valid_solid(defining) is not None
    expected = {
        (
            tuple(round(value, 3) for value in graph.normal(node)),
            (
                round(float((centre := graph.face(node).center()).X), 3),
                round(float(centre.Y), 3),
                round(float(centre.Z), 3),
            ),
        )
        for node in defining
    }
    published = set(zip(record.flat_directions, record.flat_centres, strict=True))
    assert published == expected
    assert all(abs(direction[axis_index]) == 0 for direction in record.flat_directions)
    assert all(
        centre[axis_index] == pytest.approx(record.center[axis_index], abs=1e-3)
        for centre in record.flat_centres
    )


def test_attached_hexagon_issues_only_its_six_original_side_faces() -> None:
    part = _attached()
    (record,), (candidate,), ledger = _claim(part)
    _assert_six_side_role(part, 0, record, candidate, ledger)
    defining = ledger.defining_of(candidate)
    assert all(abs(ledger.graph.normal(node)[2]) < 0.02 for node in defining)
    assert any(
        ledger.graph.is_planar(node)
        and abs(ledger.graph.normal(node)[2]) > 0.99
        and node not in defining
        for node in ledger.graph.nodes
    )


def test_narrow_stud_annular_terminal_route_excludes_all_cap_context() -> None:
    plate = Box(100, 80, 10)
    prism = extrude(RegularPolygon(20, 6), 30) + Pos(0, 0, 30) * Cylinder(14, 8)
    part = plate + Pos(0, 0, 5) * prism
    records, candidates, ledger = _claim(part)
    assert len(records) == 1 and records[0].top == 35.0
    _assert_six_side_role(part, 0, records[0], candidates[0], ledger)


def test_equal_records_on_coincident_valid_solids_keep_body_identity() -> None:
    original = _attached()
    part = Compound([original, copy.deepcopy(original)])
    records, candidates, ledger = _claim(part)
    assert len(records) == 2 and records[0] == records[1] and records[0] is not records[1]
    first = ledger.defining_of(candidates[0])
    second = ledger.defining_of(candidates[1])
    assert first.isdisjoint(second)
    assert ledger.graph.common_valid_solid(first) != ledger.graph.common_valid_solid(second)
    for index, (record, candidate) in enumerate(zip(records, candidates, strict=True)):
        _assert_six_side_role(part, index, record, candidate, ledger)


def test_multiple_unequal_bosses_keep_sorted_occurrence_to_face_identity() -> None:
    plate = Box(140, 80, 10)
    part = (
        plate
        + Pos(-35, 0, 5) * extrude(RegularPolygon(12, 6), 20)
        + Pos(35, 0, 5) * extrude(RegularPolygon(18, 6), 30)
    )
    records, candidates, ledger = _claim(part)
    assert len(records) == 2 and records[0] != records[1]
    for index, (record, candidate) in enumerate(zip(records, candidates, strict=True)):
        _assert_six_side_role(part, index, record, candidate, ledger)


def test_equal_size_distinct_location_bosses_keep_occurrence_identity() -> None:
    plate = Box(140, 80, 10)
    part = (
        plate
        + Pos(-35, 0, 5) * extrude(RegularPolygon(14, 6), 25)
        + Pos(35, 0, 5) * extrude(RegularPolygon(14, 6), 25)
    )
    records, candidates, ledger = _claim(part)
    assert len(records) == 2
    assert records[0].across_flats == records[1].across_flats
    for index, (record, candidate) in enumerate(zip(records, candidates, strict=True)):
        _assert_six_side_role(part, index, record, candidate, ledger)


def test_all_side_bindings_and_bodies_validate_before_first_publication(monkeypatch) -> None:
    part = Compound([Pos(-80, 0, 0) * _attached(), Pos(80, 0, 0) * _attached()])
    ledger = ClaimLedger(FaceGraph(part))
    original = ledger.graph.common_valid_solid
    calls = 0

    def fail_second(nodes):
        nonlocal calls
        calls += 1
        return None if calls == 2 else original(nodes)

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail_second)
    with pytest.raises(ValueError, match="one valid solid"):
        _discover_polygonal_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_late_side_binding_and_deep_clone_fail_without_prefix(monkeypatch) -> None:
    part = _attached()
    ledger = ClaimLedger(FaceGraph(part))
    original_require = ledger.graph.require_node
    calls = 0

    def fail_late(face):
        nonlocal calls
        calls += 1
        if calls == 6:
            raise ValueError("late side binding failure")
        return original_require(face)

    monkeypatch.setattr(ledger.graph, "require_node", fail_late)
    with pytest.raises(ValueError, match="late side binding"):
        _discover_polygonal_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()

    clone_ledger = ClaimLedger(FaceGraph(part))
    original_recognise = polygonal_module._recognise_one

    def cloned(*args, **kwargs):
        proposals = original_recognise(*args, **kwargs)
        if not proposals:
            return []
        proposal = proposals[0]
        faces = (copy.deepcopy(proposal.side_faces[0]), *proposal.side_faces[1:])
        return [replace(proposal, side_faces=faces)]

    monkeypatch.setattr(polygonal_module, "_recognise_one", cloned)
    with pytest.raises(ValueError):
        _discover_polygonal_bosses(part, writer=clone_ledger.writer)
    assert clone_ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_repeated_side_snapshot_refuses_atomically(monkeypatch) -> None:
    part = Compound([Pos(-80, 0, 0) * _attached(), Pos(80, 0, 0) * _attached()])
    original_recognise = polygonal_module._recognise_one

    def repeated(*args, **kwargs):
        proposals = original_recognise(*args, **kwargs)
        if not proposals:
            return []
        proposal = proposals[0]
        faces = (proposal.side_faces[0], proposal.side_faces[0], *proposal.side_faces[2:])
        return [replace(proposal, side_faces=faces)]

    ledger = ClaimLedger(FaceGraph(part))
    monkeypatch.setattr(polygonal_module, "_recognise_one", repeated)
    with pytest.raises(ValueError, match="six distinct"):
        _discover_polygonal_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


@pytest.mark.parametrize(
    ("terminal", "message"),
    [
        (lambda proposal: None, "requires one retained terminal cap"),
        (lambda proposal: proposal.side_faces[0], "terminal cap identity is unavailable"),
    ],
)
def test_missing_or_side_aliased_terminal_cap_refuses_atomically(
    monkeypatch, terminal, message
) -> None:
    part = _attached()
    original_recognise = polygonal_module._recognise_one

    def corrupted(*args, **kwargs):
        proposals = original_recognise(*args, **kwargs)
        return [replace(proposal, terminal_cap=terminal(proposal)) for proposal in proposals]

    ledger = ClaimLedger(FaceGraph(part))
    monkeypatch.setattr(polygonal_module, "_recognise_one", corrupted)
    with pytest.raises(ValueError, match=message):
        _discover_polygonal_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_shallow_same_topology_wrappers_resolve_to_the_same_six_nodes(monkeypatch) -> None:
    part = _attached()
    original_recognise = polygonal_module._recognise_one

    def wrapped(*args, **kwargs):
        return [
            replace(
                proposal,
                side_faces=tuple(copy.copy(face) for face in proposal.side_faces),
            )
            for proposal in original_recognise(*args, **kwargs)
        ]

    monkeypatch.setattr(polygonal_module, "_recognise_one", wrapped)
    records, candidates, ledger = _claim(part)
    _assert_six_side_role(part, 0, records[0], candidates[0], ledger)


def test_translated_stale_side_snapshot_refuses_without_prefix(monkeypatch) -> None:
    part = _attached()
    original_recognise = polygonal_module._recognise_one

    def stale(*args, **kwargs):
        proposals = original_recognise(*args, **kwargs)
        if not proposals:
            return []
        proposal = proposals[0]
        translated = Pos(1, 0, 0) * copy.deepcopy(proposal.side_faces[0])
        return [replace(proposal, side_faces=(translated, *proposal.side_faces[1:]))]

    ledger = ClaimLedger(FaceGraph(part))
    monkeypatch.setattr(polygonal_module, "_recognise_one", stale)
    with pytest.raises(ValueError):
        _discover_polygonal_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


@pytest.mark.parametrize("publish_first", [False, True])
def test_mixed_or_cross_occurrence_side_snapshots_refuse_atomically(
    monkeypatch, publish_first
) -> None:
    part = Compound([Pos(-80, 0, 0) * _attached(), Pos(80, 0, 0) * _attached()])
    original_recognise = polygonal_module._recognise_one
    first_faces = None

    def corrupted(*args, **kwargs):
        nonlocal first_faces
        proposals = original_recognise(*args, **kwargs)
        if not proposals:
            return []
        if first_faces is None:
            first_faces = proposals[0].side_faces
            return proposals if publish_first else []
        proposal = proposals[0]
        return [replace(proposal, side_faces=(first_faces[0], *proposal.side_faces[1:]))]

    ledger = ClaimLedger(FaceGraph(part))
    monkeypatch.setattr(polygonal_module, "_recognise_one", corrupted)
    message = "share defining" if publish_first else "one valid solid"
    with pytest.raises(ValueError, match=message):
        _discover_polygonal_bosses(part, writer=ledger.writer)
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_registry_terminal_lifecycle_retains_nonempty_polygonal_boss() -> None:
    product = _take_inventory(_attached())
    candidates = product.physical.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates
    assert len(candidates) == len(product.result.polygonal_bosses) == 1
    assert candidates[0].record is product.result.polygonal_bosses[0]
    assert len(product.evidence.defining_of(candidates[0])) == 6
    assert len(product.evidence.constituent_of(candidates[0])) == 7


def test_six_explicit_blend_bridges_recover_the_sharp_polygonal_boss() -> None:
    sharp = _attached()
    rounded = _blend_interrupted_attached()
    expected = recognise_polygonal_bosses(sharp)
    records, candidates, ledger = _claim(rounded)
    assert [record.to_dict() for record in records] == [record.to_dict() for record in expected]
    assert len(records) == len(candidates) == 1
    defining = ledger.defining_of(candidates[0])
    assert len(defining) == 6
    assert all(ledger.graph.is_planar(node) for node in defining)
    assert all(abs(ledger.graph.normal(node)[2]) <= 0.02 for node in defining)


def test_blend_interrupted_boss_aggregate_keeps_fillets_and_evidence_disjoint() -> None:
    part = _blend_interrupted_attached()
    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates
    assert product.result.polygonal_bosses == tuple(recognise_polygonal_bosses(part))
    assert len(product.result.polygonal_bosses) == len(candidates) == 1
    assert product.result.fillets == tuple(recognise_fillets(part)) == ()
    assert len(product.evidence.defining_of(candidates[0])) == 6


def test_partial_blend_cycle_does_not_select_a_subset() -> None:
    assert recognise_polygonal_bosses(_partially_blend_interrupted_attached()) == []


def test_blend_interrupted_boss_survives_step_roundtrip(tmp_path) -> None:
    path = tmp_path / "blend-interrupted-polygonal-boss.step"
    export_step(_blend_interrupted_attached(), path)
    imported = import_step(path)
    assert [record.to_dict() for record in recognise_polygonal_bosses(imported)] == [
        record.to_dict() for record in recognise_polygonal_bosses(_attached())
    ]


def test_independent_blend_oracle_proves_exact_cycle_and_expansion() -> None:
    graph = FaceGraph(_blend_interrupted_attached())
    index = BlendCollapseIndex(graph, EffectiveSurfaceIndex(graph))
    chains = index.chains()
    assert len(chains) == 6
    assert all(chain.side == "convex" and len(chain.blend_nodes) == 1 for chain in chains)
    assert all(tuple(map(len, chain.supports)) == (1, 1) for chain in chains)
    supports = frozenset(node for chain in chains for region in chain.supports for node in region)
    assert len(supports) == 6
    assert all(
        sum(node in region for chain in chains for region in chain.supports) == 2
        for node in supports
    )

    view = index.view(chains)
    logical = {
        next(iter(source)): node
        for node in view.logical_nodes()
        if len(source := view.expand_node(node)) == 1
    }
    for chain in chains:
        left = next(iter(chain.supports[0]))
        right = next(iter(chain.supports[1]))
        (bridge,) = tuple(
            arc for arc in view.arcs_between(logical[left], logical[right]) if arc.synthetic
        )
        expanded = view.expand_arc(bridge)
        assert expanded.nodes == frozenset(
            (*chain.blend_nodes, *chain.supports[0], *chain.supports[1])
        )
        assert Counter(arc.occurrence for arc in expanded.arcs) == Counter(
            arc.occurrence
            for arc in (*chain.spring_arcs, *chain.internal_arcs, *chain.terminal_arcs)
        )


def test_six_cycle_reducer_refuses_partial_chord_and_duplicate_components() -> None:
    nodes = tuple(FaceNode(at) for at in range(12))

    def cycle(offset: int) -> tuple[frozenset, ...]:
        return tuple(
            frozenset((nodes[offset + at], nodes[offset + ((at + 1) % 6)])) for at in range(6)
        )

    first = cycle(0)
    second = cycle(6)
    assert polygonal_module._six_support_cycle_indices(first[:5]) == ()
    assert polygonal_module._six_support_cycle_indices((*first, first[0])) == ()
    assert (
        polygonal_module._six_support_cycle_indices((*first, frozenset((nodes[0], nodes[3])))) == ()
    )
    assert polygonal_module._six_support_cycle_indices((*first, *second)) == tuple(range(12))


def test_corrupted_expansion_refuses_before_candidate_publication(monkeypatch) -> None:
    part = _blend_interrupted_attached()
    ledger = ClaimLedger(FaceGraph(part))
    original = CollapsedGraphView.expand_arc

    def corrupted(self, arc):
        expanded = original(self, arc)
        return FrozenProvenance(frozenset(), expanded.arcs)

    monkeypatch.setattr(CollapsedGraphView, "expand_arc", corrupted)
    with pytest.raises(ValueError, match="lost original provenance"):
        _discover_polygonal_bosses(
            part, graph=GeometryGraph._from_graph(ledger.graph), writer=ledger.writer
        )
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


@pytest.mark.parametrize("mutation", ["duplicate", "replace"])
def test_corrupted_expansion_occurrence_multiset_refuses_atomically(monkeypatch, mutation) -> None:
    part = _blend_interrupted_attached()
    original = CollapsedGraphView.expand_arc

    def corrupted(self, arc):
        expanded = original(self, arc)
        arcs = expanded.arcs
        changed = (*arcs, arcs[0]) if mutation == "duplicate" else (arcs[0], arcs[0], *arcs[2:])
        return FrozenProvenance(expanded.nodes, changed)

    monkeypatch.setattr(CollapsedGraphView, "expand_arc", corrupted)
    with pytest.raises(ValueError, match="lost original provenance"):
        recognise_polygonal_bosses(part)
    ledger = ClaimLedger(FaceGraph(part))
    with pytest.raises(ValueError, match="lost original provenance"):
        _discover_polygonal_bosses(
            part, graph=GeometryGraph._from_graph(ledger.graph), writer=ledger.writer
        )
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


@pytest.mark.parametrize("mutation", ["concave", "multi_node", "multi_support"])
def test_consumer_refuses_ineligible_chain_without_candidate_prefix(monkeypatch, mutation) -> None:
    part = _blend_interrupted_attached()
    original = BlendCollapseIndex.chains

    def changed(self):
        chains = original(self)
        first = chains[0]
        if mutation == "concave":
            replacement = replace(first, side="concave")
        elif mutation == "multi_node":
            extra = next(node for node in self._graph.nodes if node not in first.blend_nodes)
            replacement = replace(first, blend_nodes=frozenset((*first.blend_nodes, extra)))
        else:
            extra = next(node for node in self._graph.nodes if node not in first.supports[0])
            replacement = replace(
                first,
                supports=(frozenset((*first.supports[0], extra)), first.supports[1]),
            )
        return (replacement, *chains[1:])

    monkeypatch.setattr(BlendCollapseIndex, "chains", changed)
    assert recognise_polygonal_bosses(part) == []
    ledger = ClaimLedger(FaceGraph(part))
    assert (
        _discover_polygonal_bosses(
            part, graph=GeometryGraph._from_graph(ledger.graph), writer=ledger.writer
        )
        == []
    )
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_non_cylinder_blend_fact_refuses_full_consumer_path(monkeypatch) -> None:
    part = _blend_interrupted_attached()
    graph = FaceGraph(part)
    oracle = BlendCollapseIndex(graph, EffectiveSurfaceIndex(graph))
    target = next(iter(oracle.chains()[0].blend_nodes))
    original = EffectiveSurfaceIndex.fact
    hit_queries: list[EffectiveSurfaceIndex] = []

    def changed(self, node):
        fact = original(self, node)
        if node is target and isinstance(fact, AnalyticSurfaceFact):
            if all(self is not query for query in hit_queries):
                hit_queries.append(self)
            return replace(fact, kind=polygonal_module.SurfaceKind.PLANE)
        return fact

    monkeypatch.setattr(EffectiveSurfaceIndex, "fact", changed)
    assert recognise_polygonal_bosses(part, graph=GeometryGraph._from_graph(graph)) == []
    ledger = ClaimLedger(graph)
    assert (
        _discover_polygonal_bosses(
            part, graph=GeometryGraph._from_graph(graph), writer=ledger.writer
        )
        == []
    )
    assert len(hit_queries) == 2
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_unequal_support_span_refuses_after_chain_issuance(monkeypatch) -> None:
    part = _blend_interrupted_attached()
    graph = FaceGraph(part)
    oracle = BlendCollapseIndex(graph, EffectiveSurfaceIndex(graph))
    target = next(iter(oracle.chains()[0].supports[0]))
    original_chains = BlendCollapseIndex.chains
    original_bounds = FaceGraph.bounds
    active = False
    generation = 0
    hit_generations: set[int] = set()

    def chains(self):
        nonlocal active, generation
        result = original_chains(self)
        generation += 1
        active = True
        return result

    def bounds(self, node):
        result = original_bounds(self, node)
        if active and node is target:
            hit_generations.add(generation)
            x, y, (lo, hi) = result
            return x, y, (lo, hi + 1.0)
        return result

    monkeypatch.setattr(BlendCollapseIndex, "chains", chains)
    monkeypatch.setattr(FaceGraph, "bounds", bounds)
    assert recognise_polygonal_bosses(part, graph=GeometryGraph._from_graph(graph)) == []
    active = False
    ledger = ClaimLedger(graph)
    assert (
        _discover_polygonal_bosses(
            part, graph=GeometryGraph._from_graph(graph), writer=ledger.writer
        )
        == []
    )
    assert hit_generations == {1, 2}
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_two_disjoint_blend_cycles_and_reversed_presentation_are_order_neutral() -> None:
    left = Pos(-120, 0, 0) * _blend_interrupted_attached()
    right = Pos(120, 0, 0) * _blend_interrupted_attached()
    part = Compound([left, right])
    expected = recognise_polygonal_bosses(part)
    assert len(expected) == 2
    reversed_records = recognise_polygonal_bosses(_ReversedFacesPart(part))
    assert [record.to_dict() for record in reversed_records] == [
        record.to_dict() for record in expected
    ]


@pytest.mark.parametrize(
    "transform",
    [
        Pos(17, -11, 0),
        Rot(0, 0, 37),
        Rot(180, 0, 0),
        Rot(90, 0, 0),
        Rot(-90, 0, 0),
        Rot(0, 90, 0),
        Rot(0, -90, 0),
    ],
)
def test_blend_cycle_rigid_transforms_match_their_sharp_controls(transform) -> None:
    rounded = transform * _blend_interrupted_attached()
    sharp = transform * _attached()
    assert [record.to_dict() for record in recognise_polygonal_bosses(rounded)] == [
        record.to_dict() for record in recognise_polygonal_bosses(sharp)
    ]


def test_uniformly_scaled_blend_cycle_matches_its_sharp_control() -> None:
    rounded = _blend_interrupted_attached().scale(3)
    sharp = _attached(scale=3)
    assert [record.to_dict() for record in recognise_polygonal_bosses(rounded, tol=0.6)] == [
        record.to_dict() for record in recognise_polygonal_bosses(sharp, tol=0.6)
    ]


def test_real_fillet_and_recovered_boss_coexist_with_disjoint_evidence() -> None:
    part = Compound(
        [
            Pos(-120, 0, 0) * _blend_interrupted_attached(),
            Pos(120, 0, 0) * _independent_prismatic_fillet(),
        ]
    )
    product = _take_inventory(part)
    bosses = product.physical.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates
    fillets = product.physical.candidate_set(FamilyId.FILLETS).candidates
    assert len(bosses) == 1
    assert fillets
    boss_nodes = product.evidence.defining_of(bosses[0])
    fillet_nodes = frozenset(
        node for candidate in fillets for node in product.evidence.defining_of(candidate)
    )
    assert len(boss_nodes) == 6
    assert boss_nodes.isdisjoint(fillet_nodes)
    assert product.result.polygonal_bosses == tuple(recognise_polygonal_bosses(part))
    assert product.result.fillets == tuple(recognise_fillets(part))


@pytest.mark.parametrize(
    "transform",
    [Pos(17, -11, 3), Rot(0, 0, 37), Rot(0, 0, 180)],
)
def test_supported_rigid_transforms_keep_exact_side_roles(transform) -> None:
    part = transform * _attached()
    records, candidates, ledger = _claim(part)
    assert len(records) == 1
    _assert_six_side_role(part, 0, records[0], candidates[0], ledger)


@pytest.mark.parametrize(
    ("rotation", "axis"),
    [
        (Rot(0, 0, 0), "z"),
        (Rot(180, 0, 0), "z"),
        (Rot(90, 0, 0), "y"),
        (Rot(-90, 0, 0), "y"),
        (Rot(0, 90, 0), "x"),
        (Rot(0, -90, 0), "x"),
    ],
)
def test_principal_axis_bosses_preserve_physical_parameters(rotation, axis) -> None:
    (baseline,) = recognise_polygonal_bosses(_attached())

    records, candidates, ledger = _claim(rotation * _attached())
    (record,) = records

    assert record.axis == axis
    assert record.side_count == 6
    assert record.across_flats == baseline.across_flats
    assert record.height == baseline.height
    assert all(abs(direction["xyz".index(axis)]) == 0 for direction in record.flat_directions)
    _assert_record_matches_original_side_evidence(record, candidates[0], ledger)


def test_equal_body_local_bosses_on_distinct_principal_axes_remain_distinct() -> None:
    part = Compound(
        [
            Pos(-120, 0, 0) * _attached(),
            Pos(120, 0, 0) * Rot(90, 0, 0) * _attached(),
        ]
    )

    records, candidates, ledger = _claim(part)

    assert len(records) == len(candidates) == 2
    assert {record.axis for record in records} == {"y", "z"}
    assert len({record.center for record in records}) == 2
    assert all(
        record.across_flats == pytest.approx(20 * math.sqrt(3), abs=1e-3) for record in records
    )
    for record, candidate in zip(records, candidates, strict=True):
        _assert_record_matches_original_side_evidence(record, candidate, ledger)


def test_principal_axis_aggregate_reconciles_one_physical_boss_once() -> None:
    part = Rot(0, -90, 0) * _attached()

    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates

    assert len(product.result.polygonal_bosses) == len(candidates) == 1
    assert candidates[0].record is product.result.polygonal_bosses[0]
    _assert_record_matches_original_side_evidence(
        product.result.polygonal_bosses[0],
        candidates[0],
        product.evidence,
        graph=product.context.graph,
    )


def test_polygonal_boss_survives_arbitrary_rigid_motion_through_framed_aggregate() -> None:
    moved = Pos(17, -23, 9) * Rot(31, 47, 13) * _attached()

    framed = build_framed_recognition_result(moved, rotational=False)

    assert isinstance(framed, FramedRecognitionResult)
    (record,) = framed.result.polygonal_bosses
    assert record.axis in "xyz"
    assert record.side_count == 6
    assert record.across_flats == pytest.approx(20 * math.sqrt(3), abs=1e-3)
    assert record.height == 30
    direct, candidates, ledger = _claim(framed.part)
    assert direct == [record]
    _assert_record_matches_original_side_evidence(record, candidates[0], ledger)


@pytest.mark.parametrize("plane", [Plane.YZ, Plane.XZ])
def test_principal_xy_mirrors_keep_exact_side_roles(plane) -> None:
    part = _attached().mirror(plane)
    records, candidates, ledger = _claim(part)
    assert len(records) == 1
    _assert_six_side_role(part, 0, records[0], candidates[0], ledger)


@pytest.mark.parametrize("scale", [0.2, 5.0])
def test_uniform_scales_and_custom_tolerances_keep_lifecycle(scale) -> None:
    part = _attached(scale=scale)
    records, candidates, ledger = _claim(part, tol=0.04 * scale)
    assert len(records) == 1
    _assert_six_side_role(part, 0, records[0], candidates[0], ledger)


@pytest.mark.parametrize("height", [0.19, 0.2])
def test_minimum_height_at_or_below_tolerance_never_publishes(height) -> None:
    part = Box(20, 20, 2) + Pos(0, 0, 1) * extrude(RegularPolygon(3, 6), height)
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_polygonal_bosses(part, tol=0.2) == []
    assert _discover_polygonal_bosses(part, tol=0.2, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_minimum_height_just_above_tolerance_has_full_lifecycle() -> None:
    part = Box(20, 20, 2) + Pos(0, 0, 1) * extrude(RegularPolygon(3, 6), 0.21)
    records, candidates, ledger = _claim(part, tol=0.2)
    assert len(records) == 1
    _assert_six_side_role(part, 0, records[0], candidates[0], ledger, tol=0.2)


@pytest.mark.parametrize("rotation", [Rot(0, 0, 0), Rot(90, 0, 0), Rot(0, -90, 0)])
def test_step_round_trip_preserves_record_and_side_role_geometry(tmp_path: Path, rotation) -> None:
    source = rotation * _attached()
    path = tmp_path / "polygonal-boss.step"
    export_step(source, path)
    imported = import_step(path)

    source_records, source_candidates, source_ledger = _claim(source)
    imported_records, imported_candidates, imported_ledger = _claim(imported)
    assert [item.to_dict() for item in source_records] == [
        item.to_dict() for item in imported_records
    ]
    _assert_record_matches_original_side_evidence(
        source_records[0], source_candidates[0], source_ledger
    )
    _assert_record_matches_original_side_evidence(
        imported_records[0], imported_candidates[0], imported_ledger
    )


def test_reversed_face_traversal_preserves_record_to_side_occurrence(monkeypatch) -> None:
    part = _attached()
    baseline, _, _ = _claim(part)
    original_faces = type(part).faces

    def reversed_faces(self):
        faces = list(original_faces(self))
        return list(reversed(faces)) if self is part else faces

    monkeypatch.setattr(type(part), "faces", reversed_faces)
    records, candidates, ledger = _claim(part)
    assert [record.to_dict() for record in records] == [record.to_dict() for record in baseline]
    _assert_six_side_role(part, 0, records[0], candidates[0], ledger)


def test_wrong_discovery_graph_and_foreign_writer_refuse_without_prefix() -> None:
    part = _attached()
    foreign = ClaimLedger(FaceGraph(Box(20, 20, 20)))
    with pytest.raises(ValueError):
        _discover_polygonal_bosses(
            part, graph=GeometryGraph._from_graph(foreign.graph), writer=foreign.writer
        )
    assert foreign.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()

    local = FaceGraph(part)
    other = ClaimLedger(FaceGraph(part))
    with pytest.raises(ValueError, match="different runs"):
        _discover_polygonal_bosses(
            part, graph=GeometryGraph._from_graph(local), writer=other.writer
        )
    assert other.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()

    superset = Compound([part, Pos(150, 0, 0) * _attached()])
    superset_ledger = ClaimLedger(FaceGraph(superset))
    with pytest.raises(ValueError, match="exactly match"):
        _discover_polygonal_bosses(
            part,
            graph=GeometryGraph._from_graph(superset_ledger.graph),
            writer=superset_ledger.writer,
        )
    assert superset_ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_supplied_single_graph_and_multi_solid_local_graph_routes() -> None:
    single = _attached()
    graph = FaceGraph(single)
    geometry = GeometryGraph._from_graph(graph)
    ledger = ClaimLedger(graph)
    public = recognise_polygonal_bosses(single, graph=geometry)
    records = _discover_polygonal_bosses(single, graph=geometry, writer=ledger.writer)
    assert [record.to_dict() for record in records] == [record.to_dict() for record in public]
    _assert_six_side_role(
        single,
        0,
        records[0],
        ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates[0],
        ledger,
    )

    multi = Compound([Pos(-80, 0, 0) * single, Pos(80, 0, 0) * single])
    whole = ClaimLedger(FaceGraph(multi))
    records = _discover_polygonal_bosses(
        multi,
        graph=FaceGraph(Box(3, 3, 3)),
        writer=whole.writer,
    )
    assert len(records) == 2
    for index, candidate in enumerate(whole.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates):
        _assert_six_side_role(multi, index, records[index], candidate, whole)


@pytest.mark.parametrize(
    "part",
    [
        extrude(RegularPolygon(20, 6), 30),
        Box(80, 80, 20) - Pos(0, 0, 10) * extrude(RegularPolygon(20, 6), 20),
        Box(30, 30, 30),
        Box(100, 80, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(20, 4), 30),
        Box(100, 80, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(20, 8), 30),
        Box(100, 80, 10)
        + Pos(0, 0, 5) * (extrude(RegularPolygon(20, 6), 30) + Pos(0, 0, 32) * Cylinder(25, 4)),
        Box(100, 80, 10) + Pos(0, 0, 25) * extrude(RegularPolygon(20, 6), 30),
        Box(100, 80, 10) + Pos(0, 0, 5) * _irregular_hexagon(),
        Box(100, 80, 10)
        + Pos(0, 0, 5) * (extrude(RegularPolygon(20, 6), 30) - Pos(24, 0, 15) * Box(20, 60, 60)),
        Box(100, 80, 10)
        + Pos(0, 0, 5)
        * (extrude(RegularPolygon(20, 6), 30) - Pos(0, 0, 32) * (Rot(25, 0, 0) * Box(60, 60, 20))),
    ],
)
def test_excluded_stock_recess_and_nonhex_shapes_never_publish(part) -> None:
    ledger = ClaimLedger(FaceGraph(part))
    assert recognise_polygonal_bosses(part) == []
    assert _discover_polygonal_bosses(part, writer=ledger.writer) == []
    assert ledger.candidate_set(FamilyId.POLYGONAL_BOSSES).candidates == ()


def test_private_core_has_one_production_writer_caller_and_one_boss_constructor() -> None:
    core_sites: list[tuple[str, ast.Call]] = []
    constructors: list[tuple[str, ast.Call]] = []
    for path in (ROOT / "src/quiddity").glob("*.py"):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for qualified, call in _qualified_calls(tree):
            if (
                qualified.endswith("._discover_polygonal_bosses")
                or qualified == "_discover_polygonal_bosses"
            ):
                core_sites.append((path.name, call))
            if (
                qualified.endswith(".PolygonalBoss")
                or qualified == "PolygonalBoss"
                or (path.name == "polygonal_bosses.py" and qualified == "record_type")
            ):
                constructors.append((path.name, call))

    assert {path for path, _call in core_sites} == {
        "polygonal_bosses.py",
        "_registry.py",
    }
    registry_call = next(call for path, call in core_sites if path == "_registry.py")
    keywords = {keyword.arg: keyword.value for keyword in registry_call.keywords}
    writer = keywords["writer"]
    assert isinstance(writer, ast.Attribute) and writer.attr == "writer"
    assert isinstance(writer.value, ast.Name) and writer.value.id == "s"
    graph = keywords["graph"]
    assert isinstance(graph, ast.Attribute) and graph.attr == "geometry"
    assert isinstance(graph.value, ast.Attribute) and graph.value.attr == "context"
    assert isinstance(graph.value.value, ast.Name) and graph.value.value.id == "s"
    assert [(path, len(call.args)) for path, call in constructors] == [("polygonal_bosses.py", 0)]
