# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""The state one recognition run shares, in one object instead of several optional arguments.

A recogniser called on its own takes a part and works everything out. Called as part of an
aggregate it should not: the face-edge derivation, the face graph and the cylinder scan are
all facts about *this run over this part*, and deriving them per family is
both waste and a way for two families to disagree about the same solid.

Before this they were threaded as separate optional parameters -- ``face_edges=``, ``ledger=``,
``cyls=`` -- each defaulting to "build your own if not given". That shape has produced two real
defects rather than one hypothetical: `recognise_polygonal_bosses` had no parameter with which
to *receive* the caller's graph, so every aggregate run over a single solid built a second one
over the same faces; and the two orchestration paths derived similar state separately and
drifted, reporting different answers about one part until a corpus-wide comparison found it.

**Run-local and short-lived, like everything it holds.** The graph's node ids mean nothing once
the part changes and `FaceEdges` must not outlive the call, so this must not either. It is
private for the same reason: a public standalone recogniser keeps its own signature and
constructs what it needs, and only the orchestration passes one of these.
"""

from __future__ import annotations

from dataclasses import dataclass

from quiddity._adjacency import FaceEdges, FaceGraph
from quiddity._cylinder_substrate import analyse_cylinders
from quiddity._effective_surfaces import (
    EffectiveFaceSurfaceQuery,
    EffectiveSurfaceIndex,
    effective_faces_for_graph,
)
from quiddity._typing import CylinderInventory, FrozenCylinderInventory, Part
from quiddity.experimental_geometry import GeometryGraph


@dataclass(frozen=True, slots=True)
class RecognitionContext:
    """The derived facts every family in one run over one part should share.

    Frozen because nothing here is a result: a run is the *evidence* a run has in common, and a
    family that wanted to replace one of these would be changing what the others already saw.
    ``FaceGraph`` and ``FaceEdges`` fill private memo caches but their geometric meaning cannot
    be replaced through this capability. Evidence is deliberately absent: candidates belong to
    the discovery lifecycle, not to neutral context.
    """

    part: Part
    face_edges: FaceEdges
    graph: FaceGraph
    geometry: GeometryGraph
    surfaces: EffectiveSurfaceIndex
    face_surfaces: EffectiveFaceSurfaceQuery
    cylinders: FrozenCylinderInventory
    rotational: bool


# Temporary internal name compatibility while phase callers migrate in this change.
RecognitionRun = RecognitionContext


def start(
    part: Part,
    cylinders: CylinderInventory | None = None,
    *,
    rotational: bool = False,
    local_degradation: bool = False,
) -> RecognitionContext:
    """Derive the shared state for one run over *part*.

    *cylinders* is accepted because `build_recognition_result` lets a caller inject a scan it
    has already paid for. Everything else is derived here, once, rather than by whichever family
    happens to ask first.

    The graph is built **with** the face-edge memo. `feature_census` always did this and
    `build_recognition_result` did not, so the aggregate paid for a second `Face.edges()`
    derivation across its whole run -- measured at about a fifth of a census when that memo was
    introduced. Nothing about the answers changes; it is the same derivation asked once.
    """

    face_edges = FaceEdges()
    graph = FaceGraph(part, face_edges=face_edges, local_degradation=local_degradation)
    surfaces = EffectiveSurfaceIndex(graph)
    face_surfaces = effective_faces_for_graph(graph, surfaces)
    derived = (
        analyse_cylinders(part, face_surfaces=face_surfaces) if cylinders is None else cylinders
    )
    return RecognitionContext(
        part=part,
        face_edges=face_edges,
        graph=graph,
        geometry=GeometryGraph._from_graph(graph, surfaces),
        surfaces=surfaces,
        face_surfaces=face_surfaces,
        cylinders=(tuple(derived[0]), tuple(derived[1])),
        rotational=rotational,
    )
