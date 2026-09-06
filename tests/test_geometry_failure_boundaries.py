"""Expected kernel failures refuse geometry; programming errors must remain visible."""

from types import SimpleNamespace

import pytest
from build123d import Box, Circle, Rectangle, extrude
from OCP.Standard import Standard_Failure

import quiddity.profiled_bores as bores
import quiddity.round_bottom_slots as slots


@pytest.mark.parametrize("error", [RuntimeError, ValueError, Standard_Failure])
def test_planar_face_kernel_failure_refuses(monkeypatch, error):
    def fail(_):
        raise error("construction failed")

    monkeypatch.setattr(slots, "Face", fail)
    assert slots._validated_planar_face(object()) is None


@pytest.mark.parametrize("error", [AssertionError, AttributeError, TypeError, KeyError])
def test_planar_face_programming_errors_propagate(monkeypatch, error):
    def fail(_):
        raise error("programming error")

    monkeypatch.setattr(slots, "Face", fail)
    with pytest.raises(error):
        slots._validated_planar_face(object())


@pytest.mark.parametrize("error", [RuntimeError, ValueError, Standard_Failure, AssertionError])
def test_wire_combine_boundary(monkeypatch, error):
    graph = SimpleNamespace(
        face=lambda _: SimpleNamespace(is_valid=True), edges=lambda _: [object()]
    )

    def fail(*args, **kwargs):
        raise error("combine failed")

    monkeypatch.setattr(slots.Wire, "combine", fail)
    if error is AssertionError:
        with pytest.raises(error):
            slots._region_boundary_wire(graph, frozenset({1}))
    else:
        assert slots._region_boundary_wire(graph, frozenset({1})) is None


@pytest.mark.parametrize("error", [RuntimeError, ValueError, Standard_Failure, AssertionError])
def test_double_d_prism_failure_boundary(monkeypatch, error):
    tool = extrude(Circle(5) & Rectangle(7.2, 20), amount=20, both=True)
    part = Box(40, 40, 10) - tool
    assert len(bores.recognise_double_d_bores(part)) == 1

    def fail(*args, **kwargs):
        raise error("prism failed")

    monkeypatch.setattr(bores.Solid, "extrude", fail)
    if error is AssertionError:
        with pytest.raises(error):
            bores.recognise_double_d_bores(part)
    else:
        assert bores.recognise_double_d_bores(part) == []


@pytest.mark.parametrize("volumes,accepted", [([], True), ([0.0, 0.0], True), ([1.0, 2.0], False)])
def test_double_d_boolean_fragments_are_measured_not_hidden_as_attribute_errors(
    monkeypatch, volumes, accepted
):
    tool = extrude(Circle(5) & Rectangle(7.2, 20), amount=20, both=True)
    part = Box(40, 40, 10) - tool
    monkeypatch.setattr(
        type(part), "__and__", lambda *_: [SimpleNamespace(volume=value) for value in volumes]
    )
    assert bool(bores.recognise_double_d_bores(part)) is accepted


@pytest.mark.parametrize("error", [RuntimeError, ValueError, Standard_Failure, AssertionError])
def test_declared_double_d_tool_failure_boundary(monkeypatch, error):
    tool = extrude(Circle(5) & Rectangle(7.2, 20), amount=20, both=True)
    assert bores.read_double_d_tool(tool)

    def fail(*args, **kwargs):
        raise error("prism failed")

    monkeypatch.setattr(bores.Solid, "extrude", fail)
    if error is AssertionError:
        with pytest.raises(error, match="prism failed"):
            bores.read_double_d_tool(tool)
    else:
        with pytest.raises(ValueError, match="needs one constant extrusion"):
            bores.read_double_d_tool(tool)
