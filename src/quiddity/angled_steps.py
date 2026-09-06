# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Angled blind step recognition for prismatic parts.

An **angled blind step** is a wedge taken out of an edge of the part: one oblique planar
wall, stopping part-way along the edge, with a triangular flat closing the blind end. Milled
as an angled shoulder or a lead-in ramp, it is the feature MFCAD++ calls a *triangular blind
step*.

Geometrically it is the same read as a chamfer — an oblique planar face bridging two
mutually-perpendicular axis-aligned faces at a convex corner — and that is exactly why it
needs recognising. Before this module existed ``recognise_chamfers`` reported these slants
as chamfers, because nothing distinguished them. Measured over 60 MFCAD++ models, **9 of
the 10 models carrying one had their step's slant reported as their only chamfer**, while
the genuine chamfers on the same parts were rejected.

The distinction is **not** size. That was the tempting answer and it does not survive
measurement: the legs of the two populations overlap on every part-relative and
neighbour-relative ratio tried, and a threshold that separated them on one corpus would be
fitted to that corpus. The distinction is topological — **a chamfer runs the full length of
the edge it breaks; an angled step stops, and something has to close the end.** That
something is a triangular flat, and :func:`_closed_by_a_triangular_flat` is the whole
discriminator. It says nothing about the part around the face, so a step is a step at any
scale, which a size gate could never promise.

**That test is this family's own, and no longer shared.** It lived in ``_adjacency`` while
``recognise_chamfers`` consulted it too — to *decline* a bevel this family would claim — and the
comment on it said the two must agree or the feature would vanish from the census, claimed by
neither. Agreement by shared helper is a hand-rolled ownership device: it worked for two
contestants and generalised to no third. The chamfer family no longer asks the
question at all. It proposes every bevel it sees, this family claims the ones with a blind end,
and :func:`quiddity._reconcile.chamfers_that_are_not_angled_steps` reads the claims and
drops the duplicates — so there is exactly one implementation of the discriminator, owned by the
family it defines, and nothing left for a second copy to drift from.

Five gates, the first three shared with :func:`quiddity.recognise_chamfers` so the
two cannot disagree about what they are looking at:

- **an oblique bevel** — :func:`quiddity.classify_bevel`, so the slant is a planar
  face running along exactly one principal axis;
- **bridges two axis-aligned faces on distinct in-plane axes** — this is what excludes a
  *pocket* wall, and it does nearly all the work. A triangular pocket whose plan is not
  axis-aligned otherwise matches the signature exactly: its walls are oblique planes and its
  floor is a triangle. Prototyped without this gate, pockets outnumbered steps three to one
  (precision 21%); over 120 MFCAD++ models it stops all 109 of them, and the convex probe
  stops none;
- **convex** — :func:`quiddity.chamfers.convex_bevel`, and it carries real weight:
  of the 85 faces that reach it over 120 MFCAD++ models it rejects 24. What it catches is
  a slant with material *behind* it rather than below — a gusset filling a concave corner,
  whose hypotenuse bridges two perpendicular walls and whose ends are triangles, so it
  satisfies every other gate here and the material is the only thing that differs;
- **no material beyond the corner** — :func:`quiddity._bevel.material_beyond_corner`,
  which finishes the job the second gate was doing alone. That gate excludes a pocket wall by
  asking what the slant bridges, and a triangular pocket whose *other two* walls happen to be
  axis-aligned answers correctly and is a pocket anyway. The held-out corpus contained one and
  the design corpus did not, which is the whole argument for holding a corpus out. The corner
  a step replaces is a corner of the stock, with free space behind it; the corner a recess wall
  replaces is where two walls of that recess meet, with the material they were cut from behind
  it. Convexity cannot see the difference — both have vacuum on the bevel side;
- **a triangular companion** — the blind end.

No size gate or length tolerance: every gate here is either a shared geometric classification,
a solid-classifier probe, a boundary count or a dimensionless direction comparison.

**A raw edge count is not the triangle proof.** A hole drilled through the blind end adds an
inner wire, while a neighbouring feature may split one straight outer side into several
co-directed edges. Neither changes the terminal's geometric boundary. The predicate therefore
requires exactly three cyclic linear runs on the outer wire: it collapses only consecutive
co-directed straight edges under the shared dimensionless smooth-direction tolerance. A chamfer
strip's rectangular cap retains four runs, a kinked near-triangle retains four, and a curved or
unreadable boundary fails closed. This is the named subdivided-region query required by ADR 0004,
not a relaxation from three edges to four or five.

Depends on ``chamfers`` for the bevel read and the convexity probe rather than copying
either, so a change to what counts as a bevel reaches both recognisers at once.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from quiddity._adjacency import (
    FaceEdges,
    FaceGraph,
    axis_aligned_axis,
    edge_face_map,
    nearest_axis_aligned_planes,
    neighbours,
)
from quiddity._bevel import (
    BevelReject,
    classify_bevel,
    convex_bevel,
    material_beyond_corner,
)
from quiddity._candidates import (
    EvidenceSink,
    FamilyId,
)
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._geometry import SMOOTH_ARC_GAP
from quiddity._record import Record
from quiddity._typing import FaceLike, Part


@dataclass(frozen=True, order=True)
class AngledStep(Record):
    """A recognised angled blind step. ``axis`` is the direction the slant runs along
    ("x"/"y"/"z"); ``leg1``/``leg2`` are the cut depths into the two adjacent faces
    (``leg1`` the larger); ``angle`` is the slant angle in degrees (45 for equal-leg);
    ``length`` is how far the step runs before its blind end; ``at`` is the slant face
    centre in part space (the callout leader's tip).

    The fields mirror :class:`quiddity.Chamfer` because the geometry is the same
    read — ``length`` is the addition, and it is the field a chamfer has no use for: a
    chamfer runs the whole edge, so its length is not a chosen dimension.
    """

    axis: str
    leg1: float
    leg2: float
    angle: float
    length: float
    at: tuple[float, float, float]


def _closed_by_a_triangular_flat(
    face: FaceLike, edge_faces: dict, *, face_edges: FaceEdges | None = None
) -> bool:
    """Is *face* adjacent to an axis-aligned planar face with a triangular outer boundary?

    The blind end, and the whole discriminator (see the module docstring). A chamfer strip runs
    the length of the edge it breaks, so its neighbours are the two walls it bridges; a slant
    that stops part-way into the part needs a flat to close it, and that flat is a triangle.

    Purely topological: it reads a neighbour's surface type, its plane's alignment and its edge
    count, and nothing about the size of anything. That is what lets the family promise a step
    is a step at any scale.

    Private, and that is the point. It was ``_adjacency``'s shared
    ``has_triangular_companion`` while ``recognise_chamfers`` consulted it as well; it consults
    nothing now, so the one caller keeps it.
    """

    return bool(_terminal_read(face, edge_faces, face_edges=face_edges))


def _effective_linear_sides(face: FaceLike) -> int | None:
    """Count cyclic, co-directed linear runs on one outer boundary, or fail closed."""

    try:
        edges = list(face.outer_wire().edges())
        if not edges or any(edge.geom_type.name != "LINE" for edge in edges):
            return None
        directions = [edge.tangent_at().normalized() for edge in edges]
    except Exception:  # pragma: no cover - defensive kernel boundary
        # This is diagnostic evidence only. An unreadable imported boundary must not change
        # recognition behavior by raising where the existing terminal predicate returned false.
        return None
    return sum(
        1
        for previous, current in zip(directions, directions[1:] + directions[:1], strict=True)
        if 1.0 - previous.dot(current) > SMOOTH_ARC_GAP
    )


def _terminal_read(
    face: FaceLike, edge_faces: dict, *, face_edges: FaceEdges | None = None
) -> list[FaceLike]:
    """Return legacy three-edge terminals or split terminals proved as three straight runs."""

    terminals: list[FaceLike] = []
    for other in neighbours(face, edge_faces, face_edges=face_edges):
        if axis_aligned_axis(other.wrapped) is None:
            continue
        edges = face_edges.of(other) if face_edges is not None else other.edges()
        if len(edges) == 3:
            terminals.append(other)
            continue
        # Four edges is either a rectangle -- a chamfer strip's own end cap, which has to stay
        # rejected -- or a triangle with a hole through it, which is a blind end with a bolt
        # hole in it. The outer wire separates them and the memo cannot, so it is consulted
        # only when the plain count has already failed.
        outer_edges = list(other.outer_wire().edges())
        if len(outer_edges) == 3:
            terminals.append(other)
            continue
        effective_sides = _effective_linear_sides(other)
        if len(outer_edges) > 3 and effective_sides == 3:
            terminals.append(other)
    return terminals


def recognise_angled_steps(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[AngledStep]:
    """Recognise the angled blind steps of *part* (see module docstring). Returns one
    :class:`AngledStep` per qualifying slant face, sorted deterministically. Empty when the
    part has none. Only single-axis slants (running along one principal axis) are recovered;
    a step whose blind end is closed by anything other than a triangular flat is not one —
    that end is what makes the feature blind, and without it the slant is a chamfer or a
    through step.

    *ledger* records the face a step was **established by**: its slant, and only that. Every
    number on the record is read off that one face — both legs from its in-plane extents,
    ``length`` from its span along the edge, ``at`` from its centre. The triangular flat that
    closes the blind end remains outside defining ownership, so it cannot make a step contest its
    own end cap; it is retained only as wider constituent membership for downstream selection.

    ``recognise_chamfers`` reads the same slant as a bevel and proposes it too, because on the
    face alone it is one. Which of the two survives is
    :func:`quiddity._reconcile.chamfers_that_are_not_angled_steps`, decided from these
    claims rather than by each family second-guessing the other."""

    graph = None if ledger is None else ledger.graph
    sink = None if ledger is None else ledger.sink
    return _discover_angled_steps(part, face_edges=face_edges, graph=graph, sink=sink)


def _discover_angled_steps(
    part: Part,
    *,
    face_edges: FaceEdges | None,
    graph: FaceGraph | None,
    sink: EvidenceSink | None,
) -> list[AngledStep]:
    """Discover from geometry and append through a capability with no evidence reads."""

    all_faces = list(part.faces())
    edge_faces = edge_face_map(all_faces, face_edges=face_edges)

    out: list[tuple[AngledStep, FaceLike, tuple[FaceLike, ...]]] = []
    for f in all_faces:
        try:
            edge_i, _nv, span, leg_hi, leg_lo = classify_bevel(f)
        except BevelReject:
            continue
        oi = [j for j in (0, 1, 2) if j != edge_i]
        fc = {i: 0.5 * (span[i][0] + span[i][1]) for i in (0, 1, 2)}  # face centre
        neigh_coord = nearest_axis_aligned_planes(
            f, edge_faces, fc, exclude_axis=edge_i, face_edges=face_edges
        )
        if oi[0] not in neigh_coord or oi[1] not in neigh_coord:
            continue
        if not convex_bevel(part, fc, edge_i, neigh_coord):
            continue  # concave — a pocket or passage wall, not a step
        if material_beyond_corner(part, fc, edge_i, neigh_coord):
            continue  # the corner two recess walls meet at, not a corner of the part
        # Last, though it is the gate this family is named for — the three above it are the
        # shared bevel read, and this is the one thing that is only about a step.
        # Hoisting it ahead of the solid classifier is the obvious optimisation and
        # measures as nothing (850.0 ms against 848.0 ms over the golden corpus, interleaved):
        # `convex_bevel` is reached by so few faces that it is 1% of this function, while the
        # companion walk costs a `.edges()` per axis-aligned neighbour. The cost lives in the
        # unavoidable scan above — `edge_face_map` and `classify_bevel` are two thirds of it.
        terminals = _terminal_read(f, edge_faces, face_edges=face_edges)
        if not terminals:
            continue  # runs edge to edge — a chamfer, and the reconciler leaves it to them
        fctr = f.center()
        out.append(
            (
                AngledStep(
                    axis="xyz"[edge_i],
                    leg1=round(leg_hi, 3),
                    leg2=round(leg_lo, 3),
                    angle=round(math.degrees(math.atan2(leg_lo, leg_hi)), 2),
                    length=round(span[edge_i][1] - span[edge_i][0], 3),
                    at=(round(fctr.X, 3), round(fctr.Y, 3), round(fctr.Z, 3)),
                ),
                f,
                tuple(terminals),
            )
        )
    out.sort(key=lambda pair: (pair[0].axis, pair[0].at))
    if sink is not None:
        if graph is None:
            raise ValueError("an evidence sink requires its graph")
        for step, face, terminal_faces in out:
            defining = graph.require_node(face)
            sink.propose(
                FamilyId.ANGLED_STEPS,
                step,
                defining=[defining],
                constituent=[
                    defining,
                    *(graph.require_node(item) for item in terminal_faces),
                ],
            )
    return [step for step, _face, _terminals in out]
