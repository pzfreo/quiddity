# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Internal bridge from facade faces to one aggregate evidence authority."""

from __future__ import annotations

from collections.abc import Iterable

from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._typing import FaceLike
from quiddity.experimental_geometry import FaceRef, GeometryGraph


class GeometryEvidenceBridge:
    """Resolve borrowed facade faces only against the writer's exact graph."""

    __slots__ = ("geometry", "_writer")

    def __init__(self, writer: object, geometry: GeometryGraph | None = None) -> None:
        if type(writer) is not EvidenceWriter:
            raise TypeError("geometry evidence requires an aggregate evidence writer")
        if geometry is None:
            geometry = GeometryGraph._from_graph(writer.graph)
        elif not geometry._uses_graph(writer.graph):
            raise ValueError("geometry facade and evidence writer belong to different runs")
        self.geometry = geometry
        self._writer = writer

    def refs(self, faces: Iterable[FaceLike]) -> tuple[FaceRef, ...]:
        resolved = {self.geometry.ref(face) for face in faces}
        return tuple(ref for ref in self.geometry.faces if ref in resolved)

    def add_defining(
        self,
        claimant: object,
        refs: Iterable[FaceRef],
        *,
        family: FamilyId,
        constituent: Iterable[FaceRef] | None = None,
    ) -> None:
        ordered = tuple(refs)
        nodes = tuple(self.geometry._node(ref) for ref in ordered)
        members = (
            None if constituent is None else tuple(self.geometry._node(ref) for ref in constituent)
        )
        self._writer.add_defining(claimant, nodes, family=family, constituent=members)

    def validate_defining(self, refs: Iterable[FaceRef]) -> None:
        """Validate a complete publication batch before any candidate is issued."""

        nodes = tuple(self.geometry._node(ref) for ref in refs)
        if self._writer.graph.common_valid_solid(nodes) is None:
            raise ValueError("defining faces do not belong to one valid solid")


__all__: list[str] = []
