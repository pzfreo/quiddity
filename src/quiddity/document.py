# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""JSON document projection of one ordinary framed recognition and evidence run."""

from __future__ import annotations

from dataclasses import asdict

from OCP.BRepCheck import BRepCheck_Analyzer, BRepCheck_Status

from quiddity import __version__
from quiddity._adjacency import FaceGraph
from quiddity._edit_document import enrich_edit_document
from quiddity._typing import Part
from quiddity.evidence import FaceRef, FramedRecognitionEvidence
from quiddity.frames import build_framed_recognition_evidence


def build_recognition_document(part: Part, *, rotational: bool = False) -> dict[str, object]:
    """Recognise once and project existing records, evidence and association to JSON values.

    Coordinates and embedded face/body indices refer to the local working shape. The frame
    maps local coordinates back to the caller; each face also carries its caller roster index.
    Indices are document-local, not persistent STEP identifiers. Frame/evidence refusal raises
    ValueError rather than silently switching to raw coordinates.
    """
    degraded = False
    try:
        view = build_framed_recognition_evidence(part, rotational=rotational)
    except ValueError:
        if not any(not BRepCheck_Analyzer(solid.wrapped).IsValid() for solid in part.solids()):
            raise
        view = build_framed_recognition_evidence(
            part, rotational=rotational, local_degradation=True
        )
        degraded = True
    if not isinstance(view, FramedRecognitionEvidence):
        raise ValueError(f"framed recognition refused: {view.reason.value}")

    local_faces = tuple(view.part.faces())
    caller_faces = tuple(part.faces())
    indices: dict[FaceRef, int] = {}
    caller_indices: dict[int, int] = {}
    for reference in view.faces:
        local = view.face(reference)
        caller = view.caller_face(reference)
        matches = [i for i, face in enumerate(local_faces) if local.wrapped.IsSame(face.wrapped)]
        originals = [
            i for i, face in enumerate(caller_faces) if caller.wrapped.IsSame(face.wrapped)
        ]
        if len(matches) != 1 or len(originals) != 1:
            raise ValueError("document face mapping is not one-to-one")
        indices[reference] = matches[0]
        caller_indices[matches[0]] = originals[0]
    if len(caller_indices) != len(local_faces) or len(set(caller_indices.values())) != len(
        caller_faces
    ):
        raise ValueError("document face mapping is incomplete")

    bodies = tuple(view.part.solids())
    body_faces = [tuple(body.faces()) for body in bodies]
    faces = [
        {
            "index": index,
            "caller_index": caller_indices[index],
            "body_indices": [
                body_index
                for body_index, members in enumerate(body_faces)
                if any(face.wrapped.IsSame(member.wrapped) for member in members)
            ],
        }
        for index, face in enumerate(local_faces)
    ]
    graph = FaceGraph(view.part)
    if degraded:
        by_face = {face: index for index, face in enumerate(local_faces)}
        defective: set[int] = set()
        for body in bodies:
            analyzer = BRepCheck_Analyzer(body.wrapped)
            defective.update(
                by_face[face]
                for face in body.faces()
                if face in by_face
                and tuple(analyzer.Result(face.wrapped).Status())
                != (BRepCheck_Status.BRepCheck_NoError,)
            )
        unproven = defective | {
            neighbour.index
            for index in defective
            for neighbour in graph.neighbours(graph.nodes[index])
        }
        for face in faces:
            face["proof"] = "not_proven" if face["index"] in unproven else "local"
    features = [
        {
            "index": index,
            "family": view.family(feature),
            "record_type": type(view.record(feature)).__name__,
            "record": view.record(feature).to_dict(),
            "defining_faces": sorted(indices[face] for face in view.defining_faces(feature)),
            "constituent_faces": sorted(indices[face] for face in view.constituent_faces(feature)),
        }
        for index, feature in enumerate(view.features)
    ]
    derived = {
        name: [record.to_dict() for record in getattr(view.result, name)]
        for name in (
            "hole_patterns",
            "slot_patterns",
            "oriented_slot_patterns",
            "section_recess_patterns",
            "turned_profiles",
        )
    }
    enrich_edit_document(view, faces, features, derived, graph=graph)
    association = view.association
    frame = view.frame
    body_records = []
    for index, body in enumerate(bodies):
        bounds = body.bounding_box()
        spans = {
            axis: float(getattr(bounds.max, axis.upper()) - getattr(bounds.min, axis.upper()))
            for axis in "xyz"
        }
        thin_axis = min(spans, key=spans.__getitem__)
        body_records.append(
            {
                "index": index,
                "named_dimensions": {
                    "overall_spans": spans,
                    "overall_thickness": spans[thin_axis],
                    "thickness_axis": thin_axis,
                },
            }
        )
    return {
        "format": "quiddity-recognition",
        "format_version": 2,
        "package": {"name": "quiddity", "version": __version__},
        "coordinate_space": "local",
        "rotational": rotational,
        "proof": "local_degradation" if degraded else "whole_solid",
        "frame": {
            "origin": list(frame.origin),
            "x": list(frame.x),
            "y": list(frame.y),
            "z": list(frame.z),
            "gauge": frame.gauge.value,
        },
        "bodies": body_records,
        "faces": faces,
        "features": features,
        "derived": derived,
        "association": {
            "face_count": {**asdict(association.face_count), "ratio": association.face_count.ratio},
            "surface_area": {
                **asdict(association.surface_area),
                "ratio": association.surface_area.ratio,
            },
            "families": [asdict(family) for family in association.families],
            "unassociated_faces": sorted(indices[face] for face in association.unassociated_faces),
        },
    }
