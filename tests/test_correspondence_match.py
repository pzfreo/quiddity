from __future__ import annotations

import ast
import math
from dataclasses import dataclass, replace
from itertools import combinations

import pytest
from build123d import (
    Align,
    Box,
    Compound,
    Cylinder,
    Plane,
    Polygon,
    Pos,
    Rot,
    export_step,
    extrude,
    import_step,
    loft,
)
from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps

import quiddity._correspondence_match as correspondence_match_module
import quiddity._correspondence_partition as partition_module
from quiddity._body_geometry import (
    ANGLE_TOL,
    DESCRIPTOR_FLOOR,
    DESCRIPTOR_REL,
    DIRECTION_TOL,
)
from quiddity._correspondence import correspondence_snapshot
from quiddity._correspondence_match import (
    IDENTITY_ROTATION,
    PROPER_ROTATIONS,
    ChangeKind,
    CorrespondenceMatchError,
    CorrespondenceRelation,
    CorrespondenceResult,
    RigidScaleWitness,
    _affine_point,
    _body_similarity,
    _canonicalize_partition_witnesses,
    _compare_snapshots,
    _curve_similarity,
    _determinant,
    _direction_close,
    _face_similarity,
    _inverse_witness,
    _MatchBudget,
    _maximum_matchings,
    _normalize_partition_translation,
    _order_bound,
    _partition_witnesses,
    _prism_cap_similarity,
    _rotate,
    _scale_is_identity,
    _validate_result,
    _wire_alignments,
    correspondence_changes,
)
from quiddity._correspondence_partition import prism_fact
from quiddity.result import _take_inventory
from tests.test_correspondence_snapshot import (
    _line_rrp,
    _proper_signed_permutations,
    _proper_transform,
    _raw_planar_cycle_oracle,
    _rrp,
    _two_rrp_one_solid,
)


def _asymmetric_rrp():
    return _line_rrp(5) + Pos(18, 0, 5) * Box(
        4, 3, 2, align=(Align.CENTER, Align.CENTER, Align.CENTER)
    )


def _chiral_rrp():
    return (
        _line_rrp(5)
        + Pos(18, 0, 3) * Box(4, 3, 2, align=(Align.CENTER, Align.CENTER, Align.CENTER))
        + Pos(0, 18, 7) * Box(2, 5, 2, align=(Align.CENTER, Align.CENTER, Align.CENTER))
    )


def _partition_rrp(height: float, start: float = 0.0, *, phase: float = 13.0, repeats: int = 5):
    points = []
    for sector in range(repeats):
        for offset, radius in enumerate((20.0, 16.0, 20.0, 18.0)):
            angle = 2.0 * math.pi * (sector / repeats + offset / (4 * repeats))
            points.append((radius * math.cos(angle), radius * math.sin(angle)))
    return Pos(0, 0, start) * Rot(0, 0, phase) * extrude(Polygon(*points), height)


def _mixed_partition_rrp(
    height: float, start: float = 0.0, *, repeats: int = 7, phase: float = 13.0
):
    part = Cylinder(20, height)
    for index in range(repeats):
        part -= Rot(0, 0, phase + 360 * index / repeats) * Pos(18, 0, 0) * Box(8, 3, height)
    return Pos(0, 0, start + height / 2.0) * part


def _prism_fact_for(occurrence, *, graph=None, summary=None):
    summary = occurrence.summary if summary is None else summary
    return prism_fact(
        occurrence.matching_boundary if graph is None else graph,
        axis_name=summary.axis,
        span=summary.span,
        profile_centre=summary.centre,
        section_signature=summary.sector_signature,
        defining=summary.defining,
        repeat_count=summary.repeat_count,
        edge_count=summary.edge_count,
        volume=occurrence.body.intrinsic.volume,
        centre_of_mass=occurrence.body.placement.centre_of_mass,
        quantization=occurrence.body.quantization,
    )


@dataclass(frozen=True)
class _RawPrismOracleFact:
    lo: float
    hi: float
    axis: tuple[float, float, float]
    low_cycle: tuple[tuple, ...]
    high_cycle: tuple[tuple, ...]
    low_material_side: int
    high_material_side: int
    profile_centre: tuple[float, float, float]
    section_samples: tuple[tuple[tuple[float, float, float], ...], ...]
    side_kinds: tuple[str, ...]
    axial_pairs: tuple[tuple[int, int], ...]
    volume: float
    centre_of_mass: tuple[float, float, float]
    metric_quantum: float
    volume_quantum: float


def _raw_cycle_presentations(cycle):
    congruence = tuple(
        (value[0], value[1], value[3], abs(value[4])) if value[0] == "CIRCLE" else value[:2]
        for value in cycle
    )
    reverse = tuple(reversed(congruence))
    return {congruence[offset:] + congruence[:offset] for offset in range(len(congruence))} | {
        reverse[offset:] + reverse[:offset] for offset in range(len(reverse))
    }


def _raw_prism_partition_oracle(part):
    """Derive complete raw prism topology before any production snapshot is read."""

    solids = tuple(part.solids())
    facts = []
    for solid in solids:
        planar_cycles, raw_edges = _raw_planar_cycle_oracle(solid)
        faces = tuple(solid.faces())

        owners = {
            edge_at: tuple(
                face_at
                for face_at, face in enumerate(faces)
                if any(candidate.wrapped.IsSame(edge.wrapped) for candidate in face.edges())
            )
            for edge_at, edge in enumerate(raw_edges)
        }
        assert all(len(face_owners) == 2 for face_owners in owners.values())

        candidates = []
        planar_faces = tuple(
            face_at
            for face_at, face in enumerate(faces)
            if BRepAdaptor_Surface(face.wrapped).GetType().name == "GeomAbs_Plane"
            and (face_at, "outer") in planar_cycles
        )
        for left_at in planar_faces:
            for right_at in planar_faces:
                if right_at <= left_at:
                    continue
                left_face, right_face = faces[left_at], faces[right_at]
                left_normal = tuple(
                    float(value) for value in left_face.normal_at(left_face.center())
                )
                right_normal = tuple(
                    float(value) for value in right_face.normal_at(right_face.center())
                )
                if sum(a * b for a, b in zip(left_normal, right_normal, strict=True)) > -(
                    1.0 - 4.0 * DIRECTION_TOL
                ):
                    continue
                first_axis = next(value for value in left_normal if abs(value) > 1e-12)
                axis = tuple((1.0 if first_axis > 0.0 else -1.0) * value for value in left_normal)
                left_position = sum(
                    value * direction
                    for value, direction in zip(left_face.center(), axis, strict=True)
                )
                right_position = sum(
                    value * direction
                    for value, direction in zip(right_face.center(), axis, strict=True)
                )
                if right_position < left_position:
                    left_at, right_at = right_at, left_at
                    left_face, right_face = right_face, left_face
                    left_normal, right_normal = right_normal, left_normal
                    left_position, right_position = right_position, left_position
                low_cycle = planar_cycles[(left_at, "outer")]
                high_cycle = planar_cycles[(right_at, "outer")]
                low_edges = tuple(edge_at for edge_at, _direction in low_cycle)
                high_edges = tuple(edge_at for edge_at, _direction in high_cycle)
                low_sides = tuple(
                    next(face for face in owners[edge] if face != left_at) for edge in low_edges
                )
                high_sides = tuple(
                    next(face for face in owners[edge] if face != right_at) for edge in high_edges
                )
                if len(set(low_sides)) != len(low_sides) or set(low_sides) != set(high_sides):
                    continue
                side_set = set(low_sides)
                if side_set != set(range(len(faces))) - {left_at, right_at}:
                    continue
                if any(
                    sum(face in side_set for face in owners[edge]) != 2
                    for edge in owners
                    if edge not in set(low_edges) | set(high_edges)
                ):
                    continue
                side_pairs = tuple(
                    (
                        low_sides[index],
                        low_sides[(index + 1) % len(low_sides)],
                    )
                    for index in range(len(low_sides))
                )
                axial_pairs = {
                    tuple(sorted(owners[edge]))
                    for edge in owners
                    if edge not in set(low_edges) | set(high_edges)
                }
                if axial_pairs != {tuple(sorted(pair)) for pair in side_pairs}:
                    continue

                def label(edge_at: int, direction: int, raw_edges=raw_edges):
                    adaptor = BRepAdaptor_Curve(raw_edges[edge_at].wrapped)
                    kind = adaptor.GetType().name.removeprefix("GeomAbs_").upper()
                    base = (kind, round(float(raw_edges[edge_at].length), 9), direction)
                    if kind != "CIRCLE":
                        return base
                    circle = adaptor.Circle()
                    return (
                        *base,
                        round(float(circle.Radius()), 9),
                        round(float(adaptor.LastParameter() - adaptor.FirstParameter()), 12),
                    )

                low_labels = tuple(label(*item) for item in low_cycle)
                high_labels = tuple(label(*item) for item in high_cycle)

                # Both cycles are independently material-oriented; opposite cap normals reverse
                # the presentation while preserving the complete analytic curve roster.
                if not _raw_cycle_presentations(low_labels) & _raw_cycle_presentations(high_labels):
                    continue
                side_kinds = tuple(
                    BRepAdaptor_Surface(faces[face_at].wrapped)
                    .GetType()
                    .name.removeprefix("GeomAbs_")
                    .upper()
                    for face_at in low_sides
                )
                if any(
                    (label_value[0] == "LINE" and side_kind != "PLANE")
                    or (label_value[0] == "CIRCLE" and side_kind != "CYLINDER")
                    for label_value, side_kind in zip(low_labels, side_kinds, strict=True)
                ):
                    continue
                properties = GProp_GProps()
                BRepGProp.VolumeProperties_s(solid.wrapped, properties)
                surface_properties = GProp_GProps()
                BRepGProp.SurfaceProperties_s(solid.wrapped, surface_properties)
                centre_of_mass = tuple(float(value) for value in properties.CentreOfMass().Coord())
                raw_volume = float(properties.Mass())
                raw_area = float(surface_properties.Mass())
                characteristic_scale = max(raw_volume ** (1.0 / 3.0), math.sqrt(raw_area))
                metric_quantum = DESCRIPTOR_REL * characteristic_scale + DESCRIPTOR_FLOOR
                volume_quantum = (
                    characteristic_scale + metric_quantum
                ) ** 3 - characteristic_scale**3
                low_centre = tuple(float(value) for value in left_face.center())
                high_centre = tuple(float(value) for value in right_face.center())
                profile_centre = tuple(
                    (low + high) / 2.0 for low, high in zip(low_centre, high_centre, strict=True)
                )

                def section_samples(
                    cycle,
                    raw_edges=raw_edges,
                    profile_centre=profile_centre,
                    axis=axis,
                ):
                    values = []
                    for edge_at, direction in cycle:
                        adaptor = BRepAdaptor_Curve(raw_edges[edge_at].wrapped)
                        first_parameter, last_parameter = (
                            (adaptor.FirstParameter(), adaptor.LastParameter())
                            if direction == 1
                            else (adaptor.LastParameter(), adaptor.FirstParameter())
                        )
                        samples = []
                        for sample_at in range(9):
                            parameter = first_parameter + (last_parameter - first_parameter) * (
                                sample_at / 8.0
                            )
                            point = adaptor.Value(parameter)
                            relative = tuple(
                                float(value) - origin
                                for value, origin in zip(point.Coord(), profile_centre, strict=True)
                            )
                            axial = sum(
                                value * axis_value
                                for value, axis_value in zip(relative, axis, strict=True)
                            )
                            samples.append(
                                tuple(
                                    value - axial * axis_value
                                    for value, axis_value in zip(relative, axis, strict=True)
                                )
                            )
                        values.append(tuple(samples))
                    return tuple(values)

                low_material = (
                    1
                    if sum(
                        value * direction
                        for value, direction in zip(left_normal, axis, strict=True)
                    )
                    > 0.0
                    else -1
                )
                high_material = (
                    1
                    if sum(
                        value * direction
                        for value, direction in zip(right_normal, axis, strict=True)
                    )
                    > 0.0
                    else -1
                )
                assert low_material == -high_material

                for edge_at in set(owners) - set(low_edges) - set(high_edges):
                    adaptor = BRepAdaptor_Curve(raw_edges[edge_at].wrapped)
                    if adaptor.GetType().name != "GeomAbs_Line":
                        break
                    first = tuple(
                        float(value) for value in adaptor.Value(adaptor.FirstParameter()).Coord()
                    )
                    last = tuple(
                        float(value) for value in adaptor.Value(adaptor.LastParameter()).Coord()
                    )
                    delta = tuple(right - left for left, right in zip(first, last, strict=True))
                    length = math.sqrt(sum(value * value for value in delta))
                    if (
                        length <= 0.0
                        or 1.0
                        - abs(
                            sum(
                                value * direction
                                for value, direction in zip(delta, axis, strict=True)
                            )
                            / length
                        )
                        > 4.0 * DIRECTION_TOL
                    ):
                        break
                    endpoints = sorted(
                        sum(value * direction for value, direction in zip(point, axis, strict=True))
                        for point in (first, last)
                    )
                    if endpoints != pytest.approx((left_position, right_position), abs=1e-7):
                        break
                else:
                    candidates.append(
                        _RawPrismOracleFact(
                            left_position,
                            right_position,
                            tuple(round(value, 12) for value in axis),
                            low_labels,
                            high_labels,
                            low_material,
                            high_material,
                            profile_centre,
                            section_samples(low_cycle),
                            side_kinds,
                            tuple(sorted(axial_pairs)),
                            raw_volume,
                            centre_of_mass,
                            metric_quantum,
                            volume_quantum,
                        )
                    )
        assert len(candidates) == 1
        facts.append(candidates[0])
    return tuple(facts)


def _raw_partition_relation_oracle(parent, children):
    """Prove the raw geometric partition and common witness without matcher values."""

    assert len(parent) == 1 and children
    source = parent[0]
    ordered = tuple(sorted(children, key=lambda fact: (fact.lo, fact.hi)))
    assert len({(fact.lo, fact.hi) for fact in ordered}) == len(ordered)
    assert all(
        abs(left.hi - right.lo) <= 2.0 * (left.metric_quantum + right.metric_quantum)
        for left, right in zip(ordered, ordered[1:], strict=False)
    )
    scale = (ordered[-1].hi - ordered[0].lo) / (source.hi - source.lo)

    def rotate(rotation, point):
        return tuple(
            sum(rotation[row][column] * point[column] for column in range(3)) for row in range(3)
        )

    def point_cycle_matches(rotation, child):
        metric_bound = 2.0 * (scale * source.metric_quantum + child.metric_quantum)
        transformed = tuple(
            (
                tuple(
                    tuple(scale * value for value in rotate(rotation, point)) for point in samples
                ),
                side_kind,
            )
            for samples, side_kind in zip(source.section_samples, source.side_kinds, strict=True)
        )
        target = tuple(zip(child.section_samples, child.side_kinds, strict=True))
        if len(transformed) != len(target):
            return False
        reversed_target = tuple(
            (tuple(reversed(samples)), side_kind) for samples, side_kind in reversed(target)
        )
        presentations = tuple(
            target[offset:] + target[:offset] for offset in range(len(target))
        ) + tuple(
            reversed_target[offset:] + reversed_target[:offset] for offset in range(len(target))
        )
        return any(
            all(
                left_kind == right_kind
                and all(
                    math.dist(left_point, right_point) <= metric_bound
                    for left_point, right_point in zip(left_samples, right_samples, strict=True)
                )
                for (left_samples, left_kind), (right_samples, right_kind) in zip(
                    transformed, candidate, strict=True
                )
            )
            for candidate in presentations
        )

    rotations = tuple(
        rotation
        for rotation in PROPER_ROTATIONS
        if abs(
            sum(
                left * right
                for left, right in zip(rotate(rotation, source.axis), ordered[0].axis, strict=True)
            )
        )
        == pytest.approx(1.0, abs=1e-12)
        and all(point_cycle_matches(rotation, child) for child in ordered)
    )
    assert rotations
    for child in ordered:
        assert len(child.axial_pairs) == len(source.axial_pairs)
    for left, right in zip(ordered, ordered[1:], strict=False):
        assert _raw_cycle_presentations(left.high_cycle) & _raw_cycle_presentations(right.low_cycle)
        assert left.high_material_side == -right.low_material_side
    child_volume = sum(child.volume for child in ordered)
    weighted_child_com = tuple(
        sum(child.volume * child.centre_of_mass[at] for child in ordered) / child_volume
        for at in range(3)
    )
    witnesses = []
    for rotation in rotations:
        target_axis = ordered[0].axis
        target_midpoint = (ordered[0].lo + ordered[-1].hi) / 2.0
        first_profile = ordered[0].profile_centre
        target_anchor = tuple(
            value
            + (
                target_midpoint
                - sum(a * b for a, b in zip(first_profile, target_axis, strict=True))
            )
            * axis_value
            for value, axis_value in zip(first_profile, target_axis, strict=True)
        )
        for child in ordered:
            child_anchor = tuple(
                value
                - sum(a * b for a, b in zip(child.profile_centre, target_axis, strict=True))
                * axis_value
                for value, axis_value in zip(child.profile_centre, target_axis, strict=True)
            )
            target_line = tuple(
                value
                - sum(a * b for a, b in zip(target_anchor, target_axis, strict=True)) * axis_value
                for value, axis_value in zip(target_anchor, target_axis, strict=True)
            )
            assert math.dist(child_anchor, target_line) <= 2.0 * (
                ordered[0].metric_quantum + child.metric_quantum
            )
        rotated_anchor = rotate(rotation, source.profile_centre)
        translation = tuple(
            target - scale * value
            for target, value in zip(target_anchor, rotated_anchor, strict=True)
        )
        rotated_com = rotate(rotation, source.centre_of_mass)
        transformed_com = tuple(
            scale * value + shift for value, shift in zip(rotated_com, translation, strict=True)
        )
        first_moment = tuple(
            sum(
                child.volume * (child.centre_of_mass[at] - transformed_com[at]) for child in ordered
            )
            for at in range(3)
        )
        volume_bound = 2.0 * (
            scale**3 * source.volume_quantum + sum(child.volume_quantum for child in ordered)
        )
        assert abs(scale**3 * source.volume - child_volume) <= volume_bound
        volume_errors = tuple(2.0 * child.volume_quantum for child in ordered)
        assert (
            sum(child.volume - error for child, error in zip(ordered, volume_errors, strict=True))
            > 0.0
        )
        first_moment_bound = 0.0
        child_volume_upper = 0.0
        for child, volume_error in zip(ordered, volume_errors, strict=True):
            metric_error = 2.0 * child.metric_quantum
            volume_upper = abs(child.volume) + volume_error
            child_volume_upper += volume_upper
            first_moment_bound += volume_upper * metric_error + volume_error * math.dist(
                child.centre_of_mass, transformed_com
            )
        first_moment_bound += child_volume_upper * 2.0 * scale * source.metric_quantum
        assert math.sqrt(sum(value * value for value in first_moment)) <= first_moment_bound
        witnesses.append((rotation, translation, scale))
    return tuple(witnesses), weighted_child_com


def _raw_partition_exact_covers(parents, children, *, include_singletons=False):
    """Enumerate witnessed singleton/partition hyperedges and exact covers independently."""

    edges = []
    for parent_at, parent in enumerate(parents):
        first_size = 1 if include_singletons else 2
        for size in range(first_size, len(children) + 1):
            for child_positions in combinations(range(len(children)), size):
                try:
                    witnesses, _com = _raw_partition_relation_oracle(
                        (parent,), tuple(children[at] for at in child_positions)
                    )
                except AssertionError:
                    continue
                edges.extend(
                    (
                        parent_at,
                        child_positions,
                        witness,
                        "singleton" if size == 1 else "partition",
                    )
                    for witness in witnesses
                )

    covers = []

    def extend(parent_at, used_children, selected):
        if parent_at == len(parents):
            if used_children == frozenset(range(len(children))):
                covers.append(tuple(selected))
            return
        for edge in edges:
            edge_parent, edge_children, _witness, _kind = edge
            if edge_parent != parent_at or used_children & frozenset(edge_children):
                continue
            extend(
                parent_at + 1,
                used_children | frozenset(edge_children),
                (*selected, edge),
            )

    extend(0, frozenset(), ())
    return tuple(edges), tuple(covers)


def _raw_joint_exact_covers(before, after):
    """Build both singleton and split/merge raw hypotheses, then exact-cover them."""

    edges = []

    def add(before_positions, after_positions, witnesses, kind):
        edges.extend(
            (tuple(before_positions), tuple(after_positions), witness, kind)
            for witness in witnesses
        )

    for before_at, before_fact in enumerate(before):
        for size in range(1, len(after) + 1):
            for after_positions in combinations(range(len(after)), size):
                try:
                    witnesses, _com = _raw_partition_relation_oracle(
                        (before_fact,), tuple(after[at] for at in after_positions)
                    )
                except AssertionError:
                    continue
                add(
                    (before_at,),
                    after_positions,
                    witnesses,
                    "singleton" if size == 1 else "split",
                )
    for after_at, after_fact in enumerate(after):
        for size in range(2, len(before) + 1):
            for before_positions in combinations(range(len(before)), size):
                try:
                    witnesses, _com = _raw_partition_relation_oracle(
                        (after_fact,), tuple(before[at] for at in before_positions)
                    )
                except AssertionError:
                    continue
                add(
                    before_positions,
                    (after_at,),
                    tuple(
                        (
                            tuple(
                                tuple(rotation[column][row] for column in range(3))
                                for row in range(3)
                            ),
                            tuple(
                                -sum(
                                    rotation[column][row] * translation[column]
                                    for column in range(3)
                                )
                                / scale
                                for row in range(3)
                            ),
                            1.0 / scale,
                        )
                        for rotation, translation, scale in witnesses
                    ),
                    "merge",
                )

    degree_before = frozenset(at for edge in edges for at in edge[0])
    degree_after = frozenset(at for edge in edges for at in edge[1])
    covers = []

    def extend(edge_at, used_before, used_after, selected):
        if edge_at == len(edges):
            if used_before == degree_before and used_after == degree_after:
                covers.append(tuple(selected))
            return
        edge = edges[edge_at]
        before_positions, after_positions = frozenset(edge[0]), frozenset(edge[1])
        extend(edge_at + 1, used_before, used_after, selected)
        if not used_before & before_positions and not used_after & after_positions:
            extend(
                edge_at + 1,
                used_before | before_positions,
                used_after | after_positions,
                (*selected, edge),
            )

    extend(0, frozenset(), frozenset(), ())
    degree_zero_before = frozenset(range(len(before))) - degree_before
    degree_zero_after = frozenset(range(len(after))) - degree_after
    return tuple(edges), tuple(covers), degree_zero_before, degree_zero_after


def _snapshot_partition_witness(before_product, after_product, rotation):
    """Derive the affine witness directly from frozen partition snapshot facts."""

    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    parent = before.occurrences[0]
    children = after.occurrences
    axis_at = "xyz".index(children[0].summary.axis)
    target_lo = min(item.summary.span[0] for item in children)
    target_hi = max(item.summary.span[1] for item in children)
    scale = (target_hi - target_lo) / (parent.summary.span[1] - parent.summary.span[0])
    rotated_centre = _rotate(rotation, parent.summary.centre)
    zero_bound = 2.0 * (
        scale * parent.body.quantization.metric_quantum
        + min(item.body.quantization.metric_quantum for item in children)
    )
    candidates = []
    for child in children:
        target_centre = list(child.summary.centre)
        target_centre[axis_at] = (target_lo + target_hi) / 2.0
        candidates.append(
            _normalize_partition_translation(
                tuple(
                    target - scale * source
                    for source, target in zip(rotated_centre, target_centre, strict=True)
                ),
                zero_bound,
            )
        )
    translation = tuple(
        math.fsum(candidate[at] for candidate in candidates) / len(candidates) for at in range(3)
    )
    return RigidScaleWitness(
        rotation,
        translation,
        scale,
    )


def test_empty_products_have_one_successful_empty_correspondence() -> None:
    before = _take_inventory(Box(10, 10, 10))
    after = _take_inventory(Box(20, 10, 10))
    result = correspondence_changes(before, after)
    assert result.schema_version == 2
    assert result.before_schema == result.after_schema == 3
    assert result.relations == ()


def test_exact_occurrences_are_unchanged_without_symmetry_witnesses() -> None:
    before = _take_inventory(_line_rrp(5))
    after = _take_inventory(_line_rrp(5))
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [ChangeKind.UNCHANGED]
    (relation,) = result.relations
    assert relation.witness is None
    assert relation.candidate_witnesses == ()
    assert relation.before_refs[0].occurrence == correspondence_snapshot(before).occurrences[0]
    assert relation.after_refs[0].occurrence == correspondence_snapshot(after).occurrences[0]


def test_exact_equal_distinct_body_groups_remain_one_ambiguous_component() -> None:
    before = _take_inventory(Compound([_rrp(5), _rrp(5)]))
    after = _take_inventory(Compound([_rrp(5), _rrp(5)]))
    before_snapshot = correspondence_snapshot(before)
    after_snapshot = correspondence_snapshot(after)
    assert before_snapshot.body_groups == after_snapshot.body_groups == ((0,), (1,))
    assert before_snapshot.occurrences[0] == before_snapshot.occurrences[1]

    (relation,) = correspondence_changes(before, after).relations
    assert relation.kind is ChangeKind.AMBIGUOUS
    assert tuple(ref.position for ref in relation.before_refs) == (0, 1)
    assert tuple(ref.position for ref in relation.after_refs) == (0, 1)
    assert relation.witness is None


def test_empty_to_nonempty_and_inverse_preserve_every_occurrence() -> None:
    empty = _take_inventory(Box(10, 10, 10))
    populated = _take_inventory(_line_rrp(5))
    added = correspondence_changes(empty, populated)
    removed = correspondence_changes(populated, empty)
    assert [relation.kind for relation in added.relations] == [ChangeKind.ADDED]
    assert [relation.kind for relation in removed.relations] == [ChangeKind.REMOVED]
    assert (
        added.relations[0].after_refs[0].occurrence
        == removed.relations[0].before_refs[0].occurrence
    )


def test_unique_two_child_geometric_partition_and_inverse() -> None:
    whole_part = _partition_rrp(10.0)
    pieces_part = Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)])
    raw_witnesses, _raw_com = _raw_partition_relation_oracle(
        _raw_prism_partition_oracle(whole_part),
        _raw_prism_partition_oracle(pieces_part),
    )
    assert len(raw_witnesses) == 1
    raw_rotation, raw_translation, raw_scale = raw_witnesses[0]
    raw_zero_bound = 2.0 * (
        raw_scale * _raw_prism_partition_oracle(whole_part)[0].metric_quantum
        + min(fact.metric_quantum for fact in _raw_prism_partition_oracle(pieces_part))
    )
    raw_witness = RigidScaleWitness(
        raw_rotation,
        (0.0, 0.0, 0.0)
        if sum(value * value for value in raw_translation) <= raw_zero_bound**2
        else raw_translation,
        raw_scale,
    )
    whole = _take_inventory(whole_part)
    pieces = _take_inventory(pieces_part)
    split = correspondence_changes(whole, pieces)
    merge = correspondence_changes(pieces, whole)

    assert split.schema_version == merge.schema_version == 2
    (split_relation,) = split.relations
    (merge_relation,) = merge.relations
    assert split_relation.kind is ChangeKind.SPLIT
    assert merge_relation.kind is ChangeKind.MERGED
    assert len(split_relation.before_refs) == len(merge_relation.after_refs) == 1
    assert len(split_relation.after_refs) == len(merge_relation.before_refs) == 2
    assert split_relation.witness is not None
    assert split_relation.witness == raw_witness
    assert merge_relation.witness == _inverse_witness(split_relation.witness)
    assert merge_relation.witness == _inverse_witness(raw_witness)


@pytest.mark.parametrize(
    ("factory", "repeats", "expected"),
    [
        (_partition_rrp, 5, ChangeKind.SPLIT),
        (_mixed_partition_rrp, 7, ChangeKind.SPLIT),
        (_partition_rrp, 8, ChangeKind.AMBIGUOUS),
    ],
)
def test_reviewed_line_mixed_and_higher_prism_roster(
    factory, repeats: int, expected: ChangeKind
) -> None:
    whole_part = factory(10.0, repeats=repeats)
    pieces_part = Compound(
        [
            factory(4.0, repeats=repeats),
            factory(6.0, 4.0, repeats=repeats),
        ]
    )
    whole_oracle = _raw_prism_partition_oracle(whole_part)
    pieces_oracle = _raw_prism_partition_oracle(pieces_part)
    assert len(whole_oracle) == 1 and len(pieces_oracle) == 2
    witnesses, target_com = _raw_partition_relation_oracle(whole_oracle, pieces_oracle)
    raw_edges, raw_covers = _raw_partition_exact_covers(whole_oracle, pieces_oracle)
    expected_witness_count = 4 if repeats == 8 else 1
    assert len(raw_edges) == len(raw_covers) == expected_witness_count
    assert any(rotation == IDENTITY_ROTATION for rotation, _translation, _scale in witnesses)
    assert all(scale == pytest.approx(1.0) for _rotation, _translation, scale in witnesses)
    assert target_com == pytest.approx(whole_oracle[0].centre_of_mass)

    whole = _take_inventory(whole_part)
    pieces = _take_inventory(pieces_part)
    (relation,) = correspondence_changes(whole, pieces).relations
    assert relation.kind is expected
    raw_witnesses = {
        RigidScaleWitness(
            rotation,
            (0.0, 0.0, 0.0)
            if sum(value * value for value in translation)
            <= (
                2.0
                * (
                    scale * whole_oracle[0].metric_quantum
                    + min(fact.metric_quantum for fact in pieces_oracle)
                )
            )
            ** 2
            else translation,
            scale,
        )
        for rotation, translation, scale in witnesses
    }
    if expected is ChangeKind.AMBIGUOUS:
        assert len(relation.candidate_witnesses) == 4
        assert set(relation.candidate_witnesses) == raw_witnesses
    else:
        assert {relation.witness} == raw_witnesses


def test_duplicate_partition_alternatives_make_the_whole_component_ambiguous() -> None:
    whole_part = _partition_rrp(10.0)
    pieces_part = Compound(
        [
            _partition_rrp(4.0),
            _partition_rrp(4.0),
            _partition_rrp(6.0, 4.0),
        ]
    )
    raw_edges, raw_covers = _raw_partition_exact_covers(
        _raw_prism_partition_oracle(whole_part),
        _raw_prism_partition_oracle(pieces_part),
    )
    assert len(raw_edges) == 2
    assert raw_covers == ()
    whole = _take_inventory(whole_part)
    pieces = _take_inventory(pieces_part)
    (relation,) = correspondence_changes(whole, pieces).relations
    assert relation.kind is ChangeKind.AMBIGUOUS
    assert relation.candidate_witnesses


def test_singleton_and_partition_competition_is_one_whole_ambiguity() -> None:
    before_part = _partition_rrp(10.0)
    after_part = Compound(
        [
            _partition_rrp(10.0),
            Pos(50, 0, 0) * _partition_rrp(4.0),
            Pos(50, 0, 4) * _partition_rrp(6.0),
        ]
    )
    raw_edges, raw_covers = _raw_partition_exact_covers(
        _raw_prism_partition_oracle(before_part),
        _raw_prism_partition_oracle(after_part),
        include_singletons=True,
    )
    assert {edge[3] for edge in raw_edges} == {"singleton", "partition"}
    assert raw_covers == ()
    before = _take_inventory(before_part)
    after = _take_inventory(after_part)
    (relation,) = correspondence_changes(before, after).relations
    assert relation.kind is ChangeKind.AMBIGUOUS
    assert len(relation.before_refs) == 1
    assert len(relation.after_refs) == 3
    assert relation.candidate_witnesses


def test_raw_oracle_refuses_children_without_one_common_transverse_axis_line() -> None:
    parent = _raw_prism_partition_oracle(_partition_rrp(10.0))
    shifted = _raw_prism_partition_oracle(
        Compound(
            [
                Pos(1, 0, 0) * _partition_rrp(4.0),
                Pos(2, 0, 4) * _partition_rrp(6.0),
            ]
        )
    )
    with pytest.raises(AssertionError):
        _raw_partition_relation_oracle(parent, shifted)
    result = correspondence_changes(
        _take_inventory(_partition_rrp(10.0)),
        _take_inventory(
            Compound(
                [
                    Pos(1, 0, 0) * _partition_rrp(4.0),
                    Pos(2, 0, 4) * _partition_rrp(6.0),
                ]
            )
        ),
    )
    assert all(
        relation.kind not in {ChangeKind.SPLIT, ChangeKind.MERGED} for relation in result.relations
    )


def test_raw_prism_oracle_refuses_inner_wire_taper_and_twist() -> None:
    profile = Polygon((-10, -7), (11, -5), (8, 9), (-9, 8))
    tapered = loft([profile, Pos(0, 0, 10) * profile.scale(0.8)])
    twisted = loft([profile, Pos(0, 0, 10) * Rot(0, 0, 9) * profile])
    inner_wire = Cylinder(20, 10) - Cylinder(5, 10)
    for unsupported in (tapered, twisted, inner_wire):
        with pytest.raises(AssertionError):
            _raw_prism_partition_oracle(unsupported)


def test_product_pair_refuses_real_inner_wire_taper_twist_and_anisotropy() -> None:
    profile = Polygon((-10, -7), (11, -5), (8, 9), (-9, 8))
    tapered = loft([profile, Pos(0, 0, 10) * profile.scale(0.8)])
    twisted = loft([profile, Pos(0, 0, 10) * Rot(0, 0, 9) * profile])
    inner_wire = Cylinder(20, 10) - Cylinder(5, 10)
    parent = _take_inventory(_partition_rrp(10.0))
    for unsupported in (tapered, twisted, inner_wire):
        result = correspondence_changes(parent, _take_inventory(unsupported))
        assert all(
            relation.kind not in {ChangeKind.SPLIT, ChangeKind.MERGED}
            for relation in result.relations
        )

    def anisotropic_piece(height: float, start: float):
        points = []
        for sector in range(5):
            for offset, radius in enumerate((20.0, 16.0, 20.0, 18.0)):
                angle = 2.0 * math.pi * (sector / 5 + offset / 20)
                points.append((1.2 * radius * math.cos(angle), radius * math.sin(angle)))
        return Pos(0, 0, start) * Rot(0, 0, 13) * extrude(Polygon(*points), height)

    anisotropic_children = Compound([anisotropic_piece(4.0, 0.0), anisotropic_piece(6.0, 4.0)])
    with pytest.raises(AssertionError):
        _raw_partition_relation_oracle(
            _raw_prism_partition_oracle(_partition_rrp(10.0)),
            _raw_prism_partition_oracle(anisotropic_children),
        )
    result = correspondence_changes(parent, _take_inventory(anisotropic_children))
    assert all(
        relation.kind not in {ChangeKind.SPLIT, ChangeKind.MERGED} for relation in result.relations
    )


def test_gap_does_not_become_a_partial_geometric_partition() -> None:
    whole = _take_inventory(_partition_rrp(10.0))
    pieces = _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(5.0, 5.0)]))
    assert all(
        relation.kind not in {ChangeKind.SPLIT, ChangeKind.MERGED}
        for relation in correspondence_changes(whole, pieces).relations
    )


def test_overlap_does_not_become_a_geometric_partition() -> None:
    parent_snapshot = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    child_snapshot = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    parent_occurrence = parent_snapshot.occurrences[0]
    parent = _prism_fact_for(parent_occurrence)
    children = tuple(_prism_fact_for(value) for value in child_snapshot.occurrences)
    assert parent is not None and all(child is not None for child in children)
    child_facts = tuple(child for child in children if child is not None)
    overlapping = (child_facts[0], replace(child_facts[1], interval=(3.0, 9.0)))
    assert not _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        overlapping,
        _MatchBudget(),
    )


def test_ulps_of_section_centre_rebuild_drift_are_one_partition_witness() -> None:
    parent_snapshot = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    pieces = Compound(
        [_partition_rrp(2.0), _partition_rrp(3.0, 2.0), _partition_rrp(5.0, 5.0)]
    ).scale(1.25)
    pieces = Pos(3, -4, 7) * Rot(90, 0, 0) * pieces
    child_snapshot = correspondence_snapshot(_take_inventory(pieces))
    parent_occurrence = parent_snapshot.occurrences[0]
    parent = _prism_fact_for(parent_occurrence)
    children = tuple(_prism_fact_for(value) for value in child_snapshot.occurrences)
    assert parent is not None and all(child is not None for child in children)
    child_facts = tuple(child for child in children if child is not None)
    drifted = tuple(
        replace(
            occurrence,
            summary=replace(
                occurrence.summary,
                centre=(
                    occurrence.summary.centre[0],
                    occurrence.summary.centre[1] + (index - 1) * 8e-15,
                    occurrence.summary.centre[2],
                ),
            ),
        )
        for index, occurrence in enumerate(child_snapshot.occurrences)
    )

    witnesses = _partition_witnesses(
        parent_occurrence,
        parent,
        drifted,
        child_facts,
        _MatchBudget(),
    )

    assert len(witnesses) == 1
    assert witnesses[0].rotation == ((1, 0, 0), (0, 0, -1), (0, 1, 0))
    assert witnesses[0].translation == pytest.approx((3.0, -4.0, 7.0), abs=1e-12)


def test_partition_witness_canonicalization_is_clique_closed_and_swap_covariant() -> None:
    close = (
        RigidScaleWitness(IDENTITY_ROTATION, (3.0, -4.0 - 8e-15, 7.0), 1.25),
        RigidScaleWitness(IDENTITY_ROTATION, (3.0, -4.0, 7.0), 1.25),
        RigidScaleWitness(IDENTITY_ROTATION, (3.0, -4.0 + 8e-15, 7.0), 1.25),
    )
    budget = _MatchBudget()
    (canonical,) = _canonicalize_partition_witnesses(close, 2e-14, budget)
    assert budget.attempts == 3
    assert canonical.translation == pytest.approx((3.0, -4.0, 7.0), abs=1e-15)
    inverse = _canonicalize_partition_witnesses(
        tuple(_inverse_witness(witness) for witness in close),
        2e-14 / 1.25,
        _MatchBudget(),
    )
    expected_inverse = _inverse_witness(canonical)
    assert inverse[0].rotation == expected_inverse.rotation
    assert inverse[0].scale == pytest.approx(expected_inverse.scale, abs=1e-15)
    assert inverse[0].translation == pytest.approx(expected_inverse.translation, abs=1e-15)

    bridge = (
        RigidScaleWitness(IDENTITY_ROTATION, (0.0, 0.0, 0.0), 1.0),
        RigidScaleWitness(IDENTITY_ROTATION, (0.75, 0.0, 0.0), 1.0),
        RigidScaleWitness(IDENTITY_ROTATION, (1.5, 0.0, 0.0), 1.0),
    )
    assert _canonicalize_partition_witnesses(bridge, 1.0, _MatchBudget()) == bridge

    near_zero = (
        RigidScaleWitness(IDENTITY_ROTATION, (0.8, 0.0, 0.0), 1.0),
        RigidScaleWitness(IDENTITY_ROTATION, (1.2, 0.0, 0.0), 1.0),
    )
    (near_zero_canonical,) = _canonicalize_partition_witnesses(near_zero, 2.0, _MatchBudget())
    assert near_zero_canonical.translation == (1.0, 0.0, 0.0)


def test_partition_leaf_rejects_interface_pcurve_volume_and_first_moment_drift() -> None:
    parent_snapshot = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    child_snapshot = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    parent_occurrence = parent_snapshot.occurrences[0]
    parent = _prism_fact_for(parent_occurrence)
    children = tuple(_prism_fact_for(value) for value in child_snapshot.occurrences)
    assert parent is not None and all(child is not None for child in children)
    child_facts = tuple(child for child in children if child is not None)
    assert _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        child_facts,
        _MatchBudget(),
    )

    first = child_facts[0]
    curve = first.low_cap.section_curves[0]
    assert curve.start_parameter is not None
    changed_curve = replace(
        curve,
        start_parameter=(curve.start_parameter[0] + 1.0, curve.start_parameter[1]),
    )
    changed_low = replace(
        first.low_cap,
        section_curves=(changed_curve, *first.low_cap.section_curves[1:]),
    )
    mutations = (
        (
            first,
            replace(
                child_facts[1],
                low_cap=replace(child_facts[1].low_cap, section_curves=()),
            ),
        ),
        (
            first,
            replace(child_facts[1], low_cap=first.high_cap),
        ),
        (replace(first, low_cap=changed_low), *child_facts[1:]),
        (replace(first, volume=first.volume + 1.0), *child_facts[1:]),
        (
            replace(
                first,
                centre_of_mass=(first.centre_of_mass[0] + 1.0, *first.centre_of_mass[1:]),
            ),
            *child_facts[1:],
        ),
        (
            first,
            replace(
                child_facts[1],
                low_cap=replace(
                    child_facts[1].low_cap,
                    face=replace(
                        child_facts[1].low_cap.face,
                        material_side=first.high_cap.face.material_side,
                    ),
                ),
            ),
        ),
        (replace(first, interval=(first.interval[0], first.interval[0])), *child_facts[1:]),
        (
            replace(
                first,
                low_cap=replace(
                    first.low_cap,
                    section_curves=(
                        replace(first.low_cap.section_curves[0], kind="CIRCLE"),
                        *first.low_cap.section_curves[1:],
                    ),
                ),
            ),
            *child_facts[1:],
        ),
    )
    for changed_children in mutations:
        assert not _partition_witnesses(
            parent_occurrence,
            parent,
            child_snapshot.occurrences,
            changed_children,
            _MatchBudget(),
        )


def test_prism_fact_binds_summary_winding_and_exact_incidence() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_partition_rrp(10.0))).occurrences[0]
    graph = occurrence.matching_boundary
    summary = occurrence.summary
    assert _prism_fact_for(occurrence) is not None

    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(summary, repeat_count=summary.repeat_count + 1),
        )
        is None
    )
    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(summary, edge_count=summary.edge_count + 1),
        )
        is None
    )
    changed_signature = (
        "CIRCLE" if summary.sector_signature[0][0] == "LINE" else "LINE",
    ) + summary.sector_signature[0][1:]
    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(
                summary,
                sector_signature=(changed_signature, *summary.sector_signature[1:]),
            ),
        )
        is None
    )
    sampled = summary.sector_signature[0][2]
    changed_sample = (sampled[0][0] + 1.0, sampled[0][1])
    changed_samples = (changed_sample, *sampled[1:])
    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(
                summary,
                sector_signature=(
                    (*summary.sector_signature[0][:2], changed_samples),
                    *summary.sector_signature[1:],
                ),
            ),
        )
        is None
    )
    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(
                summary,
                centre=(summary.centre[0] + 1.0, *summary.centre[1:]),
            ),
        )
        is None
    )
    changed_defining = replace(summary.defining[0], area=summary.defining[0].area + 1.0)
    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(summary, defining=(changed_defining, summary.defining[1])),
        )
        is None
    )

    cap_at = next(
        at
        for at, face in enumerate(graph.faces)
        if face.kind == "PLANE"
        and len(face.parameters) == 4
        and abs(abs(face.parameters[2]) - 1.0) <= 4.0 * DIRECTION_TOL
    )
    cap = graph.faces[cap_at]
    inner_wire = replace(cap.wires[0], role="inner")
    inner_faces = (
        *graph.faces[:cap_at],
        replace(cap, wires=(cap.wires[0], inner_wire)),
        *graph.faces[cap_at + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=inner_faces)) is None

    changed_wire = replace(cap.wires[0], theta_winding=1)
    changed_faces = (
        *graph.faces[:cap_at],
        replace(cap, wires=(changed_wire,)),
        *graph.faces[cap_at + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=changed_faces)) is None

    half_edge = cap.wires[0].cycle[0]
    assert half_edge.start is not None
    changed_half_edge = replace(
        half_edge,
        start=replace(
            half_edge.start,
            parameter=(half_edge.start.parameter[0] + 1.0, half_edge.start.parameter[1]),
        ),
    )
    changed_cycle = (changed_half_edge, *cap.wires[0].cycle[1:])
    changed_wire = replace(cap.wires[0], cycle=changed_cycle)
    changed_faces = (
        *graph.faces[:cap_at],
        replace(cap, wires=(changed_wire,)),
        *graph.faces[cap_at + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=changed_faces)) is None

    cap_curve = cap.wires[0].cycle[0].curve
    incidence = dict(graph.incidence)
    incidence[cap_curve] = (*incidence[cap_curve], incidence[cap_curve][0])
    changed_incidence = tuple(sorted(incidence.items()))
    assert _prism_fact_for(occurrence, graph=replace(graph, incidence=changed_incidence)) is None


def test_prism_fact_rejects_complete_lateral_and_connector_topology_drift() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_partition_rrp(10.0))).occurrences[0]
    graph = occurrence.matching_boundary
    fact = _prism_fact_for(occurrence)
    assert fact is not None
    cap_positions = {fact.low_cap.face_position, fact.high_cap.face_position}
    side_position = next(at for at in range(len(graph.faces)) if at not in cap_positions)
    side = graph.faces[side_position]

    def changed_face(position, value):
        return replace(
            graph,
            faces=(*graph.faces[:position], value, *graph.faces[position + 1 :]),
        )

    assert (
        _prism_fact_for(occurrence, graph=changed_face(side_position, replace(side, kind="SPHERE")))
        is None
    )
    assert (
        _prism_fact_for(
            occurrence, graph=changed_face(side_position, replace(side, kind="CYLINDER"))
        )
        is None
    )
    assert (
        _prism_fact_for(
            occurrence,
            graph=changed_face(
                side_position,
                replace(side, parameters=(0.0, 0.0, 1.0, *side.parameters[3:])),
            ),
        )
        is None
    )
    assert (
        _prism_fact_for(occurrence, graph=changed_face(side_position, replace(side, wires=())))
        is None
    )

    joining_position = next(
        curve_at
        for curve_at, owners in graph.incidence
        if {owner[0] for owner in owners}.isdisjoint(cap_positions)
    )
    joining_curve = graph.curves[joining_position]
    assert joining_curve.vertices is not None
    changed_curves = (
        *graph.curves[:joining_position],
        replace(joining_curve, kind="CIRCLE"),
        *graph.curves[joining_position + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, curves=changed_curves)) is None
    start_at, end_at = joining_curve.vertices
    changed_vertices = list(graph.vertices)
    changed_vertices[end_at] = (
        changed_vertices[end_at][0] + 1.0,
        changed_vertices[end_at][1],
        changed_vertices[end_at][2],
    )
    assert (
        _prism_fact_for(occurrence, graph=replace(graph, vertices=tuple(changed_vertices))) is None
    )

    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(
                occurrence.summary,
                span=(occurrence.summary.span[0] + 1.0, occurrence.summary.span[1] + 1.0),
            ),
        )
        is None
    )


def test_prism_leaf_numeric_domains_fail_closed(monkeypatch) -> None:
    monkeypatch.setattr(partition_module, "DIRECTION_TOL", 2.0)
    with pytest.raises(ValueError, match="basis is degenerate"):
        partition_module._plane_basis((0.0, 0.0, 1.0))
    monkeypatch.setattr(partition_module, "DIRECTION_TOL", DIRECTION_TOL)
    with pytest.raises(ValueError, match="axis is not principal"):
        partition_module._polar_signature(((0.0, 0.0, 0.0),), (0.0, 0.0, 0.0), (1.0, 1.0, 0.0))


def test_prism_curve_leafs_refuse_missing_or_incompatible_geometry() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_partition_rrp(10.0))).occurrences[0]
    fact = _prism_fact_for(occurrence)
    assert fact is not None
    curve = fact.low_cap.section_curves[0]
    metric = occurrence.body.quantization.metric_quantum

    endpoint_free = replace(
        curve,
        start=None,
        end=None,
        start_parameter=None,
        end_parameter=None,
    )
    assert partition_module._plane_curve_parameters_match(endpoint_free, fact.low_cap.face, metric)
    assert not partition_module._plane_curve_parameters_match(
        replace(curve, start_parameter=None), fact.low_cap.face, metric
    )
    assert partition_module._sample_curve(endpoint_free) is None
    assert (
        partition_module._sample_curve(
            replace(curve, kind="CIRCLE", centre=None, axis=None, sweep=None)
        )
        is None
    )

    attempts = 0

    def charged() -> None:
        nonlocal attempts
        attempts += 1

    incompatible = replace(fact.high_cap.section_curves[0], kind="CIRCLE")
    high = (incompatible, *fact.high_cap.section_curves[1:])
    assert not partition_module._translated_cap_curves_match(
        fact.low_cap.section_curves, high, fact.axis, metric, charged
    )
    assert attempts > 0


def test_prism_fact_rejects_malformed_signature_and_complete_incidence_drift() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_mixed_partition_rrp(10.0))).occurrences[0]
    graph = occurrence.matching_boundary
    fact = _prism_fact_for(occurrence)
    assert fact is not None

    malformed_signature = (
        (
            occurrence.summary.sector_signature[0][0],
            occurrence.summary.sector_signature[0][1],
            [],
        ),
        *occurrence.summary.sector_signature[1:],
    )
    assert (
        _prism_fact_for(
            occurrence,
            summary=replace(occurrence.summary, sector_signature=malformed_signature),
        )
        is None
    )

    cap_positions = {fact.low_cap.face_position, fact.high_cap.face_position}
    side_positions = set(range(len(graph.faces))) - cap_positions
    curved_side_position = next(
        position for position in side_positions if graph.faces[position].kind == "CYLINDER"
    )
    curved_side = graph.faces[curved_side_position]
    changed_faces = (
        *graph.faces[:curved_side_position],
        replace(curved_side, kind="PLANE", parameters=(1.0, 0.0, 0.0, 0.0)),
        *graph.faces[curved_side_position + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=changed_faces)) is None

    straight_side_position = next(
        position for position in side_positions if graph.faces[position].kind == "PLANE"
    )
    straight_side = graph.faces[straight_side_position]
    changed_wire = replace(straight_side.wires[0], role="inner")
    changed_faces = (
        *graph.faces[:straight_side_position],
        replace(straight_side, wires=(changed_wire,)),
        *graph.faces[straight_side_position + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=changed_faces)) is None

    cap_curve_position = graph.faces[fact.low_cap.face_position].wires[0].cycle[0].curve
    changed_incidence = tuple(
        (position, owners[1:] if position == cap_curve_position else owners)
        for position, owners in graph.incidence
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, incidence=changed_incidence)) is None

    extra_face = replace(graph.faces[fact.low_cap.face_position], wires=())
    assert (
        _prism_fact_for(occurrence, graph=replace(graph, faces=(*graph.faces, extra_face))) is None
    )


def test_prism_fact_rejects_cap_sampling_and_side_cycle_authority_drift() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_partition_rrp(10.0))).occurrences[0]
    graph = occurrence.matching_boundary
    fact = _prism_fact_for(occurrence)
    assert fact is not None

    low_curve_position = graph.faces[fact.low_cap.face_position].wires[0].cycle[0].curve
    low_face = graph.faces[fact.low_cap.face_position]
    low_half_edge = low_face.wires[0].cycle[0]
    changed_low_cycle = (
        replace(low_half_edge, start=None, end=None),
        *low_face.wires[0].cycle[1:],
    )
    changed_faces = (
        *graph.faces[: fact.low_cap.face_position],
        replace(low_face, wires=(replace(low_face.wires[0], cycle=changed_low_cycle),)),
        *graph.faces[fact.low_cap.face_position + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=changed_faces)) is None

    high_face = graph.faces[fact.high_cap.face_position]
    high_half_edge = high_face.wires[0].cycle[0]
    changed_high_cycle = (
        replace(high_half_edge, direction=-high_half_edge.direction),
        *high_face.wires[0].cycle[1:],
    )
    changed_faces = (
        *graph.faces[: fact.high_cap.face_position],
        replace(high_face, wires=(replace(high_face.wires[0], cycle=changed_high_cycle),)),
        *graph.faces[fact.high_cap.face_position + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=changed_faces)) is None

    cap_positions = {fact.low_cap.face_position, fact.high_cap.face_position}
    side_position = fact.low_cap.side_faces[0]
    side = graph.faces[side_position]
    changed_cycle = tuple(item for item in side.wires[0].cycle if item.curve != low_curve_position)
    changed_faces = (
        *graph.faces[:side_position],
        replace(side, wires=(replace(side.wires[0], cycle=changed_cycle),)),
        *graph.faces[side_position + 1 :],
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, faces=changed_faces)) is None

    joining_position = next(
        position
        for position, owners in graph.incidence
        if {owner[0] for owner in owners}.isdisjoint(cap_positions)
    )
    changed_incidence = tuple(
        (position, owners[:1] if position == joining_position else owners)
        for position, owners in graph.incidence
    )
    assert _prism_fact_for(occurrence, graph=replace(graph, incidence=changed_incidence)) is None


def test_two_independent_partitions_share_one_exact_cover_without_order_authority() -> None:
    before = _take_inventory(
        Compound(
            [
                Pos(-40, 0, 0) * _partition_rrp(10.0),
                Pos(40, 0, 0) * _partition_rrp(10.0, repeats=7),
            ]
        )
    )
    after = _take_inventory(
        Compound(
            [
                Pos(40, 0, 0) * _partition_rrp(3.0, repeats=7),
                Pos(-40, 0, 0) * _partition_rrp(4.0),
                Pos(40, 0, 3) * _partition_rrp(7.0, repeats=7),
                Pos(-40, 0, 4) * _partition_rrp(6.0),
            ]
        )
    )
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [
        ChangeKind.SPLIT,
        ChangeKind.SPLIT,
    ]


def test_raw_joint_graph_covers_simultaneous_split_and_merge() -> None:
    before_part = Compound(
        [
            Pos(-60, 0, 0) * _partition_rrp(10.0),
            Pos(60, 0, 0) * _partition_rrp(4.0, repeats=7),
            Pos(60, 0, 4) * _partition_rrp(6.0, repeats=7),
        ]
    )
    after_part = Compound(
        [
            Pos(-60, 0, 0) * _partition_rrp(4.0),
            Pos(-60, 0, 4) * _partition_rrp(6.0),
            Pos(60, 0, 0) * _partition_rrp(10.0, repeats=7),
        ]
    )
    raw_edges, raw_covers, degree_zero_before, degree_zero_after = _raw_joint_exact_covers(
        _raw_prism_partition_oracle(before_part),
        _raw_prism_partition_oracle(after_part),
    )
    assert degree_zero_before == degree_zero_after == frozenset()
    assert len(raw_covers) == 1
    assert {edge[3] for edge in raw_covers[0]} == {"split", "merge"}
    assert {edge[3] for edge in raw_edges} >= {"split", "merge"}
    result = correspondence_changes(_take_inventory(before_part), _take_inventory(after_part))
    assert {relation.kind for relation in result.relations} == {
        ChangeKind.SPLIT,
        ChangeKind.MERGED,
    }


def test_raw_joint_graph_keeps_degree_zero_add_remove_outside_exact_cover() -> None:
    before_part = _partition_rrp(10.0, repeats=5)
    after_part = Pos(60, 0, 0) * _partition_rrp(10.0, repeats=7)
    edges, covers, degree_zero_before, degree_zero_after = _raw_joint_exact_covers(
        _raw_prism_partition_oracle(before_part),
        _raw_prism_partition_oracle(after_part),
    )
    assert edges == ()
    assert covers == ((),)
    assert degree_zero_before == degree_zero_after == frozenset({0})
    result = correspondence_changes(_take_inventory(before_part), _take_inventory(after_part))
    assert {relation.kind for relation in result.relations} == {
        ChangeKind.ADDED,
        ChangeKind.REMOVED,
    }
    added = next(relation for relation in result.relations if relation.kind is ChangeKind.ADDED)
    removed = next(relation for relation in result.relations if relation.kind is ChangeKind.REMOVED)
    assert (len(added.before_refs), len(added.after_refs)) == (0, 1)
    assert (len(removed.before_refs), len(removed.after_refs)) == (1, 0)


def test_partition_and_unrelated_multi_occurrence_f6b1_group_share_joint_cover() -> None:
    before = _take_inventory(
        Compound(
            [
                Pos(-90, 0, 0) * _partition_rrp(10.0),
                Pos(90, 0, 0) * _two_rrp_one_solid(),
            ]
        )
    )
    after = _take_inventory(
        Compound(
            [
                Pos(-90, 0, 0) * _partition_rrp(4.0),
                Pos(-90, 0, 4) * _partition_rrp(6.0),
                Pos(90, 0, 0) * _two_rrp_one_solid(),
            ]
        )
    )
    before_snapshot = correspondence_snapshot(before)
    after_snapshot = correspondence_snapshot(after)
    before_multi = next(group for group in before_snapshot.body_groups if len(group) == 2)
    after_multi = next(group for group in after_snapshot.body_groups if len(group) == 2)
    rows = tuple(
        tuple(
            after_at
            for after_at in after_multi
            if before_snapshot.occurrences[before_at] == after_snapshot.occurrences[after_at]
        )
        for before_at in before_multi
    )
    assert all(len(row) == 1 for row in rows)
    exact_group_bijection = tuple(row[0] for row in rows)
    assert len(set(exact_group_bijection)) == len(exact_group_bijection)

    raw_split_witnesses, _raw_com = _raw_partition_relation_oracle(
        _raw_prism_partition_oracle(_partition_rrp(10.0)),
        _raw_prism_partition_oracle(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)])),
    )
    assert len(raw_split_witnesses) == 1
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations].count(ChangeKind.SPLIT) == 1
    assert [relation.kind for relation in result.relations].count(ChangeKind.UNCHANGED) == 2
    assert all(
        relation.kind not in {ChangeKind.ADDED, ChangeKind.REMOVED} for relation in result.relations
    )


def test_independent_moved_resized_body_group_witness_and_competition() -> None:
    rotation = ((0, -1, 0), (1, 0, 0), (0, 0, 1))
    scale = 1.25
    translation = (11.0, -7.0, 3.0)
    source_part = _two_rrp_one_solid()
    target_part = Pos(*translation) * _proper_transform(source_part.scale(scale), rotation)
    before_product = _take_inventory(source_part)
    after_product = _take_inventory(target_part)
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    assert before.body_groups == after.body_groups == ((0, 1),)

    rows = []
    for left in before.occurrences:
        matches = []
        for right_at, right in enumerate(after.occurrences):
            if (
                left.summary.repeat_count != right.summary.repeat_count
                or left.summary.edge_count != right.summary.edge_count
            ):
                continue
            rotated = tuple(
                sum(rotation[row][column] * left.summary.centre[column] for column in range(3))
                for row in range(3)
            )
            expected_centre = tuple(
                scale * value + offset for value, offset in zip(rotated, translation, strict=True)
            )
            if math.dist(expected_centre, right.summary.centre) > 1.0e-6:
                continue
            if right.summary.span != pytest.approx(
                tuple(scale * value + translation[2] for value in left.summary.span),
                abs=1.0e-6,
            ):
                continue
            volume_bound = 2.0 * (
                scale**3 * left.body.quantization.volume_quantum
                + right.body.quantization.volume_quantum
            )
            area_bound = 2.0 * (
                scale**2 * left.body.quantization.area_quantum
                + right.body.quantization.area_quantum
            )
            if (
                abs(right.body.intrinsic.volume - scale**3 * left.body.intrinsic.volume)
                > volume_bound
                or abs(
                    right.body.intrinsic.surface_area - scale**2 * left.body.intrinsic.surface_area
                )
                > area_bound
            ):
                continue
            matches.append(right_at)
        rows.append(tuple(matches))
    assert all(len(row) == 1 for row in rows)
    assert len({row[0] for row in rows}) == 2

    def derive_witness(left_snapshot, right_snapshot, right_group=(0, 1)):
        left = next(item for item in left_snapshot.occurrences if item.summary.repeat_count == 5)
        right = next(
            right_snapshot.occurrences[at]
            for at in right_group
            if right_snapshot.occurrences[at].summary.repeat_count == 5
        )
        exact_scale = (
            right.body.quantization.characteristic_scale
            / left.body.quantization.characteristic_scale
        )
        rotated = tuple(
            sum(
                rotation[row][column] * left.body.placement.centre_of_mass[column]
                for column in range(3)
            )
            for row in range(3)
        )
        exact_translation = tuple(
            target - exact_scale * source
            for source, target in zip(rotated, right.body.placement.centre_of_mass, strict=True)
        )
        return RigidScaleWitness(rotation, exact_translation, exact_scale)

    expected = derive_witness(before, after)
    result = correspondence_changes(before_product, after_product)
    assert [relation.kind for relation in result.relations] == [
        ChangeKind.RESIZED,
        ChangeKind.RESIZED,
    ]
    assert {relation.witness for relation in result.relations} == {expected}

    duplicated_target = Compound([target_part, Pos(100, 0, 0) * target_part])
    duplicated_product = _take_inventory(duplicated_target)
    duplicated_snapshot = correspondence_snapshot(duplicated_product)
    (ambiguous,) = correspondence_changes(before_product, duplicated_product).relations
    assert ambiguous.kind is ChangeKind.AMBIGUOUS
    assert (len(ambiguous.before_refs), len(ambiguous.after_refs)) == (2, 4)
    assert set(ambiguous.candidate_witnesses) == {
        derive_witness(before, duplicated_snapshot, group)
        for group in duplicated_snapshot.body_groups
    }


def test_raw_joint_graph_exposes_connected_many_to_many_cover_ambiguity() -> None:
    before_part = Compound([_partition_rrp(10.0), Pos(50, 0, 0) * _partition_rrp(10.0)])
    after_part = Compound(
        [
            _partition_rrp(4.0),
            _partition_rrp(6.0, 4.0),
            Pos(50, 0, 0) * _partition_rrp(4.0),
            Pos(50, 0, 4) * _partition_rrp(6.0),
        ]
    )
    before = _raw_prism_partition_oracle(before_part)
    after = _raw_prism_partition_oracle(after_part)
    edges, covers, degree_zero_before, degree_zero_after = _raw_joint_exact_covers(before, after)
    assert degree_zero_before == degree_zero_after == frozenset()
    assert len(edges) >= 4
    assert len(covers) > 1
    (relation,) = correspondence_changes(
        _take_inventory(before_part), _take_inventory(after_part)
    ).relations
    assert relation.kind is ChangeKind.AMBIGUOUS
    assert (len(relation.before_refs), len(relation.after_refs)) == (2, 4)
    assert set(relation.candidate_witnesses) == {
        RigidScaleWitness(rotation=edge[2][0], translation=edge[2][1], scale=edge[2][2])
        for cover in covers
        for edge in cover
    }


def test_three_child_partition_accepts_one_shared_moved_scaled_rotation() -> None:
    whole_part = _partition_rrp(10.0)
    pieces = Compound(
        [
            _partition_rrp(2.0),
            _partition_rrp(3.0, 2.0),
            _partition_rrp(5.0, 5.0),
        ]
    ).scale(1.25)
    pieces = Pos(3, -4, 7) * Rot(90, 0, 0) * pieces
    raw_witnesses, _raw_com = _raw_partition_relation_oracle(
        _raw_prism_partition_oracle(whole_part),
        _raw_prism_partition_oracle(pieces),
    )
    assert len(raw_witnesses) == 1
    before_product = _take_inventory(whole_part)
    after_product = _take_inventory(pieces)
    (relation,) = correspondence_changes(before_product, after_product).relations
    assert relation.kind is ChangeKind.SPLIT
    assert len(relation.after_refs) == 3
    assert relation.witness is not None
    assert relation.witness.rotation == ((1, 0, 0), (0, 0, -1), (0, 1, 0))
    assert relation.witness.scale == pytest.approx(1.25, abs=1e-7)
    assert relation.witness.translation == pytest.approx((3.0, -4.0, 7.0), abs=1e-6)
    raw_rotation, raw_translation, raw_scale = raw_witnesses[0]
    assert raw_rotation == relation.witness.rotation
    assert raw_scale == pytest.approx(relation.witness.scale, abs=1e-7)
    assert raw_translation == pytest.approx(relation.witness.translation, abs=1e-7)
    snapshot_witness = _snapshot_partition_witness(
        before_product,
        after_product,
        ((1, 0, 0), (0, 0, -1), (0, 1, 0)),
    )
    assert relation.witness.rotation == snapshot_witness.rotation
    assert relation.witness.scale == snapshot_witness.scale
    before_snapshot = correspondence_snapshot(before_product)
    after_snapshot = correspondence_snapshot(after_product)
    witness_bound = 4.0 * (
        relation.witness.scale * before_snapshot.occurrences[0].body.quantization.metric_quantum
        + min(
            occurrence.body.quantization.metric_quantum for occurrence in after_snapshot.occurrences
        )
    )
    assert math.dist(relation.witness.translation, snapshot_witness.translation) <= witness_bound


def test_raw_partition_oracle_and_matcher_cover_all_24_proper_rotations() -> None:
    whole = _partition_rrp(10.0)
    raw_parent = _raw_prism_partition_oracle(whole)
    before = _take_inventory(whole)
    before_snapshot = correspondence_snapshot(before)
    low = _partition_rrp(4.0)
    high = _partition_rrp(6.0, 4.0)
    for rotation in _proper_signed_permutations():
        transformed = Compound(
            [_proper_transform(low, rotation), _proper_transform(high, rotation)]
        )
        raw = _raw_prism_partition_oracle(transformed)
        assert len(raw) == 2
        raw_witnesses, _target_com = _raw_partition_relation_oracle(raw_parent, raw)
        assert len(raw_witnesses) == 1
        raw_rotation, _raw_translation, _raw_scale = raw_witnesses[0]
        after = _take_inventory(transformed)
        after_snapshot = correspondence_snapshot(after)
        parent_occurrence = before_snapshot.occurrences[0]
        children = after_snapshot.occurrences
        axis_at = "xyz".index(children[0].summary.axis)
        target_lo = min(item.summary.span[0] for item in children)
        target_hi = max(item.summary.span[1] for item in children)
        expected_scale = (target_hi - target_lo) / (
            parent_occurrence.summary.span[1] - parent_occurrence.summary.span[0]
        )
        target_centre = list(children[0].summary.centre)
        target_centre[axis_at] = (target_lo + target_hi) / 2.0
        rotated_centre = _rotate(rotation, parent_occurrence.summary.centre)
        expected_translation = tuple(
            target - expected_scale * source
            for source, target in zip(rotated_centre, target_centre, strict=True)
        )
        zero_bound = 2.0 * (
            expected_scale * parent_occurrence.body.quantization.metric_quantum
            + min(item.body.quantization.metric_quantum for item in children)
        )
        expected = RigidScaleWitness(
            rotation,
            _normalize_partition_translation(expected_translation, zero_bound),
            expected_scale,
        )
        (relation,) = correspondence_changes(before, after).relations
        assert relation.kind is ChangeKind.SPLIT
        assert relation.witness == expected
        assert raw_rotation == rotation


def test_partition_is_stable_through_step_roundtrip(tmp_path) -> None:
    whole_path = tmp_path / "partition-whole.step"
    pieces_path = tmp_path / "partition-pieces.step"
    whole_part = _partition_rrp(10.0)
    pieces_part = Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)])
    export_step(whole_part, whole_path)
    export_step(pieces_part, pieces_path)
    imported_whole = import_step(whole_path)
    imported_pieces = import_step(pieces_path)
    raw_whole = _raw_prism_partition_oracle(imported_whole)
    raw_pieces = _raw_prism_partition_oracle(imported_pieces)
    assert len(raw_whole) == 1
    assert len(raw_pieces) == 2
    raw_witnesses, _raw_com = _raw_partition_relation_oracle(raw_whole, raw_pieces)
    assert len(raw_witnesses) == 1
    before_product = _take_inventory(imported_whole)
    after_product = _take_inventory(imported_pieces)
    (relation,) = correspondence_changes(before_product, after_product).relations
    assert relation.kind is ChangeKind.SPLIT
    assert relation.witness is not None
    raw_rotation, raw_translation, raw_scale = raw_witnesses[0]
    assert raw_rotation == relation.witness.rotation
    assert raw_scale == pytest.approx(relation.witness.scale, abs=1e-7)
    assert raw_translation == pytest.approx(relation.witness.translation, abs=1e-7)
    assert relation.witness == _snapshot_partition_witness(
        before_product, after_product, IDENTITY_ROTATION
    )


def test_partition_witness_is_independent_of_unequal_child_presentation_order() -> None:
    before = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    after = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    assert after.occurrences[0].body.quantization != after.occurrences[1].body.quantization
    direct = _compare_snapshots(before, after)
    reversed_after = replace(
        after,
        occurrences=tuple(reversed(after.occurrences)),
        body_groups=((0,), (1,)),
    )
    reversed_result = _compare_snapshots(before, reversed_after)
    assert [relation.kind for relation in direct.relations] == [ChangeKind.SPLIT]
    assert [relation.kind for relation in reversed_result.relations] == [ChangeKind.SPLIT]
    assert direct.relations[0].witness == reversed_result.relations[0].witness


def test_partition_translation_zero_bound_is_euclidean_and_inclusive() -> None:
    bound = 1.0e-6
    diagonal = bound / math.sqrt(2.0)
    assert _normalize_partition_translation((diagonal, diagonal, 0.0), bound) == (
        0.0,
        0.0,
        0.0,
    )
    outside = math.nextafter(diagonal, math.inf)
    assert _normalize_partition_translation((outside, outside, 0.0), bound) == (
        outside,
        outside,
        0.0,
    )
    assert _normalize_partition_translation((bound, 0.0, 0.0), bound) == (0.0, 0.0, 0.0)
    axial_outside = math.nextafter(bound, math.inf)
    assert _normalize_partition_translation((axial_outside, 0.0, 0.0), bound) == (
        axial_outside,
        0.0,
        0.0,
    )


def test_partition_interval_join_is_inclusive_and_nextafter_closed() -> None:
    parent_snapshot = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    child_snapshot = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    parent_occurrence = parent_snapshot.occurrences[0]
    parent = _prism_fact_for(parent_occurrence)
    children = tuple(_prism_fact_for(value) for value in child_snapshot.occurrences)
    assert parent is not None and all(child is not None for child in children)
    left, right = (child for child in children if child is not None)
    join_bound = 2.0 * (left.quantization.metric_quantum + right.quantization.metric_quantum)

    equality_gap_at = left.interval[1] + join_bound
    while abs(equality_gap_at - left.interval[1]) > join_bound:
        equality_gap_at = math.nextafter(equality_gap_at, left.interval[1])
    equality_gap = (left, replace(right, interval=(equality_gap_at, right.interval[1])))
    assert _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        equality_gap,
        _MatchBudget(),
    )
    outside_gap = math.nextafter(equality_gap_at, math.inf)
    assert not _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        (left, replace(right, interval=(outside_gap, right.interval[1]))),
        _MatchBudget(),
    )

    equality_overlap_at = right.interval[0] + join_bound
    while abs(equality_overlap_at - right.interval[0]) > join_bound:
        equality_overlap_at = math.nextafter(equality_overlap_at, right.interval[0])
    equality_overlap = (
        replace(left, interval=(left.interval[0], equality_overlap_at)),
        right,
    )
    assert _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        equality_overlap,
        _MatchBudget(),
    )
    outside_overlap = math.nextafter(equality_overlap_at, math.inf)
    assert not _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        (replace(left, interval=(left.interval[0], outside_overlap)), right),
        _MatchBudget(),
    )


def test_partition_cap_metric_area_pcurve_and_angle_bounds_are_closed() -> None:
    snapshot = correspondence_snapshot(_take_inventory(_mixed_partition_rrp(10.0)))
    occurrence = snapshot.occurrences[0]
    fact = _prism_fact_for(occurrence)
    assert fact is not None
    before = fact.low_cap
    metric = 4.0 * fact.quantization.metric_quantum
    area = 4.0 * fact.quantization.area_quantum

    def similar(after):
        return _prism_cap_similarity(
            before,
            after,
            IDENTITY_ROTATION,
            1.0,
            2,
            metric,
            area,
            _MatchBudget(),
        )

    assert similar(before)
    area_equality = replace(before, face=replace(before.face, area=before.face.area + area))
    assert similar(area_equality)
    area_outside = math.nextafter(before.face.area + area, math.inf)
    while abs(area_outside - before.face.area) <= area:
        area_outside = math.nextafter(area_outside, math.inf)
    assert not similar(replace(before, face=replace(before.face, area=area_outside)))

    line_at = next(at for at, curve in enumerate(before.section_curves) if curve.kind == "LINE")
    line = before.section_curves[line_at]
    length_equality_value = line.length + metric
    while abs(length_equality_value - line.length) > metric:
        length_equality_value = math.nextafter(length_equality_value, line.length)
    length_equality = replace(line, length=length_equality_value)
    assert similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:line_at],
                length_equality,
                *before.section_curves[line_at + 1 :],
            ),
        )
    )
    length_outside = math.nextafter(length_equality_value, math.inf)
    assert not similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:line_at],
                replace(line, length=length_outside),
                *before.section_curves[line_at + 1 :],
            ),
        )
    )

    circle_at = next(at for at, curve in enumerate(before.section_curves) if curve.kind == "CIRCLE")
    circle = before.section_curves[circle_at]
    assert circle.radius is not None and circle.centre is not None
    radius_equality_value = circle.radius + metric
    while abs(radius_equality_value - circle.radius) > metric:
        radius_equality_value = math.nextafter(radius_equality_value, circle.radius)
    assert similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:circle_at],
                replace(circle, radius=radius_equality_value),
                *before.section_curves[circle_at + 1 :],
            ),
        )
    )
    assert not similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:circle_at],
                replace(circle, radius=math.nextafter(radius_equality_value, math.inf)),
                *before.section_curves[circle_at + 1 :],
            ),
        )
    )

    point_diagonal = metric / math.sqrt(2.0)
    centre_equality = (
        circle.centre[0] + point_diagonal,
        circle.centre[1] + point_diagonal,
        circle.centre[2],
    )
    assert similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:circle_at],
                replace(circle, centre=centre_equality),
                *before.section_curves[circle_at + 1 :],
            ),
        )
    )
    point_outside = math.nextafter(point_diagonal, math.inf)
    while 2.0 * point_outside**2 <= metric**2:
        point_outside = math.nextafter(point_outside, math.inf)
    assert not similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:circle_at],
                replace(
                    circle,
                    centre=(
                        circle.centre[0] + point_outside,
                        circle.centre[1] + point_outside,
                        circle.centre[2],
                    ),
                ),
                *before.section_curves[circle_at + 1 :],
            ),
        )
    )

    assert line.start is not None and line.start_parameter is not None
    # Search the *represented coordinate deltas*, not adjacent values of the small offset.
    # At a large model coordinate, many millions of offset ULPs can map to the same sum.
    # Maintain a closed inside/outside bracket so this boundary test is both exact and bounded.
    start_diagonal = 0.0
    start_outside = metric
    for _ in range(80):
        candidate = (start_diagonal + start_outside) / 2.0
        distance_squared = (line.start[0] + candidate - line.start[0]) ** 2 + (
            line.start[1] + candidate - line.start[1]
        ) ** 2
        if distance_squared <= metric**2:
            start_diagonal = candidate
        else:
            start_outside = candidate
    start_equality = (
        line.start[0] + start_diagonal,
        line.start[1] + start_diagonal,
        line.start[2],
    )
    start_parameter_equality = (
        line.start_parameter[0] + start_diagonal,
        line.start_parameter[1] + start_diagonal,
    )
    assert similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:line_at],
                replace(
                    line,
                    start=start_equality,
                    start_parameter=start_parameter_equality,
                ),
                *before.section_curves[line_at + 1 :],
            ),
        )
    )
    assert not similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:line_at],
                replace(
                    line,
                    start=(
                        line.start[0] + start_outside,
                        line.start[1] + start_outside,
                        line.start[2],
                    ),
                    start_parameter=(
                        line.start_parameter[0] + start_outside,
                        line.start_parameter[1] + start_outside,
                    ),
                ),
                *before.section_curves[line_at + 1 :],
            ),
        )
    )

    direction_bound = 4.0 * DIRECTION_TOL
    equality_angle = 2.0 * math.asin(direction_bound / 2.0)
    equality_normal = (math.sin(equality_angle), 0.0, math.cos(equality_angle))
    while not _direction_close((0.0, 0.0, 1.0), equality_normal):
        equality_angle = math.nextafter(equality_angle, 0.0)
        equality_normal = (math.sin(equality_angle), 0.0, math.cos(equality_angle))
    direction_curve = replace(
        next(curve for curve in before.section_curves if curve.kind == "CIRCLE"),
        full=True,
        start=None,
        end=None,
        start_parameter=None,
        end_parameter=None,
        sweep=2.0 * math.pi,
    )
    direction_source = replace(before, section_curves=(direction_curve,), side_faces=())
    direction_equality = replace(
        direction_source,
        face=replace(
            direction_source.face,
            parameters=(*equality_normal, *direction_source.face.parameters[3:]),
        ),
    )
    assert _prism_cap_similarity(
        direction_source,
        direction_equality,
        IDENTITY_ROTATION,
        1.0,
        2,
        metric,
        area,
        _MatchBudget(),
    )
    outside_angle = math.nextafter(equality_angle, math.inf)
    outside_normal = (math.sin(outside_angle), 0.0, math.cos(outside_angle))
    while _direction_close((0.0, 0.0, 1.0), outside_normal):
        outside_angle = math.nextafter(outside_angle, math.inf)
        outside_normal = (math.sin(outside_angle), 0.0, math.cos(outside_angle))
    assert not _prism_cap_similarity(
        direction_source,
        replace(
            direction_source,
            face=replace(
                direction_source.face,
                parameters=(*outside_normal, *direction_source.face.parameters[3:]),
            ),
        ),
        IDENTITY_ROTATION,
        1.0,
        2,
        metric,
        area,
        _MatchBudget(),
    )

    assert line.start_parameter is not None
    parameter_bound = 4.0 * metric
    parameter_equality_value = line.start_parameter[0] + parameter_bound
    while abs(parameter_equality_value - line.start_parameter[0]) > parameter_bound:
        parameter_equality_value = math.nextafter(parameter_equality_value, line.start_parameter[0])
    parameter_equality = replace(
        line,
        start_parameter=(parameter_equality_value, line.start_parameter[1]),
    )
    assert similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:line_at],
                parameter_equality,
                *before.section_curves[line_at + 1 :],
            ),
        )
    )
    parameter_outside = math.nextafter(parameter_equality_value, math.inf)
    assert not similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:line_at],
                replace(
                    line,
                    start_parameter=(parameter_outside, line.start_parameter[1]),
                ),
                *before.section_curves[line_at + 1 :],
            ),
        )
    )

    circle_at = next(at for at, curve in enumerate(before.section_curves) if curve.kind == "CIRCLE")
    circle = before.section_curves[circle_at]
    assert circle.sweep is not None
    angle_bound = 4.0 * ANGLE_TOL
    angle_equality_value = circle.sweep + angle_bound
    while abs(angle_equality_value - circle.sweep) > angle_bound:
        angle_equality_value = math.nextafter(angle_equality_value, circle.sweep)
    angle_equality = replace(circle, sweep=angle_equality_value)
    assert similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:circle_at],
                angle_equality,
                *before.section_curves[circle_at + 1 :],
            ),
        )
    )
    angle_outside = math.nextafter(angle_equality_value, math.inf)
    assert not similar(
        replace(
            before,
            section_curves=(
                *before.section_curves[:circle_at],
                replace(circle, sweep=angle_outside),
                *before.section_curves[circle_at + 1 :],
            ),
        )
    )


def test_partition_section_point_euclidean_bound_is_inclusive_and_closed() -> None:
    parent_snapshot = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    child_snapshot = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    parent_occurrence = parent_snapshot.occurrences[0]
    parent = _prism_fact_for(parent_occurrence)
    children = tuple(_prism_fact_for(value) for value in child_snapshot.occurrences)
    assert parent is not None and all(child is not None for child in children)
    typed = tuple(child for child in children if child is not None)
    first = typed[0]
    bound = 2.0 * (parent.quantization.metric_quantum + first.quantization.metric_quantum)
    point = first.section_points[0]
    source_point = min(
        parent.section_points,
        key=lambda candidate: (candidate[0] - point[0]) ** 2 + (candidate[1] - point[1]) ** 2,
    )
    diagonal = bound / math.sqrt(2.0)
    equality_point = (
        source_point[0] + diagonal,
        source_point[1] + diagonal,
        point[2],
    )
    equality_first = replace(
        first,
        section_points=(equality_point, *first.section_points[1:]),
    )
    assert _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        (equality_first, *typed[1:]),
        _MatchBudget(),
    )
    outside = math.nextafter(diagonal, math.inf)
    while (source_point[0] + outside - source_point[0]) ** 2 + (
        source_point[1] + outside - source_point[1]
    ) ** 2 <= bound**2:
        outside = math.nextafter(outside, math.inf)
    outside_first = replace(
        first,
        section_points=(
            (source_point[0] + outside, source_point[1] + outside, point[2]),
            *first.section_points[1:],
        ),
    )
    assert not _partition_witnesses(
        parent_occurrence,
        parent,
        child_snapshot.occurrences,
        (outside_first, *typed[1:]),
        _MatchBudget(),
    )


@pytest.mark.parametrize("diagonal", [False, True])
def test_partition_common_transverse_centre_bound_is_inclusive(diagonal: bool) -> None:
    parent_snapshot = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    child_snapshot = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(5.0), _partition_rrp(5.0, 5.0)]))
    )
    parent_occurrence = parent_snapshot.occurrences[0]
    parent = _prism_fact_for(parent_occurrence)
    children = tuple(_prism_fact_for(value) for value in child_snapshot.occurrences)
    assert parent is not None and all(child is not None for child in children)
    typed = tuple(child for child in children if child is not None)
    bound = 2.0 * (parent.quantization.metric_quantum + typed[0].quantization.metric_quantum)
    component = bound / math.sqrt(2.0) if diagonal else bound
    base = child_snapshot.occurrences[0].summary.centre

    def shifted(amount: float):
        delta = (amount, amount if diagonal else 0.0, 0.0)
        summary = replace(
            child_snapshot.occurrences[0].summary,
            centre=tuple(value + change for value, change in zip(base, delta, strict=True)),
        )
        return (
            replace(child_snapshot.occurrences[0], summary=summary),
            child_snapshot.occurrences[1],
        )

    assert _partition_witnesses(
        parent_occurrence,
        parent,
        shifted(component),
        typed,
        _MatchBudget(),
    )
    outside = math.nextafter(component, math.inf)
    while (2.0 if diagonal else 1.0) * outside**2 <= bound**2:
        outside = math.nextafter(outside, math.inf)
    assert not _partition_witnesses(
        parent_occurrence,
        parent,
        shifted(outside),
        typed,
        _MatchBudget(),
    )


def test_raw_partition_volume_enclosure_is_inclusive_and_nextafter_closed() -> None:
    parent = _raw_prism_partition_oracle(_partition_rrp(10.0))[0]
    children = list(
        _raw_prism_partition_oracle(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    bound = 2.0 * (parent.volume_quantum + sum(child.volume_quantum for child in children))
    common_com = parent.centre_of_mass
    children = [replace(child, centre_of_mass=common_com) for child in children]
    equality_volume = parent.volume + bound - children[1].volume
    equality_children = (replace(children[0], volume=equality_volume), children[1])
    assert _raw_partition_relation_oracle((parent,), equality_children)[0]
    outside_children = (
        replace(children[0], volume=math.nextafter(equality_volume, math.inf)),
        children[1],
    )
    with pytest.raises(AssertionError):
        _raw_partition_relation_oracle((parent,), outside_children)

    parent_snapshot = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    child_snapshot = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    parent_occurrence = parent_snapshot.occurrences[0]
    parent_fact = _prism_fact_for(parent_occurrence)
    child_facts = tuple(_prism_fact_for(value) for value in child_snapshot.occurrences)
    assert parent_fact is not None and all(child is not None for child in child_facts)
    typed_children = tuple(child for child in child_facts if child is not None)
    production_bound = 2.0 * (
        parent_fact.quantization.volume_quantum
        + sum(child.quantization.volume_quantum for child in typed_children)
    )
    production_equality = (
        replace(
            typed_children[0],
            volume=parent_fact.volume + production_bound - typed_children[1].volume,
            centre_of_mass=parent_fact.centre_of_mass,
        ),
        replace(typed_children[1], centre_of_mass=parent_fact.centre_of_mass),
    )
    assert _partition_witnesses(
        parent_occurrence,
        parent_fact,
        child_snapshot.occurrences,
        production_equality,
        _MatchBudget(),
    )
    outside_volume = math.nextafter(production_equality[0].volume, math.inf)
    while (
        abs(parent_fact.volume - (outside_volume + production_equality[1].volume))
        <= production_bound
    ):
        outside_volume = math.nextafter(outside_volume, math.inf)
    production_outside = (
        replace(
            production_equality[0],
            volume=outside_volume,
        ),
        production_equality[1],
    )
    assert not _partition_witnesses(
        parent_occurrence,
        parent_fact,
        child_snapshot.occurrences,
        production_outside,
        _MatchBudget(),
    )

    volume_errors = tuple(2.0 * child.quantization.volume_quantum for child in typed_children)
    child_volume = sum(child.volume for child in typed_children)
    error_sum = sum(volume_errors)
    volume_uppers = tuple(
        abs(child.volume) + error
        for child, error in zip(typed_children, volume_errors, strict=True)
    )
    first_moment_base = (
        sum(
            upper * 2.0 * child.quantization.metric_quantum
            for child, upper in zip(typed_children, volume_uppers, strict=True)
        )
        + sum(volume_uppers) * 2.0 * parent_fact.quantization.metric_quantum
    )
    equality_shift = first_moment_base / (child_volume - error_sum)
    moment_equality = tuple(
        replace(
            child,
            centre_of_mass=(
                parent_fact.centre_of_mass[0] + equality_shift,
                parent_fact.centre_of_mass[1],
                parent_fact.centre_of_mass[2],
            ),
        )
        for child in typed_children
    )
    assert _partition_witnesses(
        parent_occurrence,
        parent_fact,
        child_snapshot.occurrences,
        moment_equality,
        _MatchBudget(),
    )
    outside_shift = math.nextafter(equality_shift, math.inf)
    moment_outside = tuple(
        replace(
            child,
            centre_of_mass=(
                parent_fact.centre_of_mass[0] + outside_shift,
                parent_fact.centre_of_mass[1],
                parent_fact.centre_of_mass[2],
            ),
        )
        for child in typed_children
    )
    assert not _partition_witnesses(
        parent_occurrence,
        parent_fact,
        child_snapshot.occurrences,
        moment_outside,
        _MatchBudget(),
    )

    lower_children = tuple(
        replace(
            child,
            volume=error,
            centre_of_mass=parent_fact.centre_of_mass,
        )
        for child, error in zip(typed_children, volume_errors, strict=True)
    )
    lower_parent = replace(parent_fact, volume=sum(child.volume for child in lower_children))
    assert not _partition_witnesses(
        parent_occurrence,
        lower_parent,
        child_snapshot.occurrences,
        lower_children,
        _MatchBudget(),
    )
    positive_children = (
        replace(
            lower_children[0],
            volume=math.nextafter(lower_children[0].volume, math.inf),
        ),
        lower_children[1],
    )
    positive_parent = replace(parent_fact, volume=sum(child.volume for child in positive_children))
    assert _partition_witnesses(
        parent_occurrence,
        positive_parent,
        child_snapshot.occurrences,
        positive_children,
        _MatchBudget(),
    )


def test_split_merge_result_shapes_and_candidate_witness_roster_are_closed() -> None:
    whole_product = _take_inventory(_partition_rrp(10.0))
    pieces_product = _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    whole = correspondence_snapshot(whole_product)
    pieces = correspondence_snapshot(pieces_product)
    split = correspondence_changes(whole_product, pieces_product).relations[0]
    merge = correspondence_changes(pieces_product, whole_product).relations[0]
    assert split.witness is not None and merge.witness is not None

    malformed = (
        (replace(split, before_refs=()), whole, pieces),
        (replace(split, after_refs=split.after_refs[:1]), whole, pieces),
        (replace(split, candidate_witnesses=(split.witness,)), whole, pieces),
        (replace(merge, before_refs=merge.before_refs[:1]), pieces, whole),
        (replace(merge, after_refs=()), pieces, whole),
        (replace(merge, candidate_witnesses=(merge.witness,)), pieces, whole),
    )
    for relation, before_snapshot, after_snapshot in malformed:
        with pytest.raises(CorrespondenceMatchError, match="split|merge"):
            _validate_result(
                CorrespondenceResult(2, 3, 3, (relation,)),
                before_snapshot,
                after_snapshot,
            )

    symmetric_whole = _take_inventory(_partition_rrp(10.0))
    symmetric_pieces = _take_inventory(
        Compound(
            [
                _partition_rrp(4.0),
                _partition_rrp(4.0),
                _partition_rrp(6.0, 4.0),
            ]
        )
    )
    relation = correspondence_changes(symmetric_whole, symmetric_pieces).relations[0]
    before = correspondence_snapshot(symmetric_whole)
    after = correspondence_snapshot(symmetric_pieces)
    with pytest.raises(CorrespondenceMatchError, match="canonical"):
        _validate_result(
            CorrespondenceResult(
                2,
                3,
                3,
                (
                    replace(
                        relation,
                        candidate_witnesses=(
                            relation.candidate_witnesses[0],
                            relation.candidate_witnesses[0],
                        ),
                    ),
                ),
            ),
            before,
            after,
        )


def test_snapshot_only_leaf_rejects_unsupported_schema() -> None:
    product = _take_inventory(Box(10, 10, 10))
    snapshot = correspondence_snapshot(product)
    with pytest.raises(CorrespondenceMatchError, match="invalid"):
        _compare_snapshots(replace(snapshot, schema_version=1), snapshot)


def test_product_authority_is_required_before_snapshot_matching() -> None:
    product = _take_inventory(_line_rrp(5))
    copied = replace(product)
    with pytest.raises(CorrespondenceMatchError, match="authority"):
        correspondence_changes(copied, product)


def test_one_body_translation_has_one_shared_moved_witness() -> None:
    before = _take_inventory(_asymmetric_rrp())
    after = _take_inventory(Pos(11, -7, 3) * _asymmetric_rrp())
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [ChangeKind.MOVED]
    (relation,) = result.relations
    assert relation.witness is not None
    assert relation.witness.scale == 1.0
    assert relation.witness.translation == pytest.approx((11.0, -7.0, 3.0), abs=1e-6)


def test_uniform_scale_precedes_its_placement_change() -> None:
    before = _take_inventory(_asymmetric_rrp())
    after = _take_inventory((Pos(11, -7, 3) * _asymmetric_rrp()).scale(2.0))
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [ChangeKind.RESIZED]
    (relation,) = result.relations
    assert relation.witness is not None
    assert relation.witness.scale == pytest.approx(2.0, rel=1e-7)


def test_all_24_proper_rotations_produce_the_exact_supported_witness() -> None:
    part = _asymmetric_rrp()
    before = _take_inventory(part)
    for rotation in _proper_signed_permutations():
        relation = correspondence_changes(
            before, _take_inventory(_proper_transform(part, rotation))
        ).relations[0]
        if rotation == IDENTITY_ROTATION:
            assert relation.kind is ChangeKind.UNCHANGED
            assert relation.witness is None
        else:
            assert relation.kind is ChangeKind.MOVED
            assert relation.witness is not None
            assert relation.witness.rotation == rotation
            assert relation.witness.scale == pytest.approx(1.0, rel=1e-12)


def test_proper_rotation_scale_and_translation_share_one_affine_witness() -> None:
    part = _asymmetric_rrp()
    rotation = _proper_signed_permutations()[8]
    transformed = Pos(11, -7, 3) * _proper_transform(part, rotation).scale(2.0)
    relation = correspondence_changes(
        _take_inventory(part), _take_inventory(transformed)
    ).relations[0]
    assert relation.kind is ChangeKind.RESIZED
    assert relation.witness is not None
    assert relation.witness.rotation == rotation
    assert relation.witness.scale == pytest.approx(2.0, rel=1e-7)


def test_symmetric_nonidentity_witnesses_are_one_whole_ambiguity() -> None:
    before = _take_inventory(_line_rrp(5))
    after = _take_inventory(Pos(11, -7, 3) * _line_rrp(5))
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [ChangeKind.AMBIGUOUS]


def test_chiral_mirror_has_no_invented_proper_similarity() -> None:
    part = _chiral_rrp()
    result = correspondence_changes(_take_inventory(part), _take_inventory(part.mirror(Plane.YZ)))
    assert {relation.kind for relation in result.relations} == {
        ChangeKind.ADDED,
        ChangeKind.REMOVED,
    }


def test_representation_preserving_step_uses_identity_precedence(tmp_path) -> None:
    part = _asymmetric_rrp()
    path = tmp_path / "correspondence.step"
    assert export_step(part, path)
    relation = correspondence_changes(
        _take_inventory(part), _take_inventory(import_step(path))
    ).relations[0]
    assert relation.kind is ChangeKind.UNCHANGED
    assert relation.witness is None


def test_independent_unique_and_ambiguous_components_do_not_contaminate() -> None:
    unique = Pos(60, 0, 0) * _asymmetric_rrp()
    symmetric = Pos(-60, 0, 0) * _line_rrp(5)
    before = _take_inventory(Compound([unique, symmetric]))
    after = _take_inventory(Pos(11, -7, 3) * Compound([unique, symmetric]))
    result = correspondence_changes(before, after)
    assert sorted(relation.kind.value for relation in result.relations) == [
        "ambiguous",
        "moved",
    ]
    ambiguous = next(
        relation for relation in result.relations if relation.kind is ChangeKind.AMBIGUOUS
    )
    assert len(ambiguous.before_refs) == len(ambiguous.after_refs) == 1
    assert len(ambiguous.candidate_witnesses) > 1


def test_discrete_repeat_change_is_added_and_removed_not_resized() -> None:
    result = correspondence_changes(_take_inventory(_line_rrp(5)), _take_inventory(_line_rrp(7)))
    assert {relation.kind for relation in result.relations} == {
        ChangeKind.ADDED,
        ChangeKind.REMOVED,
    }


def test_equal_rrp_record_with_different_host_geometry_does_not_match() -> None:
    left = _line_rrp(5) + Pos(18, 0, 5) * Box(
        4, 3, 2, align=(Align.CENTER, Align.CENTER, Align.CENTER)
    )
    right = _line_rrp(5) + Pos(18, 0, 5) * Box(
        7, 2, 3, align=(Align.CENTER, Align.CENTER, Align.CENTER)
    )
    before = correspondence_snapshot(_take_inventory(left))
    after = correspondence_snapshot(_take_inventory(right))
    assert before.occurrences[0].record_value == after.occurrences[0].record_value
    result = correspondence_changes(_take_inventory(left), _take_inventory(right))
    assert {relation.kind for relation in result.relations} == {
        ChangeKind.ADDED,
        ChangeKind.REMOVED,
    }


def test_snapshot_tuple_permutation_changes_only_presentation_refs() -> None:
    before_product = _take_inventory(
        Compound([Pos(-60, 0, 0) * _asymmetric_rrp(), Pos(60, 0, 0) * _chiral_rrp()])
    )
    after_product = _take_inventory(
        Pos(11, -7, 3)
        * Compound([Pos(-60, 0, 0) * _asymmetric_rrp(), Pos(60, 0, 0) * _chiral_rrp()])
    )
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    direct = _compare_snapshots(before, after)
    permuted_before = replace(
        before,
        occurrences=tuple(reversed(before.occurrences)),
        body_groups=tuple(
            sorted((len(before.occurrences) - 1 - group[0],) for group in before.body_groups)
        ),
    )
    permuted_after = replace(
        after,
        occurrences=tuple(reversed(after.occurrences)),
        body_groups=tuple(
            sorted((len(after.occurrences) - 1 - group[0],) for group in after.body_groups)
        ),
    )
    permuted = _compare_snapshots(permuted_before, permuted_after)
    assert [relation.kind for relation in direct.relations] == [
        relation.kind for relation in permuted.relations
    ]
    assert [relation.witness for relation in direct.relations] == [
        relation.witness for relation in permuted.relations
    ]


def test_one_group_cannot_distribute_into_two_equal_target_groups() -> None:
    before = _take_inventory(_line_rrp(5))
    after = _take_inventory(Compound([_line_rrp(5), _line_rrp(5)]))
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [ChangeKind.AMBIGUOUS]
    (relation,) = result.relations
    assert len(relation.before_refs) == 1
    assert len(relation.after_refs) == 2
    assert relation.witness is None


def test_moved_coincident_groups_remain_one_whole_ambiguity_component() -> None:
    before = _take_inventory(Compound([_line_rrp(5), _line_rrp(5)]))
    after = _take_inventory(Pos(11, -7, 3) * Compound([_line_rrp(5), _line_rrp(5)]))
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [ChangeKind.AMBIGUOUS]
    (relation,) = result.relations
    assert len(relation.before_refs) == len(relation.after_refs) == 2


def test_two_occurrences_on_one_body_share_one_group_witness() -> None:
    before = _take_inventory(_two_rrp_one_solid())
    after = _take_inventory(Pos(11, -7, 3) * _two_rrp_one_solid())
    result = correspondence_changes(before, after)
    assert [relation.kind for relation in result.relations] == [
        ChangeKind.MOVED,
        ChangeKind.MOVED,
    ]
    first, second = result.relations
    assert first.witness == second.witness
    assert first.witness is not None
    assert first.witness.translation == pytest.approx((11.0, -7.0, 3.0), abs=1e-6)


def test_two_occurrences_on_one_body_share_one_rotation_witness() -> None:
    part = _two_rrp_one_solid()
    rotation = _proper_signed_permutations()[9]
    result = correspondence_changes(
        _take_inventory(part), _take_inventory(_proper_transform(part, rotation))
    )
    assert [relation.kind for relation in result.relations] == [
        ChangeKind.MOVED,
        ChangeKind.MOVED,
    ]
    assert result.relations[0].witness == result.relations[1].witness
    assert result.relations[0].witness is not None
    assert result.relations[0].witness.rotation == rotation


def test_one_body_group_cannot_split_across_two_target_groups() -> None:
    before_product = _take_inventory(_two_rrp_one_solid())
    after_product = _take_inventory(Pos(11, -7, 3) * _two_rrp_one_solid())
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    assert before.body_groups == after.body_groups == ((0, 1),)
    split_after = replace(after, body_groups=((0,), (1,)))
    forward = _compare_snapshots(before, split_after)
    inverse = _compare_snapshots(split_after, before)
    assert [relation.kind for relation in forward.relations] == [ChangeKind.AMBIGUOUS]
    assert [relation.kind for relation in inverse.relations] == [ChangeKind.AMBIGUOUS]
    assert len(forward.relations[0].before_refs) == 2
    assert len(forward.relations[0].after_refs) == 2


def test_unequal_weight_body_group_alternative_is_wholly_ambiguous() -> None:
    before_product = _take_inventory(_two_rrp_one_solid())
    after_product = _take_inventory(Pos(11, -7, 3) * _two_rrp_one_solid())
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    expanded_after = replace(
        after,
        occurrences=(*after.occurrences, after.occurrences[0]),
        body_groups=((0, 1), (2,)),
    )
    forward = _compare_snapshots(before, expanded_after)
    inverse = _compare_snapshots(expanded_after, before)
    assert [relation.kind for relation in forward.relations] == [ChangeKind.AMBIGUOUS]
    assert [relation.kind for relation in inverse.relations] == [ChangeKind.AMBIGUOUS]
    assert len(forward.relations[0].before_refs) == 2
    assert len(forward.relations[0].after_refs) == 3


@pytest.mark.parametrize("scale", (1.0, 2.0))
def test_swapping_products_inverts_the_identity_rotation_witness(scale: float) -> None:
    before = _take_inventory(_asymmetric_rrp())
    transformed = (Pos(11, -7, 3) * _asymmetric_rrp()).scale(scale)
    after = _take_inventory(transformed)
    forward = correspondence_changes(before, after).relations[0]
    backward = correspondence_changes(after, before).relations[0]
    assert forward.kind is backward.kind
    assert forward.witness is not None and backward.witness is not None
    assert backward.witness.scale == pytest.approx(1.0 / forward.witness.scale, rel=1e-9)
    assert backward.witness.translation == pytest.approx(
        tuple(-value / forward.witness.scale for value in forward.witness.translation),
        abs=1e-6,
    )


def test_swapping_a_rotated_resize_uses_the_exact_inverse_witness() -> None:
    part = _asymmetric_rrp()
    rotation = _proper_signed_permutations()[8]
    transformed = Pos(11, -7, 3) * _proper_transform(part, rotation).scale(2.0)
    forward = correspondence_changes(_take_inventory(part), _take_inventory(transformed)).relations[
        0
    ]
    backward = correspondence_changes(
        _take_inventory(transformed), _take_inventory(part)
    ).relations[0]
    assert forward.kind is backward.kind is ChangeKind.RESIZED
    assert forward.witness is not None and backward.witness is not None
    assert backward.witness.rotation == _inverse_witness(forward.witness).rotation
    assert backward.witness.scale == pytest.approx(0.5, rel=1e-9)
    assert backward.witness.translation == pytest.approx(
        _inverse_witness(forward.witness).translation, abs=1e-6
    )


def test_hypothesis_budget_is_inclusive_and_never_truncates(monkeypatch) -> None:
    import quiddity._correspondence_match as module

    edges = {0: (0, 1), 1: (0, 1)}
    monkeypatch.setattr(module, "MATCH_HYPOTHESIS_BUDGET", 7)
    assert _maximum_matchings(2, 2, edges) == (
        ((0, 0), (1, 1)),
        ((0, 1), (1, 0)),
    )
    monkeypatch.setattr(module, "MATCH_HYPOTHESIS_BUDGET", 6)
    with pytest.raises(CorrespondenceMatchError, match="budget"):
        _maximum_matchings(2, 2, edges)


def test_late_global_budget_refusal_returns_no_prefix_or_input_mutation(monkeypatch) -> None:
    import quiddity._correspondence_match as module

    before_product = _take_inventory(_asymmetric_rrp())
    after_product = _take_inventory(Pos(11, -7, 3) * _asymmetric_rrp())
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    monkeypatch.setattr(module, "MATCH_HYPOTHESIS_BUDGET", 1)
    with pytest.raises(CorrespondenceMatchError, match="budget"):
        correspondence_changes(before_product, after_product)
    assert correspondence_snapshot(before_product) == before
    assert correspondence_snapshot(after_product) == after


def test_partition_budget_refusal_is_atomic_after_both_snapshots(monkeypatch) -> None:
    import quiddity._correspondence_match as module

    before_product = _take_inventory(_partition_rrp(10.0))
    after_product = _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    monkeypatch.setattr(module, "MATCH_HYPOTHESIS_BUDGET", 1)
    with pytest.raises(CorrespondenceMatchError, match="budget"):
        correspondence_changes(before_product, after_product)
    assert correspondence_snapshot(before_product) == before
    assert correspondence_snapshot(after_product) == after


def test_partition_production_search_hits_inclusive_100000_boundary(monkeypatch) -> None:
    import quiddity._correspondence_match as module

    before = correspondence_snapshot(_take_inventory(_partition_rrp(10.0)))
    after = correspondence_snapshot(
        _take_inventory(Compound([_partition_rrp(4.0), _partition_rrp(6.0, 4.0)]))
    )
    original_budget = _MatchBudget
    observed = []
    original_charge = original_budget.charge

    def tracked_charge(self):
        original_charge(self)
        observed.append(self.attempts)

    monkeypatch.setattr(original_budget, "charge", tracked_charge)
    assert _compare_snapshots(before, after).relations[0].kind is ChangeKind.SPLIT
    attempts = max(observed)
    monkeypatch.setattr(original_budget, "charge", original_charge)

    monkeypatch.setattr(
        module,
        "_MatchBudget",
        lambda: original_budget(100_000 - attempts),
    )
    assert _compare_snapshots(before, after).relations[0].kind is ChangeKind.SPLIT
    monkeypatch.setattr(
        module,
        "_MatchBudget",
        lambda: original_budget(100_001 - attempts),
    )
    with pytest.raises(CorrespondenceMatchError, match="budget"):
        _compare_snapshots(before, after)


def test_reciprocal_scale_identity_boundary_is_inclusive_and_swap_stable() -> None:
    from quiddity._correspondence_match import SCALE_TOL

    upper = 1.0 + SCALE_TOL
    lower = 1.0 / upper
    assert _scale_is_identity(upper)
    assert _scale_is_identity(lower)
    assert not _scale_is_identity(float("nan"))
    assert not _scale_is_identity(0.0)
    assert not _scale_is_identity(float("inf"))
    assert not _scale_is_identity(math.nextafter(upper, math.inf))
    assert not _scale_is_identity(math.nextafter(lower, 0.0))


def test_complete_similarity_numeric_bounds_are_inclusive_and_nextafter_closed() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_asymmetric_rrp())).occurrences[0]
    quantization = occurrence.body.quantization
    metric = _order_bound(quantization, quantization, 1.0, 1)
    area = _order_bound(quantization, quantization, 1.0, 2)
    volume = _order_bound(quantization, quantization, 1.0, 3)
    moment = _order_bound(quantization, quantization, 1.0, 5)
    assert _direction_close((0.0, 0.0, 0.0), (4.0 * DIRECTION_TOL, 0.0, 0.0))
    assert not _direction_close(
        (0.0, 0.0, 0.0),
        (math.nextafter(4.0 * DIRECTION_TOL, math.inf), 0.0, 0.0),
    )
    diagonal = 4.0 * DIRECTION_TOL / math.sqrt(2.0)
    assert _direction_close((0.0, 0.0, 0.0), (diagonal, diagonal, 0.0))
    outside_diagonal = math.nextafter(math.nextafter(diagonal, math.inf), math.inf)
    assert not _direction_close(
        (0.0, 0.0, 0.0),
        (outside_diagonal, outside_diagonal, 0.0),
    )

    line_index = next(
        index
        for index, curve in enumerate(occurrence.matching_boundary.curves)
        if curve.kind == "LINE"
    )
    line = occurrence.matching_boundary.curves[line_index]
    vertex_map = tuple(range(len(occurrence.matching_boundary.vertices)))
    assert (
        _curve_similarity(
            line,
            replace(line, length=line.length + metric),
            vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is not None
    )
    assert (
        _curve_similarity(
            line,
            replace(line, length=math.nextafter(line.length + metric, math.inf)),
            vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is None
    )

    curved_occurrence = correspondence_snapshot(_take_inventory(_rrp(5))).occurrences[0]
    curved_vertex_map = tuple(range(len(curved_occurrence.matching_boundary.vertices)))
    circle = next(
        curve
        for curve in curved_occurrence.matching_boundary.curves
        if curve.kind == "CIRCLE" and not curve.full
    )
    assert circle.sweep is not None
    sweep_inside = math.nextafter(circle.sweep + 4.0 * ANGLE_TOL, circle.sweep)
    assert (
        _curve_similarity(
            circle,
            replace(circle, sweep=sweep_inside),
            curved_vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is not None
    )
    assert (
        _curve_similarity(
            circle,
            replace(
                circle,
                sweep=math.nextafter(circle.sweep + 4.0 * ANGLE_TOL, math.inf),
            ),
            curved_vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is None
    )

    face = occurrence.matching_boundary.faces[0]
    assert (
        _face_similarity(
            face,
            replace(face, area=face.area + area),
            IDENTITY_ROTATION,
            1.0,
            metric,
            area,
        )
        is not None
    )
    assert (
        _face_similarity(
            face,
            replace(face, area=math.nextafter(face.area + area, math.inf)),
            IDENTITY_ROTATION,
            1.0,
            metric,
            area,
        )
        is None
    )

    for field, bound in (("volume", volume), ("surface_area", area)):
        intrinsic = occurrence.body.intrinsic
        changed = replace(intrinsic, **{field: getattr(intrinsic, field) + bound})
        target = replace(occurrence, body=replace(occurrence.body, intrinsic=changed))
        assert _body_similarity(
            occurrence,
            target,
            IDENTITY_ROTATION,
            1.0,
            _MatchBudget(),
        )
        outside = replace(
            intrinsic,
            **{field: math.nextafter(getattr(intrinsic, field) + bound, math.inf)},
        )
        assert not _body_similarity(
            occurrence,
            replace(occurrence, body=replace(occurrence.body, intrinsic=outside)),
            IDENTITY_ROTATION,
            1.0,
            _MatchBudget(),
        )

    intrinsic = occurrence.body.intrinsic
    moments = list(intrinsic.principal_moments)
    moments[0] += moment
    assert _body_similarity(
        occurrence,
        replace(
            occurrence,
            body=replace(
                occurrence.body,
                intrinsic=replace(intrinsic, principal_moments=tuple(moments)),
            ),
        ),
        IDENTITY_ROTATION,
        1.0,
        _MatchBudget(),
    )
    moments[0] = math.nextafter(moments[0], math.inf)
    assert not _body_similarity(
        occurrence,
        replace(
            occurrence,
            body=replace(
                occurrence.body,
                intrinsic=replace(intrinsic, principal_moments=tuple(moments)),
            ),
        ),
        IDENTITY_ROTATION,
        1.0,
        _MatchBudget(),
    )


def test_wire_alignment_enumerates_reversed_whole_wire_presentation() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_asymmetric_rrp())).occurrences[0]
    graph = occurrence.matching_boundary
    face = next(face for face in graph.faces if face.kind == "PLANE" and face.wires)
    wire = face.wires[0]
    reversed_wire = replace(
        wire,
        cycle=tuple(
            replace(edge, start=edge.end, end=edge.start, direction=-edge.direction)
            for edge in reversed(wire.cycle)
        ),
        theta_winding=-wire.theta_winding,
    )
    alignments = _wire_alignments(
        wire,
        reversed_wire,
        face,
        replace(face, wires=(reversed_wire,)),
        tuple(range(len(graph.vertices))),
        tuple(range(len(graph.curves))),
        tuple(1 for _curve in graph.curves),
        graph.vertices,
        2,
        _order_bound(
            occurrence.body.quantization,
            occurrence.body.quantization,
            1.0,
            1,
        ),
        _MatchBudget(),
    )
    assert alignments


def test_proper_rotation_roster_and_affine_inverse_are_exact() -> None:
    assert len(PROPER_ROTATIONS) == 24
    assert len(set(PROPER_ROTATIONS)) == 24
    assert tuple(sorted(PROPER_ROTATIONS)) == PROPER_ROTATIONS
    point = (2.5, -3.0, 7.25)
    for rotation in PROPER_ROTATIONS:
        assert _determinant(rotation) == 1
        witness = RigidScaleWitness(rotation, (11.0, -7.0, 3.0), 2.0)
        transformed = _affine_point(
            witness.rotation,
            witness.translation,
            witness.scale,
            point,
        )
        inverse = _inverse_witness(witness)
        assert _affine_point(
            inverse.rotation,
            inverse.translation,
            inverse.scale,
            transformed,
        ) == pytest.approx(point, abs=1e-12)


@pytest.mark.parametrize(
    "witness",
    (
        RigidScaleWitness(((1, 0, 0), (0, -1, 0), (0, 0, 1)), (0.0, 0.0, 0.0), 1.0),
        RigidScaleWitness(IDENTITY_ROTATION, (0.0, float("nan"), 0.0), 1.0),
        RigidScaleWitness(IDENTITY_ROTATION, (0.0, 0.0, 0.0), 0.0),
    ),
)
def test_closed_result_validation_refuses_malformed_witnesses(
    witness: RigidScaleWitness,
) -> None:
    before_product = _take_inventory(_asymmetric_rrp())
    after_product = _take_inventory(Pos(2, 0, 0) * _asymmetric_rrp())
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    relation = correspondence_changes(before_product, after_product).relations[0]
    malformed = CorrespondenceResult(2, 3, 3, (replace(relation, witness=witness),))
    with pytest.raises(CorrespondenceMatchError, match="witness"):
        _validate_result(malformed, before, after)


def test_closed_result_validation_refuses_kind_shape_drift() -> None:
    before_product = _take_inventory(_asymmetric_rrp())
    after_product = _take_inventory(Pos(2, 0, 0) * _asymmetric_rrp())
    before = correspondence_snapshot(before_product)
    after = correspondence_snapshot(after_product)
    moved = correspondence_changes(before_product, after_product).relations[0]
    malformed = CorrespondenceResult(
        2,
        3,
        3,
        (
            CorrespondenceRelation(
                ChangeKind.ADDED,
                moved.before_refs,
                moved.after_refs,
                moved.witness,
                (moved.witness,) if moved.witness is not None else (),
            ),
        ),
    )
    with pytest.raises(CorrespondenceMatchError, match="added"):
        _validate_result(malformed, before, after)


def test_closed_reference_and_result_schema_validation_matrix() -> None:
    product = _take_inventory(_asymmetric_rrp())
    snapshot = correspondence_snapshot(product)
    unchanged = correspondence_changes(product, _take_inventory(_asymmetric_rrp())).relations[0]

    for side, position, message in (
        ("wrong", 0, "malformed"),
        ("before", True, "malformed"),
        ("before", -1, "out of range"),
        ("before", len(snapshot.occurrences), "out of range"),
    ):
        with pytest.raises(CorrespondenceMatchError, match=message):
            correspondence_match_module._ref(side, position, snapshot)

    valid_ref = unchanged.before_refs[0]
    stale_ref = replace(valid_ref, occurrence=replace(valid_ref.occurrence, family="changed"))
    with pytest.raises(CorrespondenceMatchError, match="reference changed"):
        correspondence_match_module._validate_ref(stale_ref, snapshot)

    malformed_results = (
        (object(), "schema"),
        (replace(CorrespondenceResult(2, 3, 3, ()), schema_version=1), "schema"),
        (CorrespondenceResult(2, 3, 3, (object(),)), "relation"),
        (
            CorrespondenceResult(
                2,
                3,
                3,
                (replace(unchanged, before_refs=(replace(valid_ref, side="after"),)),),
            ),
            "wrong-side",
        ),
        (
            CorrespondenceResult(
                2,
                3,
                3,
                (
                    replace(
                        unchanged,
                        after_refs=(replace(unchanged.after_refs[0], side="before"),),
                    ),
                ),
            ),
            "wrong-side",
        ),
        (CorrespondenceResult(2, 3, 3, (replace(unchanged, kind=ChangeKind.ADDED),)), "added"),
        (CorrespondenceResult(2, 3, 3, (replace(unchanged, kind=ChangeKind.REMOVED),)), "removed"),
        (
            CorrespondenceResult(
                2,
                3,
                3,
                (replace(unchanged, before_refs=(), kind=ChangeKind.UNCHANGED),),
            ),
            "unchanged",
        ),
        (
            CorrespondenceResult(2, 3, 3, (replace(unchanged, kind=ChangeKind.MOVED),)),
            "transformed",
        ),
        (
            CorrespondenceResult(
                2,
                3,
                3,
                (
                    replace(
                        unchanged,
                        kind=ChangeKind.AMBIGUOUS,
                        witness=RigidScaleWitness(IDENTITY_ROTATION, (0.0, 0.0, 0.0), 1.0),
                    ),
                ),
            ),
            "ambiguous",
        ),
        (CorrespondenceResult(2, 3, 3, ()), "cover"),
    )
    for result, message in malformed_results:
        with pytest.raises(CorrespondenceMatchError, match=message):
            _validate_result(result, snapshot, snapshot)


def test_closed_bijection_search_covers_empty_unique_and_competing_assignments() -> None:
    assert correspondence_match_module._unique_bijection(((),)) is None
    assert correspondence_match_module._unique_bijection(((0,), (1,))) == (0, 1)
    assert correspondence_match_module._unique_bijection(((0, 1), (0, 1))) is None
    assert not correspondence_match_module._has_bijection(((),))
    assert correspondence_match_module._has_bijection(((0,), (1,)))
    assert not correspondence_match_module._has_bijection(((0,), (0,)))


def test_defining_face_and_rrp_signature_refusal_matrix() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_asymmetric_rrp())).occurrences[0]
    quantization = occurrence.body.quantization
    plane = occurrence.summary.defining[0]
    assert correspondence_match_module._defining_face_similarity(
        plane, plane, IDENTITY_ROTATION, 1.0, quantization, quantization
    )
    assert not correspondence_match_module._defining_face_similarity(
        plane,
        replace(plane, parameters=plane.parameters[:-1]),
        IDENTITY_ROTATION,
        1.0,
        quantization,
        quantization,
    )
    assert not correspondence_match_module._defining_face_similarity(
        plane,
        replace(plane, parameters=(0.0, 1.0, 0.0, *plane.parameters[3:])),
        IDENTITY_ROTATION,
        1.0,
        quantization,
        quantization,
    )
    assert not correspondence_match_module._defining_face_similarity(
        plane,
        replace(plane, parameters=(*plane.parameters[:3], plane.parameters[3] + 1.0)),
        IDENTITY_ROTATION,
        1.0,
        quantization,
        quantization,
    )

    cylinder = replace(
        plane,
        kind="CYLINDER",
        parameters=(*plane.parameters[:3], 0.0, 0.0, 0.0, 2.0),
    )
    assert correspondence_match_module._defining_face_similarity(
        cylinder, cylinder, IDENTITY_ROTATION, 1.0, quantization, quantization
    )
    for changed in (
        replace(cylinder, parameters=(*cylinder.parameters[:3], 1.0, 0.0, 0.0, 2.0)),
        replace(cylinder, parameters=(*cylinder.parameters[:6], 3.0)),
        replace(cylinder, material_side=-cylinder.material_side),
        replace(cylinder, kind="SPHERE"),
    ):
        assert not correspondence_match_module._defining_face_similarity(
            cylinder, changed, IDENTITY_ROTATION, 1.0, quantization, quantization
        )

    signature = occurrence.summary.sector_signature
    metric = correspondence_match_module._order_bound(quantization, quantization, 1.0, 1)
    assert correspondence_match_module._signature_scaled(signature, signature, 1.0, metric)
    for malformed in (
        object(),
        signature[:-1],
        (("LINE",),),
        ((signature[0][0], signature[0][1], object()),),
        ((signature[0][0], signature[0][1], (*signature[0][2], (1.0, 2.0))),),
    ):
        assert not correspondence_match_module._signature_scaled(signature, malformed, 1.0, metric)


def test_occurrence_similarity_refuses_each_closed_rrp_authority_mismatch() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_asymmetric_rrp())).occurrences[0]

    def witness(target):
        return correspondence_match_module._similarity_witness(
            occurrence, target, IDENTITY_ROTATION, _MatchBudget()
        )

    assert witness(occurrence) is not None
    intrinsic = occurrence.body.intrinsic
    quantization = occurrence.body.quantization
    summary = occurrence.summary
    mismatches = (
        replace(occurrence, family="OTHER"),
        replace(occurrence, record_type="Other"),
        replace(occurrence, summary=replace(summary, repeat_count=summary.repeat_count + 1)),
        replace(occurrence, summary=replace(summary, edge_count=summary.edge_count + 1)),
        replace(
            occurrence,
            body=replace(occurrence.body, intrinsic=replace(intrinsic, volume=0.0)),
        ),
        replace(
            occurrence,
            body=replace(
                occurrence.body,
                quantization=replace(quantization, characteristic_scale=float("nan")),
            ),
        ),
        replace(occurrence, summary=replace(summary, sector_signature=())),
        replace(occurrence, summary=replace(summary, centre=(999.0, 999.0, 999.0))),
        replace(occurrence, summary=replace(summary, axis="y" if summary.axis != "y" else "x")),
        replace(occurrence, summary=replace(summary, span=(999.0, 1000.0))),
        replace(
            occurrence,
            summary=replace(
                summary,
                defining=(
                    replace(summary.defining[0], kind="SPHERE"),
                    summary.defining[1],
                ),
            ),
        ),
    )
    assert all(witness(target) is None for target in mismatches)


def test_body_graph_similarity_refuses_each_complete_label_and_topology_mutation() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_asymmetric_rrp())).occurrences[0]
    graph = occurrence.matching_boundary

    def refuses(changed_graph) -> bool:
        target = replace(occurrence, matching_boundary=changed_graph)
        return not _body_similarity(occurrence, target, IDENTITY_ROTATION, 1.0, _MatchBudget())

    assert _body_similarity(occurrence, occurrence, IDENTITY_ROTATION, 1.0, _MatchBudget())
    assert refuses(replace(graph, face_count=graph.face_count + 1))
    assert refuses(replace(graph, wire_count=graph.wire_count + 1))
    assert refuses(
        replace(
            graph,
            vertices=(
                (graph.vertices[0][0] + 1000.0, *graph.vertices[0][1:]),
                *graph.vertices[1:],
            ),
        )
    )

    line_at = next(index for index, curve in enumerate(graph.curves) if curve.kind == "LINE")
    line = graph.curves[line_at]
    curve_mutations = (
        (line_at, replace(line, kind="CIRCLE")),
        (line_at, replace(line, length=line.length + 1000.0)),
        (line_at, replace(line, vertices=None)),
    )
    for curve_at, changed in curve_mutations:
        curves = list(graph.curves)
        curves[curve_at] = changed
        assert refuses(replace(graph, curves=tuple(curves)))

    face_at = next(index for index, face in enumerate(graph.faces) if face.wires)
    face = graph.faces[face_at]
    wire = face.wires[0]
    face_mutations = (
        replace(face, kind="CYLINDER" if face.kind == "PLANE" else "PLANE"),
        replace(face, area=face.area + 1000.0),
        replace(face, centroid=(999.0, 999.0, 999.0)),
        replace(face, material_side=-face.material_side),
        replace(face, wires=()),
        replace(face, wires=(replace(wire, role="changed"), *face.wires[1:])),
        replace(
            face,
            wires=(replace(wire, theta_winding=wire.theta_winding + 7), *face.wires[1:]),
        ),
        replace(
            face,
            wires=(
                replace(
                    wire,
                    cycle=(replace(wire.cycle[0], curve=len(graph.curves)), *wire.cycle[1:]),
                ),
                *face.wires[1:],
            ),
        ),
    )
    for changed in face_mutations:
        faces = list(graph.faces)
        faces[face_at] = changed
        assert refuses(replace(graph, faces=tuple(faces)))

    assert refuses(replace(graph, incidence=graph.incidence[:-1]))


def test_curve_similarity_refuses_every_analytic_circle_field_mismatch() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_rrp(5))).occurrences[0]
    graph = occurrence.matching_boundary
    circle = next(curve for curve in graph.curves if curve.kind == "CIRCLE" and not curve.full)
    metric = _order_bound(occurrence.body.quantization, occurrence.body.quantization, 1.0, 1)
    vertex_map = tuple(range(len(graph.vertices)))

    def similarity(target):
        return _curve_similarity(circle, target, vertex_map, IDENTITY_ROTATION, 1.0, metric)

    assert similarity(circle) is not None
    mutations = (
        replace(circle, kind="LINE"),
        replace(circle, centre=None),
        replace(circle, centre=(999.0, 999.0, 999.0)),
        replace(circle, axis=None),
        replace(circle, axis=(1.0, 0.0, 0.0)),
        replace(circle, radius=None),
        replace(circle, radius=(circle.radius or 0.0) + 1000.0),
        replace(circle, sweep=None),
        replace(circle, sweep=(circle.sweep or 0.0) + 1.0),
        replace(circle, full=True),
    )
    assert all(similarity(target) is None for target in mutations)

    line = next(curve for curve in graph.curves if curve.kind == "LINE")
    assert (
        _curve_similarity(
            line,
            replace(line, kind="CIRCLE"),
            vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is None
    )

    full = replace(circle, vertices=None, sweep=2.0 * math.pi, full=True)
    assert _curve_similarity(full, full, vertex_map, IDENTITY_ROTATION, 1.0, metric) is not None
    assert (
        _curve_similarity(
            full,
            replace(full, sweep=0.0),
            vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is None
    )
    assert (
        _curve_similarity(
            full,
            replace(full, vertices=(0, 1)),
            vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is None
    )
    assert (
        _curve_similarity(
            line,
            replace(line, vertices=None),
            vertex_map,
            IDENTITY_ROTATION,
            1.0,
            metric,
        )
        is None
    )


def test_matching_face_parameter_and_wire_alignment_refusal_matrix() -> None:
    occurrence = correspondence_snapshot(_take_inventory(_rrp(5))).occurrences[0]
    graph = occurrence.matching_boundary
    metric = _order_bound(occurrence.body.quantization, occurrence.body.quantization, 1.0, 1)
    area = _order_bound(occurrence.body.quantization, occurrence.body.quantization, 1.0, 2)
    plane = next(face for face in graph.faces if face.kind == "PLANE" and face.wires)
    cylinder = next(face for face in graph.faces if face.kind == "CYLINDER" and face.wires)
    assert _face_similarity(plane, plane, IDENTITY_ROTATION, 1.0, metric, area)
    assert _face_similarity(cylinder, cylinder, IDENTITY_ROTATION, 1.0, metric, area)
    for source, changed in (
        (plane, replace(plane, parameters=(0.0, 1.0, 0.0, *plane.parameters[3:]))),
        (plane, replace(plane, parameters=(*plane.parameters[:3], plane.parameters[3] + 1.0))),
        (plane, replace(plane, material_side=-plane.material_side)),
        (
            cylinder,
            replace(
                cylinder,
                parameters=(*cylinder.parameters[:3], 999.0, 999.0, 999.0, cylinder.parameters[6]),
            ),
        ),
        (
            cylinder,
            replace(
                cylinder,
                parameters=(*cylinder.parameters[:6], cylinder.parameters[6] + 1000.0),
            ),
        ),
        (cylinder, replace(cylinder, material_side=-cylinder.material_side)),
        (plane, replace(plane, kind="SPHERE")),
    ):
        assert _face_similarity(source, changed, IDENTITY_ROTATION, 1.0, metric, area) is None

    def first_vertex(face):
        edge = next(item for wire in face.wires for item in wire.cycle if item.start is not None)
        assert edge.start is not None and edge.start.vertex is not None
        return graph.vertices[edge.start.vertex], edge.start.parameter

    vertex, parameter = first_vertex(plane)
    assert correspondence_match_module._parameter_matches(vertex, parameter, plane, metric)
    assert not correspondence_match_module._parameter_matches(
        vertex, (parameter[0] + 1.0, parameter[1] + 1.0), plane, metric
    )
    theta, z = first_vertex(cylinder)[1]
    axis = cylinder.parameters[:3]
    axis_point = cylinder.parameters[3:6]
    radius = cylinder.parameters[6]
    u, v = correspondence_match_module._plane_basis(axis)
    radial = tuple(
        radius * (math.cos(theta) * left + math.sin(theta) * right)
        for left, right in zip(u, v, strict=True)
    )
    cylinder_vertex = tuple(
        origin + radial_component + z * axis_component
        for origin, radial_component, axis_component in zip(axis_point, radial, axis, strict=True)
    )
    assert correspondence_match_module._parameter_matches(
        cylinder_vertex, (theta, z), cylinder, metric
    )
    assert not correspondence_match_module._parameter_matches(
        cylinder_vertex, (theta + 1.0, z + 1.0), cylinder, metric
    )

    vertex_map = tuple(range(len(graph.vertices)))
    curve_map = tuple(range(len(graph.curves)))
    curve_signs = tuple(1 for _curve in graph.curves)

    def align(source, target, source_face=plane, target_face=plane):
        return _wire_alignments(
            source,
            target,
            source_face,
            target_face,
            vertex_map,
            curve_map,
            curve_signs,
            graph.vertices,
            1,
            metric,
            _MatchBudget(),
        )

    wire = plane.wires[0]
    assert not align(wire, replace(wire, role="inner" if wire.role == "outer" else "outer"))
    assert not align(wire, replace(wire, cycle=wire.cycle[:-1]))
    first = wire.cycle[0]
    assert first.start is not None and first.end is not None
    assert not align(
        wire,
        replace(wire, cycle=(replace(first, start=None, end=None), *wire.cycle[1:])),
    )
    assert not align(
        wire,
        replace(
            wire,
            cycle=(
                replace(first, start=replace(first.start, vertex=None)),
                *wire.cycle[1:],
            ),
        ),
    )
    assert not align(
        wire,
        replace(
            wire,
            cycle=(
                replace(
                    first,
                    start=replace(first.start, vertex=(first.start.vertex or 0) + 1),
                ),
                *wire.cycle[1:],
            ),
        ),
    )
    assert not align(
        wire,
        replace(
            wire,
            cycle=(
                replace(
                    first,
                    start=replace(first.start, parameter=(999.0, 999.0)),
                ),
                *wire.cycle[1:],
            ),
        ),
    )

    endpoint_free_wire = replace(
        wire,
        cycle=(replace(first, start=None, end=None), *wire.cycle[1:]),
    )
    assert not align(
        endpoint_free_wire,
        replace(
            endpoint_free_wire,
            cycle=(first, *endpoint_free_wire.cycle[1:]),
        ),
    )


def test_degenerate_gauges_search_budget_and_schema_gate_refuse_closed_inputs(
    monkeypatch,
) -> None:
    with pytest.raises(CorrespondenceMatchError, match="axis is degenerate"):
        correspondence_match_module._canonical_axis((0.0, 0.0, 0.0))
    monkeypatch.setattr(correspondence_match_module, "DIRECTION_TOL", 2.0)
    with pytest.raises(CorrespondenceMatchError, match="basis is degenerate"):
        correspondence_match_module._plane_basis((1.0, 0.0, 0.0))

    budget = _MatchBudget()
    assert correspondence_match_module._enumerate_bijections(((0,), (0,)), budget) == ()
    assert budget.attempts
    assert correspondence_match_module._maximum_weight_matchings(
        (0, 1), (0,), {0: (0, 1), 1: (0,)}, {(0, 0): 1, (1, 0): 1}, _MatchBudget()
    )

    snapshot = correspondence_snapshot(_take_inventory(Box(10, 10, 10)))
    with pytest.raises(CorrespondenceMatchError, match="schema 3"):
        _compare_snapshots(replace(snapshot, schema_version=1), snapshot, _issuer_validated=True)


def test_matcher_dependency_and_policy_rosters_are_closed() -> None:
    from pathlib import Path

    path = Path(__file__).parents[1] / "src" / "quiddity" / "_correspondence_match.py"
    tree = ast.parse(path.read_text(encoding="utf-8"))
    imports = {
        node.module: {alias.name for alias in node.names}
        for node in ast.walk(tree)
        if isinstance(node, ast.ImportFrom)
        and node.module
        in {
            "quiddity._body_geometry",
            "quiddity._correspondence",
        }
    }
    assert set(imports) == {
        "quiddity._body_geometry",
        "quiddity._correspondence",
    }
    assert imports["quiddity._body_geometry"] == {
        "ANGLE_TOL",
        "DESCRIPTOR_REL",
        "DIRECTION_TOL",
        "DescriptorQuantization",
        "FaceGeometry",
        "MatchingBoundaryGraph",
        "MatchingCurve",
        "MatchingFace",
        "MatchingWire",
    }
    assert imports["quiddity._correspondence"] == {
        "AcceptedOccurrenceSnapshot",
        "CorrespondenceSnapshot",
        "CorrespondenceSnapshotError",
        "_InventoryProduct",
        "_validate_snapshot",
        "correspondence_snapshot",
    }
    names = {node.id for node in ast.walk(tree) if isinstance(node, ast.Name)}
    assert (
        not {
            "Candidate",
            "EvidenceIndex",
            "FaceGraph",
            "SolidRef",
            "RecognitionResult",
            "ClaimLedger",
            "hash",
            "digest",
        }
        & names
    )
    trusted_calls = [
        node
        for node in ast.walk(tree)
        if isinstance(node, ast.Call)
        and isinstance(node.func, ast.Name)
        and node.func.id == "_compare_snapshots"
        and any(
            keyword.arg == "_issuer_validated"
            and isinstance(keyword.value, ast.Constant)
            and keyword.value.value is True
            for keyword in node.keywords
        )
    ]
    assert len(trusted_calls) == 1
    owner = next(
        node
        for node in ast.walk(tree)
        if isinstance(node, ast.FunctionDef) and trusted_calls[0] in tuple(ast.walk(node))
    )
    assert owner.name == "correspondence_changes"
    assert "correspondence_changes" not in __import__("quiddity").__all__
