# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Geometry-only STEP loading for recognition inputs.

Recognition needs B-Rep topology, not STEP assembly names, colours or product structure.  The
ordinary build123d importer reads those attributes through XCAF; current OCP bindings can terminate
the interpreter when an assembly component has no name.  This module deliberately uses OCCT's
plain geometry reader and returns its single transferred shape as the corresponding build123d
wrapper.
"""

from __future__ import annotations

import os
from collections.abc import Callable
from typing import TypeVar, cast

from build123d import Shape
from build123d.importers import topods_lut
from build123d.topology.shape_core import downcast
from OCP.IFSelect import IFSelect_ReturnStatus
from OCP.STEPControl import STEPControl_Reader
from OCP.TopoDS import TopoDS_Shape

_ShapeT = TypeVar("_ShapeT", bound=Shape)


def import_step_geometry(path: str | os.PathLike[str]) -> Shape:
    """Load the complete STEP transfer as geometry without traversing XCAF metadata.

    The returned shape may be a solid, compound, shell, face or other build123d shape matching the
    STEP transfer.  Assembly structure is intentionally flattened into ``OneShape()``; solid and
    face topology remain available to recognition.  Read or transfer failure raises ``ValueError``
    rather than returning a null/partial shape.
    """

    source = os.fspath(path)
    reader = STEPControl_Reader()
    status = reader.ReadFile(source)
    if status != IFSelect_ReturnStatus.IFSelect_RetDone:
        raise ValueError(f"cannot read STEP geometry from {source!r}: {status.name}")
    expected_roots = int(reader.NbRootsForTransfer())
    transferred = int(reader.TransferRoots())
    if transferred <= 0:
        raise ValueError(f"STEP file {source!r} contains no transferable roots")
    if transferred != expected_roots:
        raise ValueError(
            f"STEP file {source!r} transferred {transferred} of {expected_roots} roots; "
            "refusing incomplete geometry"
        )
    topology = cast(TopoDS_Shape, downcast(reader.OneShape()))
    if topology.IsNull():
        raise ValueError(f"STEP file {source!r} transferred a null shape")
    wrapper = cast(Callable[[TopoDS_Shape], _ShapeT] | None, topods_lut.get(type(topology)))
    if wrapper is None:
        raise ValueError(
            f"STEP file {source!r} transferred unsupported topology {type(topology).__name__}"
        )
    return wrapper(topology)


__all__ = ["import_step_geometry"]
