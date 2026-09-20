from __future__ import annotations

import math
from types import SimpleNamespace

import pytest
from build123d import Box, Pos, Rot

from quiddity._adjacency import FaceGraph, SolidRef
from quiddity._claims import ClaimLedger
from quiddity._geometry import unit
from quiddity._passage_compat import (
    PassageCompatibilityView,
    _canonical_section,
    compatibility_view,
    grouping_from_view,
    passage_from_view,
    principal_projection,
)
from quiddity._section_passages import (
    _BodyAdapter,
    _canonical_run,
    _end_slab,
    _face_interval,
    _material_fraction,
    _ordered_cycle,
    _pair_line,
    _void_and_open,
)
from quiddity._sections import LocalFrame
from quiddity.passages import (
    Passage,
    PassageEnds,
    PassageFrame,
    PassageSection,
    PassageSectionVertex,
    SectionPassage,
    _proposal_legacy_projection,
    _same_legacy_passage_geometry,
    recognise_passages,
)


class _Edges:
    def __init__(self, edges=()):
        self._edges = edges

    def shared_edges(self, left, right):
        return self._edges


class _BadEndpoint:
    geom_type = SimpleNamespace(name="LINE")

    def tangent_at(self):
        return SimpleNamespace(normalized=lambda: SimpleNamespace(X=0.0, Y=0.0, Z=1.0))

    def position_at(self, at):
        raise RuntimeError("closed kernel boundary")


class _LineEdge:
    geom_type = SimpleNamespace(name="LINE")

    def __init__(self, start, end, tangent=(0.0, 0.0, 1.0)) -> None:
        self._points = (start, end)
        self._tangent = tangent

    def tangent_at(self):
        x, y, z = self._tangent
        return SimpleNamespace(normalized=lambda: SimpleNamespace(X=x, Y=y, Z=z))

    def position_at(self, at):
        x, y, z = self._points[int(at)]
        return SimpleNamespace(X=x, Y=y, Z=z)


def _frame() -> PassageFrame:
    return PassageFrame(
        (0.0, 0.0, 0.0),
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 0.0),
        (0.0, 1.0, 0.0),
    )


def _section() -> PassageSection:
    return PassageSection(
        (
            PassageSectionVertex((-1.0, -1.0), 0.0),
            PassageSectionVertex((1.0, -1.0), 0.0),
            PassageSectionVertex((0.0, 2.0), 0.0),
        )
    )


@pytest.mark.parametrize(
    "value",
    (
        object(),
        SimpleNamespace(geom_type=SimpleNamespace(name="CIRCLE")),
        SimpleNamespace(
            geom_type=SimpleNamespace(name="LINE"),
            tangent_at=lambda: SimpleNamespace(
                normalized=lambda: (_ for _ in ()).throw(ValueError())
            ),
        ),
    ),
)
def test_canonical_run_refuses_unsupported_or_malformed_edges(value) -> None:
    assert _canonical_run(value) is None


def test_pair_line_and_face_interval_closed_refusals() -> None:
    neutral = LocalFrame.canonical((0.0, 0.0, 1.0), (0.0, 0.0, 0.0))
    assert _pair_line(_Edges(), object(), object(), neutral) is None  # type: ignore[arg-type]
    assert _pair_line(_Edges((_BadEndpoint(),)), object(), object(), neutral) is None  # type: ignore[arg-type]
    missing = SimpleNamespace(face=lambda node: SimpleNamespace(vertices=lambda: ()))
    failing = SimpleNamespace(
        face=lambda node: SimpleNamespace(vertices=lambda: (_ for _ in ()).throw(RuntimeError()))
    )
    assert _face_interval(missing, object(), neutral.run) is None  # type: ignore[arg-type]
    assert _face_interval(failing, object(), neutral.run) is None  # type: ignore[arg-type]


def test_pair_line_refuses_nonparallel_and_noncollinear_shared_occurrences() -> None:
    neutral = LocalFrame.canonical((0.0, 0.0, 1.0), (0.0, 0.0, 0.0))
    nonparallel = _LineEdge((1.0, 2.0, 0.0), (1.0, 2.0, 1.0), (1.0, 0.0, 0.0))
    assert _pair_line(_Edges((nonparallel,)), object(), object(), neutral) is None  # type: ignore[arg-type]

    separated = (
        _LineEdge((1.0, 2.0, 0.0), (1.0, 2.0, 1.0)),
        _LineEdge((1.01, 2.0, 1.0), (1.01, 2.0, 2.0)),
    )
    assert _pair_line(_Edges(separated), object(), object(), neutral) is None  # type: ignore[arg-type]


def test_material_fraction_handles_all_closed_intersection_shapes() -> None:
    probe = SimpleNamespace(volume=10.0)
    assert _material_fraction(SimpleNamespace(intersect=lambda item: None), probe) == 0.0
    assert (
        _material_fraction(
            SimpleNamespace(intersect=lambda item: SimpleNamespace(volume=2.0)), probe
        )
        == 0.2
    )
    assert (
        _material_fraction(
            SimpleNamespace(
                intersect=lambda item: (SimpleNamespace(volume=1.0), SimpleNamespace(volume=2.0))
            ),
            probe,
        )
        == 0.3
    )


def test_end_and_void_kernel_failures_are_closed() -> None:
    frame = LocalFrame.canonical((0.0, 0.0, 1.0), (0.0, 0.0, 0.0))
    section = _section()
    neutral = SimpleNamespace(
        boundary=tuple(SimpleNamespace(point=vertex.point) for vertex in section.boundary)
    )
    with pytest.raises(ValueError, match="too thin"):
        _end_slab(frame, 0.0, 1.0, 1e-6, neutral)  # type: ignore[arg-type]
    assert not _void_and_open(
        SimpleNamespace(intersect=lambda item: (_ for _ in ()).throw(RuntimeError())),
        frame,
        (0.0, 10.0),
        neutral,  # type: ignore[arg-type]
    )


def test_ordered_cycle_refuses_a_branch_instead_of_choosing_by_order() -> None:
    a, b, c, d = object(), object(), object(), object()
    adjacency = {a: {b}, b: {a, c, d}, c: {b}, d: {b}}
    with pytest.raises(ValueError, match="simple cycle"):
        _ordered_cycle((a, b, c, d), adjacency, {})  # type: ignore[arg-type]


def test_body_adapter_revalidates_the_occurrence_mapping() -> None:
    adapter = _BodyAdapter()
    occurrence = SimpleNamespace(body=object())
    with pytest.raises(ValueError, match="does not match"):
        adapter.validate(object(), occurrence)  # type: ignore[arg-type]


def test_cycle_proposal_properties_and_incomplete_wall_spans_refuse(monkeypatch) -> None:
    import quiddity._section_passages as module
    from tests.test_section_passages import _square

    part = _square()
    proposal = module.section_ring_proposals(part, FaceGraph(part))[0]
    assert proposal.ends.low_capped is False
    assert proposal.ends.high_capped is False

    monkeypatch.setattr(module, "_enclosure_proposals", lambda *_args: ())
    monkeypatch.setattr(module, "_face_interval", lambda *_args: None)
    assert module.section_ring_proposals(part, FaceGraph(part)) == ()


def test_cycle_proposal_refuses_disagreeing_complete_wall_spans(monkeypatch) -> None:
    import quiddity._section_passages as module
    from tests.test_section_passages import _square

    part = _square()
    graph = FaceGraph(part)
    first = module.section_ring_proposals(part, graph)[0].nodes[0]
    monkeypatch.setattr(module, "_enclosure_proposals", lambda *_args: ())
    monkeypatch.setattr(
        module,
        "_face_interval",
        lambda _graph, node, _run: (0.0, 10.0 if node is first else 11.0),
    )
    assert module.section_ring_proposals(part, graph) == ()


def test_solid_shape_revalidates_ordinal_identity_and_closed_membership() -> None:
    graph = FaceGraph(Box(2, 2, 2))
    solid = graph.common_valid_solid(graph.nodes)
    assert solid is not None
    assert graph.solid_shape(solid).is_valid

    copied = SolidRef(solid.ordinal)
    with pytest.raises(ValueError, match="not issued"):
        graph.solid_shape(copied)
    graph._issued_solid_refs[copied] = solid.ordinal
    with pytest.raises(ValueError, match="identity changed"):
        graph.solid_shape(copied)

    object.__setattr__(solid, "ordinal", 99)
    with pytest.raises(ValueError, match="not issued"):
        graph.solid_shape(solid)
    object.__setattr__(solid, "ordinal", 0)

    assert graph._closed_solids is not None
    graph._closed_solids = frozenset()
    with pytest.raises(ValueError, match="no longer maps"):
        graph.solid_shape(solid)


@pytest.mark.parametrize(
    "factory",
    (
        lambda: PassageFrame([], (0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        lambda: PassageFrame((False, 0.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        lambda: PassageFrame(
            (math.nan, 0.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)
        ),
        lambda: PassageFrame((0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        lambda: PassageFrame((0.0, 0.0, 0.0), (0.0, 0.0, 2.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        lambda: PassageFrame((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), (0.0, 0.0, 1.0), (0.0, 1.0, 0.0)),
        lambda: PassageFrame((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, -1.0, 0.0)),
        lambda: PassageFrame((0.0, 0.0, 0.001), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
        lambda: PassageSectionVertex((0.0, 0.0), False),
        lambda: PassageSectionVertex((0.0, 0.0), math.inf),
        lambda: PassageSectionVertex((0.0, 0.0), 1e-13),
        lambda: PassageSection((object(),)),
        lambda: PassageSection(()),
        lambda: PassageEnds(False, 0),
        lambda: SectionPassage(object(), (0.0, 1.0), _section(), PassageEnds(False, False)),
        lambda: SectionPassage(_frame(), (False, 1.0), _section(), PassageEnds(False, False)),
        lambda: SectionPassage(_frame(), (0.0, 1.0), object(), PassageEnds(False, False)),
        lambda: SectionPassage(_frame(), (0.0, 1.0), _section(), PassageEnds(True, False)),
    ),
)
def test_public_schema_closed_refusal_roster(factory) -> None:
    with pytest.raises(ValueError):
        factory()


def test_section_centring_and_legacy_ledger_transition_refuse() -> None:
    with pytest.raises(ValueError, match="origin-centred"):
        PassageSection(
            (
                PassageSectionVertex((1.0, 1.0), 0.0),
                PassageSectionVertex((3.0, 1.0), 0.0),
                PassageSectionVertex((2.0, 4.0), 0.0),
            )
        )

    part = Box(10, 10, 10)
    ledger = ClaimLedger(FaceGraph(part))
    with pytest.raises(RuntimeError, match="recognise_passages"):
        recognise_passages(part, ledger=ledger)


@pytest.mark.parametrize(
    "view",
    (
        lambda: PassageCompatibilityView("bad", (), 3, 1.0, (0.0, 0.0, 0.0), 0, True),
        lambda: PassageCompatibilityView("x", None, 3, 1.0, (0.0, 0.0, 0.0), 0, True),
        lambda: PassageCompatibilityView("x", (), 3, None, (0.0, 0.0, 0.0), 0, True),
        lambda: PassageCompatibilityView("x", (), 3, 1.0, (0.0, 0.0, 0.0), 0, False),
        lambda: PassageCompatibilityView(None, None, 3, None, None, None, False),
    ),
)
def test_compatibility_fact_refusal_roster(view) -> None:
    with pytest.raises(ValueError):
        view()


def test_compatibility_projection_closed_absence_and_construction() -> None:
    assert compatibility_view(None, eligible=False).eligible is False
    with pytest.raises(ValueError, match="no principal"):
        compatibility_view(None, eligible=True)
    absent = PassageCompatibilityView(None, None, None, None, None, None, False)
    assert grouping_from_view(absent) is None
    with pytest.raises(ValueError, match="no legacy"):
        passage_from_view(absent, Passage)
    assert (
        principal_projection(
            (0.0, 0.0, 0.0),
            (0.5, 0.5, 0.0),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 1.0),
            ((0.0, 0.0), (1.0, 0.0), (0.0, 1.0)),
        )
        is None
    )
    assert (
        principal_projection(
            (0.0, 0.0, 0.0),
            (0.0, 0.0, 1.0),
            (1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, 1.0),
            ((0.0, 0.0), (1.0, 0.0), (2.0, 0.0)),
        )
        is None
    )
    assert _canonical_section(((0.0, 0.0), (1.0, 0.0))) is None
    assert _canonical_section(((0.0, 0.0), (1.0, 0.0), (2.0, 0.0))) is None


def test_unit_refuses_degenerate_vector() -> None:
    with pytest.raises(ValueError, match="degenerate"):
        unit((0.0, 0.0, 0.0))


def test_legacy_compatibility_mismatch_refuses_before_publication(monkeypatch) -> None:
    import quiddity.passages as module
    from quiddity._adjacency import FaceGraph
    from tests.test_section_passages import _square

    part = _square()
    graph = FaceGraph(part)
    ((legacy, nodes),) = module._legacy_roster(part, graph)
    monkeypatch.setattr(
        module,
        "_legacy_roster",
        lambda *_args: [
            (
                Passage(legacy.axis, legacy.sides, legacy.length + 1, legacy.at, legacy.section),
                nodes,
            )
        ],
    )
    with pytest.raises(ValueError, match="historical legacy"):
        module._discover_section_passages(part, graph, None)


def test_legacy_compatibility_accepts_only_equivalent_section_traversal() -> None:
    section = ((0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0))
    expected = Passage("Z", 4, 10.0, (1.0, 0.5, 5.0), section)

    assert _same_legacy_passage_geometry(
        Passage("Z", 4, 10.0, expected.at, (*section[2:], *section[:2])), expected
    )
    assert _same_legacy_passage_geometry(
        Passage("Z", 4, 10.0, expected.at, tuple(reversed(section))), expected
    )
    assert not _same_legacy_passage_geometry(
        Passage("Z", 4, 10.001, expected.at, section), expected
    )


def test_equal_rich_records_on_distinct_wall_sets_refuse(monkeypatch) -> None:
    import quiddity.passages as module
    from quiddity._adjacency import FaceGraph
    from quiddity._section_passages import section_ring_proposals

    part = Box(80, 40, 20)
    part = part - Pos(-20, 0, 0) * Box(8, 8, 60) - Pos(20, 0, 0) * Box(12, 6, 60)
    part = Rot(17, 23, 31) * part
    graph = FaceGraph(part)
    first, second = section_ring_proposals(part, graph)
    competing = type(second)(first.occurrence, second.nodes, second.solid, second.body_adapter)
    monkeypatch.setattr(module, "section_ring_proposals", lambda *_args: [first, competing])
    with pytest.raises(ValueError, match="equal section passage"):
        module._discover_section_passages(part, graph, None)


def test_proposal_projection_refuses_arc_boundary(monkeypatch) -> None:
    from quiddity._adjacency import FaceGraph
    from quiddity._section_passages import section_ring_proposals
    from tests.test_section_passages import _square

    part = _square()
    (proposal,) = section_ring_proposals(part, FaceGraph(part))
    curved = type(proposal.section)(
        tuple(
            type(vertex)(vertex.point, 0.25 if at == 0 else vertex.bulge)
            for at, vertex in enumerate(proposal.section.boundary)
        )
    )
    object.__setattr__(proposal.occurrence, "section", curved)
    assert _proposal_legacy_projection(proposal) is None
