# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of raised cylindrical bosses."""

from dataclasses import dataclass

from build123d import Face

from quiddity._adjacency import (
    FaceEdges,
    FaceNode,
    edge_face_map,
)
from quiddity._candidates import CompletedInputs, FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._cylinder_stacks import (
    _axis_point,
    _classify_end,
    _family_surface_query,
    _full_cyls,
    _segments,
)
from quiddity._cylinder_substrate import (
    analyse_cylinders,
)
from quiddity._definitions import (
    Counted,
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    PhysicalDefinition,
    always,
)
from quiddity._effective_surfaces import (
    EffectiveFaceSurfaceQuery,
    SurfaceUse,
    SurfaceUseRefusal,
    cylinder_surface_dependency,
)
from quiddity._geometry import without_negative_zero
from quiddity._record import Record
from quiddity._typing import CylinderInventory, Part, Vector3


@dataclass(frozen=True)
class BossRecord(Record):
    """An external cylindrical boss (including a turned part's OD).

    ``axis`` points from the base toward the free end, ``location`` is the
    axis point at the free end, and ``height`` the axial extent.
    """

    axis: Vector3
    location: Vector3
    diameter: float
    height: float

    @property
    def length(self) -> float:
        """Length along the boss axis."""
        return self.height


@dataclass(frozen=True, slots=True)
class _BossProposal:
    record: BossRecord
    segment_faces: tuple[Face, ...]
    terminal_faces: tuple[Face, ...]


def recognise_bosses(
    part: Part, *, cyls: CylinderInventory | None = None, face_edges: FaceEdges | None = None
) -> list[BossRecord]:
    """Recognise external cylindrical bosses on *part* (one
    :class:`BossRecord` per coaxial external cylinder segment, including a
    turned part's OD — callers wanting only local bosses can filter on
    diameter against the part envelope).

    Pass *cyls* — a precomputed ``analyse_cylinders(part)`` result — to avoid
    re-scanning the solid (mirrors ``recognise_holes``'s parameter, so a caller
    computing both holes and bosses can share one analysis).
    """
    return _discover_bosses(part, cyls=cyls, face_edges=face_edges)


def _discover_bosses(
    part: Part,
    *,
    cyls: CylinderInventory | None = None,
    face_edges: FaceEdges | None = None,
    writer: EvidenceWriter | None = None,
    face_surfaces: EffectiveFaceSurfaceQuery | None = None,
) -> list[BossRecord]:
    """Discover Bosses and validate every defining segment before publication."""

    effective = _family_surface_query(part, writer, face_surfaces)
    z_cyls, cross_cyls = (
        cyls if cyls is not None else analyse_cylinders(part, face_surfaces=effective)
    )
    external = [c for c in _full_cyls(z_cyls) + _full_cyls(cross_cyls) if c["external"]]
    if not external:
        return []
    edge_faces = edge_face_map(part.faces(), face_edges=face_edges)
    cache: dict = {}

    proposals: list[_BossProposal] = []
    for seg in _segments(external):
        d = seg["dir_xyz"]
        lo_faces: list[Face] = []
        hi_faces: list[Face] = []
        lo_state = _classify_end(
            seg,
            seg["s_lo"],
            False,
            edge_faces,
            cache,
            effective,
            terminal_faces=lo_faces,
        )
        hi_state = _classify_end(
            seg,
            seg["s_hi"],
            True,
            edge_faces,
            cache,
            effective,
            terminal_faces=hi_faces,
        )
        # The free end is the open one (its cap faces away from the segment);
        # default to the high end when both or neither are open.
        from_hi = not (lo_state == "open" and hi_state != "open")
        proposals.append(
            _BossProposal(
                BossRecord(
                    axis=without_negative_zero(d if from_hi else tuple(-c for c in d)),
                    location=_axis_point(seg, seg["s_hi"] if from_hi else seg["s_lo"]),
                    diameter=seg["diameter"],
                    height=round(seg["s_hi"] - seg["s_lo"], 2),
                ),
                tuple(seg["faces"]),
                tuple(hi_faces if from_hi and hi_state == "open" else lo_faces)
                if (hi_state if from_hi else lo_state) == "open"
                else (),
            )
        )

    if writer is not None:
        assert effective is not None
        pending: list[tuple[BossRecord, tuple[FaceNode, ...], tuple[FaceNode, ...]]] = []
        for proposal in proposals:
            resolved = {writer.graph.require_node(face) for face in proposal.segment_faces}
            nodes = tuple(node for node in writer.graph.nodes if node in resolved)
            if not nodes:
                if writer.graph.local_degradation:
                    continue
                raise ValueError("Boss defining faces do not prove one valid solid")
            terminal_resolved = {
                writer.graph.require_node(face) for face in proposal.terminal_faces
            }
            if resolved & terminal_resolved:
                raise ValueError("Boss terminal identity aliases cylindrical evidence")
            terminal_nodes = tuple(node for node in writer.graph.nodes if node in terminal_resolved)
            members = (*nodes, *terminal_nodes)
            solid = writer.graph.common_valid_solid(members)
            if solid is None:
                if writer.graph.local_degradation:
                    continue
                raise ValueError("Boss defining faces do not prove one valid solid")
            pending.append((proposal.record, nodes, members))
        issued_pending: list[
            tuple[BossRecord, tuple[FaceNode, ...], tuple[FaceNode, ...], tuple[SurfaceUse, ...]]
        ] = []
        for record, nodes, members in pending:
            issued = tuple(
                cylinder_surface_dependency(effective, writer.graph.face(node)) for node in nodes
            )
            if any(isinstance(use, SurfaceUseRefusal) for use in issued):
                raise ValueError("Boss cylinder provenance is unavailable")
            uses = tuple(use for use in issued if isinstance(use, SurfaceUse))
            issued_pending.append((record, nodes, members, uses))
        for record, nodes, members, uses in issued_pending:
            writer.add_defining(
                record,
                nodes,
                family=FamilyId.BOSSES,
                constituent=members,
                surfaces=uses,
            )

    return [proposal.record for proposal in proposals]


# What this family declares about itself; `_registry` decides where it runs.
def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs  # no completed predecessors
    return list(
        _discover_bosses(
            services.context.part,
            cyls=services.cylinders,
            face_edges=services.context.face_edges,
            writer=services.writer,
            face_surfaces=services.context.face_surfaces,
        )
    )


DEFINITION = PhysicalDefinition(
    family=FamilyId.BOSSES,
    record_types=(BossRecord,),
    result_field="bosses",
    public_entrypoint=recognise_bosses.__name__,
    dependencies=(),
    applicable=always,
    discover=_discover,
    census=Counted("boss"),
    attribution=FullyAttributed("every returned boss claims its original external segment faces"),
    evidence=ManifestEvidence(goldens=("simple_through_hole", "turned_steps_and_grooves")),
)
