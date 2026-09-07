"""Authored physical support proofs across supported build123d boolean return APIs."""

from itertools import permutations

import pytest
from build123d import Box, Compound, Face, Pos, Shape, ShapeList, Wire

from quiddity import build_section_recess_document
from quiddity._adjacency import FaceGraph
from quiddity._open_channel_section import _supports


def rectangle(x0, x1, y0=0, y1=10):
    return Face(
        Wire.make_polygon([(x0, y0, 0), (x1, y0, 0), (x1, y1, 0), (x0, y1, 0), (x0, y0, 0)])
    )


def supported(faces, *, every_order=False):
    graph = FaceGraph(Compound(faces))
    orders = permutations(graph.nodes) if every_order else [tuple(graph.nodes)]
    # Explicit order challenges a split-first subtraction and every subsequent fragment.
    return [_supports(graph, nodes, ((0, 10), (0, 10), (0, 0)), 2, 0, 1) for nodes in orders]


def test_public_pocket_reproduction():
    part = Box(100, 60, 20) - Pos(22, -11, 7) * Box(30, 12, 6)
    document = build_section_recess_document(part)
    assert len(document.occurrences) == 1
    assert document.occurrences[0].classification.feature_kind == "pocket"
    assert document.refusals == ()


def test_complete_support_and_empty_boolean_result():
    assert supported([rectangle(0, 10)]) == [True]


def test_fragmented_remainder_is_subtracted_from_every_piece():
    assert all(supported([rectangle(4, 6), rectangle(0, 4), rectangle(6, 10)], every_order=True))


def test_overlapping_supports_form_a_union_not_a_sum():
    assert all(supported([rectangle(0, 7), rectangle(3, 10)], every_order=True))
    # Summed area exceeds the patch but the right-hand gap is still unsupported.
    assert not any(supported([rectangle(0, 6), rectangle(2, 8)], every_order=True))


def test_a_remaining_fragment_cannot_be_dropped():
    assert not any(supported([rectangle(4, 6), rectangle(0, 4)], every_order=True))


@pytest.mark.parametrize("return_api", ["shape_list", "compound", "none_when_empty"])
def test_boolean_return_shapes_preserve_physical_support(monkeypatch, return_api):
    """Exercise each supported return convention even on the locked dependency version."""
    original_cut = Shape.cut
    result_sizes = []

    def adapted_cut(self, *others):
        result = original_cut(self, *others)
        if isinstance(result, ShapeList):
            faces = [face for shape in result for face in shape.faces()]
        else:
            faces = list(result.faces()) if result is not None else []
        result_sizes.append(len(faces))
        if return_api == "shape_list":
            return ShapeList(faces)
        if return_api == "none_when_empty" and not faces:
            return None
        return Compound(faces)

    monkeypatch.setattr(Shape, "cut", adapted_cut)
    assert supported([rectangle(4, 6), rectangle(0, 4), rectangle(6, 10)]) == [True]
    assert supported([rectangle(4, 6), rectangle(0, 4)]) == [False]
    # A support that trims the patch without splitting it, so the kernel returns a single
    # face. The cases above reach this shape only by cutting a fragment with a support that
    # cannot touch it, which ``covered_patch`` now rejects on the bounding boxes; the return
    # convention still has to be exercised, so ask for it with abutting supports instead.
    assert supported([rectangle(0, 4), rectangle(4, 10)]) == [True]
    assert {0, 1, 2} <= set(result_sizes)


def test_a_hole_in_support_remains_uncovered():
    ring = Face(rectangle(0, 10).outer_wire(), [rectangle(4, 6, 4, 6).outer_wire()])
    assert supported([ring]) == [False]
    assert all(supported([ring, rectangle(4, 6, 4, 6)], every_order=True))


@pytest.mark.parametrize("face", [rectangle(0, 10).moved(Pos(0, 0, 1)), rectangle(20, 30)])
def test_unrelated_support_cannot_fill_patch(face):
    assert supported([face]) == [False]
