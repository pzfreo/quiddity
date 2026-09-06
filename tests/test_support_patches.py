"""Regression checks for exact support union and supported kernel return shapes."""
import pytest
from build123d import Face, Pos, Rectangle, ShapeList

from quiddity._support_patches import covered_patch


@pytest.mark.parametrize("return_kind", ["native", "fragments", "none_when_empty"])
def test_exact_support_union_handles_kernel_result_forms(monkeypatch, return_kind):
    original = Face.cut

    def adapted(face, *others, **kwargs):
        result = original(face, *others, **kwargs)
        if return_kind == "native":
            return result
        if isinstance(result, ShapeList):
            faces = [f for shape in result for f in shape.faces()]
        else:
            faces = [] if result is None else list(result.faces())
        if return_kind == "none_when_empty" and not faces:
            return None
        return ShapeList(faces)

    monkeypatch.setattr(Face, "cut", adapted)
    patch = Rectangle(10, 10).face()
    middle = Rectangle(2, 10).face()
    left = (Pos(-3, 0, 0) * Rectangle(4, 10)).face()
    right = (Pos(3, 0, 0) * Rectangle(4, 10)).face()
    assert covered_patch(patch, (middle, left, right))
    assert not covered_patch(patch, (middle, left))
    assert not covered_patch(patch, (left, left, left))
    assert not covered_patch(patch, ())


def test_support_union_preserves_a_small_real_gap():
    patch = Rectangle(10, 10).face()
    left = (Pos(-2.5005, 0, 0) * Rectangle(4.999, 10)).face()
    right = (Pos(2.5005, 0, 0) * Rectangle(4.999, 10)).face()
    assert not covered_patch(patch, (left, right))
