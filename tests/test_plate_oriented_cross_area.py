# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""The oriented cross-envelope loop: the same directions, asked once each.

``_oriented_cross_area`` establishes which planar normals bound the body by asking, for each
eligible face, whether any vertex of the part projects further along that face's normal than
the face's own plane does. That question is about a *direction*, not a face, and a prismatic
body has far fewer directions than faces -- four NIST parts spent 375k inner terms on about
eighty distinct answers, and walked the body's vertices once per axis to do it.

Hoisting both is only allowed if the published records do not move, which is what the identity
test below pins: every golden fixture's plate records, computed with the hoists and computed by
the pre-change body of the function, verbatim.
"""

from __future__ import annotations

import importlib.util
import math
from pathlib import Path

from build123d import Axis
from OCP.BRepAdaptor import BRepAdaptor_Surface

from quiddity import plates as plates_module
from quiddity._geometry import AXIS_ALIGNED_COS
from quiddity._solid_properties import SolidProperties
from quiddity.plates import _ORIENTED_ANGLE_DIGITS, recognise_plates

GOLDEN_ROOT = Path(__file__).parent / "golden"
FIXTURES = sorted(GOLDEN_ROOT.glob("*/fixture.py"))
#: The multi-axis bracket: a base, a wall and two pads, so more than one axis reaches
#: the oriented envelope and the per-body vertex walk has something to be shared over.
PLATE_FIXTURE = GOLDEN_ROOT / "plates_pads_levels_and_slanted_steps" / "fixture.py"


def _load(path: Path):
    spec = importlib.util.spec_from_file_location(f"cross_area_fixture_{path.parent.name}", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _unhoisted_cross_area(part, faces, axis_index, extents, properties):
    """The body of ``_oriented_cross_area`` as it stood before the two hoists.

    Kept verbatim rather than expressed in terms of the current function: the point of the
    comparison is that the arithmetic is unchanged, and a paraphrase would test the paraphrase.
    ``properties`` is accepted and ignored, so the caller is the production caller.
    """

    other = [index for index in range(3) if index != axis_index]
    angles: set[float] = set()
    vertices = tuple(tuple(vertex) for vertex in part.vertices())
    support_eps = max(max(extents), 1.0) * 1e-9
    for face in faces:
        try:
            normal = tuple(face.normal_at())
        except Exception:  # noqa: BLE001 -- a degenerate plane establishes no direction
            continue
        if abs(normal[axis_index]) > 1.0 - AXIS_ALIGNED_COS:
            continue
        projected = math.hypot(normal[other[0]], normal[other[1]])
        if projected < AXIS_ALIGNED_COS:
            continue
        location = BRepAdaptor_Surface(face.wrapped).Plane().Location()
        plane = (location.X(), location.Y(), location.Z())
        plane_projection = sum(
            value * direction for value, direction in zip(plane, normal, strict=True)
        )
        if (
            vertices
            and max(
                sum(value * direction for value, direction in zip(vertex, normal, strict=True))
                for vertex in vertices
            )
            > plane_projection + support_eps
        ):
            continue
        angle = math.degrees(math.atan2(normal[other[1]], normal[other[0]])) % 90.0
        angles.add(round(angle, _ORIENTED_ANGLE_DIGITS) % 90.0)

    if not angles:
        return extents[other[0]] * extents[other[1]]

    rotation_axis = (Axis.X, Axis.Y, Axis.Z)[axis_index]
    sign = 1.0 if axis_index == 1 else -1.0
    cross_areas: list[float] = []
    for angle in angles:
        size = (
            extents
            if angle == 0.0
            else tuple(
                float(component)
                for component in part.rotate(rotation_axis, sign * angle).bounding_box().size
            )
        )
        cross_areas.append(size[other[0]] * size[other[1]])
    return min(cross_areas)


def test_every_golden_plate_record_survives_the_hoists(monkeypatch) -> None:
    """The identity the hoists have to buy their speed without spending."""

    parts = [(path.parent.name, _load(path).build_fixture()) for path in FIXTURES]
    hoisted = {name: recognise_plates(part) for name, part in parts}
    published = sum(len(records) for records in hoisted.values())

    monkeypatch.setattr(plates_module, "_oriented_cross_area", _unhoisted_cross_area)
    for name, part in parts:
        assert recognise_plates(part) == hoisted[name], name

    assert published, "the golden corpus published no plate at all -- the check was vacuous"


def test_the_direction_memo_answers_what_the_per_face_loop_answered() -> None:
    """One eligible normal shared by many faces must give one answer, and the same one."""

    part = _load(PLATE_FIXTURE).build_fixture()
    faces = list(part.faces())
    bounds = part.bounding_box()
    extents = (
        bounds.max.X - bounds.min.X,
        bounds.max.Y - bounds.min.Y,
        bounds.max.Z - bounds.min.Z,
    )
    for axis_index in (0, 1, 2):
        memo = SolidProperties()
        assert plates_module._oriented_cross_area(
            part, faces, axis_index, extents, memo
        ) == _unhoisted_cross_area(part, faces, axis_index, extents, memo)


def test_the_body_walks_its_vertices_once_for_all_three_axes(monkeypatch) -> None:
    """An operation-count sentinel for the second hoist.

    ``_oriented_cross_area`` is asked once per axis, and before the hoist each ask rebuilt the
    same vertex tuple -- about 10 ms of the 12 ms it cost on the 664-face ``nist_ctc_02``. The
    run cache holds it per body, so a body is walked once however many axes reach the envelope.
    """

    part = _load(PLATE_FIXTURE).build_fixture()
    walks = 0
    original = plates_module._vertex_coordinates

    def counted(solid):
        nonlocal walks
        walks += 1
        return original(solid)

    monkeypatch.setattr(plates_module, "_vertex_coordinates", counted)
    calls = 0
    inner = plates_module._oriented_cross_area

    def counting_cross_area(*args):
        nonlocal calls
        calls += 1
        return inner(*args)

    monkeypatch.setattr(plates_module, "_oriented_cross_area", counting_cross_area)
    recognise_plates(part)
    assert calls > 1, "the fixture must reach the envelope on more than one axis to prove this"
    assert walks == 1
