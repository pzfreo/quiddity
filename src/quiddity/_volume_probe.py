# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Policy-neutral volumetric evidence for axis-aligned candidate regions.

**A probe is asked about material, so it is asked of solids.** An imported STEP part is often a
:class:`~build123d.Compound` holding the machined solid *beside* loose construction geometry --
``nist_ctc_01`` is one solid plus a compound of 78 datum edges, ``nist_ctc_03`` has 87 of them,
``nist_ctc_02`` adds an open shell. build123d's ``Compound.intersect`` distributes the
intersection over every child, so a probe of ``ctc_01`` built **79** boolean operations to answer
a question only one of them could contribute to: measured, one recognition of that part ran 2154
``BRepAlgoAPI`` builds, 1772 of them inside volume probes and all but 56 of those on loose edges.

Those could not change an answer, because they could not produce volume:

- :func:`intersection_volume` sums ``.volume`` over the fragments ``Shape.intersect`` returns,
  and that list has been through ``ShapeList.expand()``, which dissolves compounds, wires and
  *shells* -- so a fragment is only ever a ``Vertex``, ``Edge``, ``Face`` or ``Solid``. The
  ``volume`` of the first three is literally ``return 0.0`` in build123d; a manifold ``Shell``
  is the one non-solid shape with a volume, and expansion means one never reaches the sum.
- A fragment can only be a ``Solid`` if it came from a ``Solid`` child. Every child intersection
  is one ``BRepAlgoAPI_Common``, whose result has the lesser dimension of its arguments, so a
  child of dimension two or less yields geometry of dimension two or less.

So the sum over *all* children equals the sum over the solid children, and
:func:`probe_solids` narrows every probe in this module to those. The sum is over all of them:
a multi-solid part keeps contributing every body it has, exactly as the distribution did.

**The cheapest boolean is the one that is not run.** With the loose children gone, the booleans
that remain are the largest single cost of a recognition: 208 of them across four NIST parts
were 3.15 s of a 13.1 s profile, about 15 ms each, and build123d already runs each one with
``SetRunParallel``, so the kernel operation itself cannot be made cheaper. Measured over the
whole 87-part corpus, a run builds 2061 of them, and two independent measurements say most of
them need not be asked at all.

- **523 (25%) repeat an earlier probe** -- the same probe geometry asked of the same shape again
  within one run, because two families ask the same region the same question. :func:`probe_volume`
  memoises its answer on the run's cache under an *exact* description of the probe (see
  :func:`_describe`); a repeat returns the identical float.
- Classifying all 2061 by geometry rather than by repetition: **489 (24%) have a probe box
  disjoint from the solid's box**; **353 more (17%) overlap no *face* box of the solid and lie
  entirely outside it** -- both answer ``0.0``; **277 (13%) overlap no face box and lie entirely
  inside**, where the common is the probe and the answer is the probe's own volume. The
  remaining 938 straddle the material and still run the boolean.

Together they take a census of ``nist_ctc_01`` from 56 probe booleans to 27.

The two geometric short-circuits are exact, not tolerant approximations:

- ``Shape.bounding_box(optimal=False)`` is a *superset* of the shape -- OCCT's loose box, padded
  outwards by the box gap -- which is what makes a disjointness test on it conservative. It is
  used here only to prove that two shapes cannot meet, never as a measurement, so the repository
  rule that a *measured* box keeps ``optimal=True`` is untouched (the solid's own box still comes
  from the run cache's ``optimal=True`` query).
- If the probe's box meets no face box, it meets no part of the solid's boundary -- and a box is
  convex, so it is connected, so *the whole box* lies in the material's interior or entirely
  outside it. The probe is a subset of its own box and follows it, voids, non-convexity and
  disconnection included. **Any** point of the box is therefore a witness for all of it, and the
  decision is only as good as the classifier that reads one.

That last sentence is why :func:`_shortcut` classifies the box's **centre** as well as every
vertex of the probe, and takes an answer only when all of them agree. Unanimity is not a step in
the proof -- the proof is finished once the boxes are apart -- it is a guard against
``BRepClass3d_SolidClassifier`` not being a perfect oracle, and this corpus shows it is not: two
probes in it (``nist_ftc_08``, ``nist_ctc_04``) have a corner the classifier calls ``IN`` while
``BRepExtrema`` puts every corner ``1e-6`` *outside* the shell and the boolean answers ``0.0``,
reproducibly and at every tolerance from ``0`` to ``1e-6``. That configuration is one this module
manufactures on purpose -- ``inset=COORD_FLOOR`` stands a probe off a pocket wall by exactly
``1e-6`` -- and a probe's vertices are the worst sample it has, sitting on its own boundary and so
as near the solid's as the probe ever comes. The box centre stands off by half the probe's
smallest dimension instead, six orders of magnitude further; over the corpus it is right on all
435 cases that reach the classifier, including both the ones a vertex gets wrong.

The exactness of the inside case (that the common of a contained probe reports the probe's own
volume, to the last bit) is not a claim about OCCT that this module can prove, so it was
measured: over all 87 corpus parts, every one of the 277 contained probes had
``intersection_volume(body.intersect(probe)) == probe.volume`` exactly. Running every
short-circuit and its boolean side by side agreed on all 1100 short-circuit decisions a corpus
dump takes and all 12035 the test suite takes, and recomputing every memo hit agreed on all 523
of the dump's; one of the suite's 5197 came back a single unit in the last place apart, which is
the kernel and not the key -- see :func:`_describe`.

That is a measurement of one version of OCCT and build123d, not a theorem: **re-run the sweep
whenever either moves.** Nothing here has a switch for it, which is as it should be -- patch
:func:`_measure` to take both answers and compare them with ``==``, run a corpus dump and the
test suite, and throw the patch away.

**A probe reaches all of this only when it is given a run to share**, because the per-solid
values it reads cost far more to build than the boolean they save -- see :func:`probe_volume`.
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import NamedTuple, Protocol, cast

from build123d import Box, Compound, Pos, Solid
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_FORWARD, TopAbs_IN, TopAbs_OUT, TopAbs_SOLID

from quiddity._solid_properties import SolidProperties, SolidPropertyOwner, solid_properties
from quiddity._typing import Bounds, Part, Vector3

#: OCCT cannot construct a volumetric probe at or below this coordinate extent.
PRISM_PROBE_FLOOR = 1e-6

#: Names under which a run's :class:`SolidProperties` holds this module's values.
#:
#: ``derived`` documents that two callers sharing a name are sharing a value on purpose, so the
#: names are namespaced by the module that owns the meaning rather than left in a flat space.
#: The classifier is a sibling of the one :mod:`quiddity._bevel` files under its own name rather
#: than the same entry: the module seams (``tests/test_architecture.py``) keep this module's
#: imports to :mod:`quiddity._solid_properties` and :mod:`quiddity._typing`, and reaching into a
#: recogniser for the string would be a worse dependency than one extra classifier per solid.
_PROBE_SOLIDS = "_volume_probe.solids"
_FACE_BOUNDS = "_volume_probe.face_bounds"
_CLASSIFIER = "_volume_probe.solid_classifier"
_PROBE_VOLUME = "_volume_probe.volume"

#: Tolerance the point classifier is asked at, and why it is not the ``1e-6`` :mod:`quiddity._bevel`
#: uses. A probe reaches the classifier only once its box has been proved not to meet any face box,
#: and both boxes are padded outwards, so a *vertex* of the probe stands clear of the boundary by
#: at least the two gaps -- but only by that. Asking at ``1e-6`` calls a legitimate inset off a
#: wall ``ON`` and throws the decision away; asking at the kernel's own confusion tolerance keeps
#: the answer crisp exactly where the boolean's is. Across the corpus no probe that reached this
#: classifier returned ``ON`` -- but two returned a wrong ``IN`` at every tolerance from ``0``
#: upwards, which is a thing no tolerance fixes and the box centre is sampled for.
_CLASSIFIER_TOLERANCE = 1e-7

#: An axis-aligned box as ``(min x, min y, min z, max x, max y, max z)``.
_Box = tuple[float, float, float, float, float, float]


class _VolumeValue(Protocol):
    @property
    def volume(self) -> float: ...


def intersection_volume(result: object) -> float:
    """Normalize empty, single-shape and fragmented boolean volumes without policy.

    Unexpected result types and kernel errors propagate to the caller's proof boundary.
    No tolerance, absolute value, clamping or fragment selection is applied here.
    """
    if result is None:
        return 0.0
    if hasattr(result, "volume"):
        return float(cast(_VolumeValue, result).volume)
    return sum(float(shape.volume) for shape in cast(Iterable[_VolumeValue], result))


def _solids_of(part: Part) -> tuple[Part, ...]:
    return tuple(part.solids())


def probe_solids(
    part: Part, *, properties: SolidProperties | SolidPropertyOwner | None = None
) -> tuple[Part, ...]:
    """The bodies of *part* a volumetric probe is measured against.

    A part that is already one solid -- what most call sites hold, having come through
    ``graph.solid_shape(owner)`` -- is its own answer, and so is anything that is not a
    build123d ``Compound`` at all: the standalone and test callers that pass a bare object
    with an ``intersect`` method keep measuring exactly what they measured before.

    Only a compound fans out, and only a compound is asked to name its solids. *properties* is
    the run's cache (see :mod:`quiddity._solid_properties`), reached from the ``graph`` the
    caller already holds; without one the traversal is simply repeated, as every other value in
    that module is when a recogniser is called standalone.
    """

    if not isinstance(part, Compound):
        return (part,)
    return solid_properties(properties).derived(_PROBE_SOLIDS, part, _solids_of)


class _ProbeGeometry(NamedTuple):
    """Everything the short-circuits and the memo need to know about one probe."""

    #: The probe's loose bounding box -- a superset of it, which is what makes the tests below
    #: conservative, and convex, which is what makes one point of it speak for all of it.
    box: _Box
    #: The points the classifier is asked about: every vertex of the probe, and the centre of the
    #: box. Any one of them would settle it if the classifier were exact; they are all asked
    #: because it is not. The centre is the one that is nowhere near the boundary.
    points: tuple[Vector3, ...]
    #: ``probe.volume``, which is the answer when the probe is contained in the material.
    volume: float
    #: An exact description of the probe -- see :func:`_describe`.
    identity: str


def _box_of(bounds: Bounds) -> _Box:
    low, high = bounds.min, bounds.max
    return (low.X, low.Y, low.Z, high.X, high.Y, high.Z)


def _apart(one: _Box, other: _Box) -> bool:
    """Whether two boxes share no volume. Boxes that merely touch count as apart.

    That is deliberate and it is still exact: a common of shapes meeting in a plane, a line or a
    point has no volume, and :func:`intersection_volume` sums volumes.
    """

    return (
        one[3] <= other[0]
        or other[3] <= one[0]
        or one[4] <= other[1]
        or other[4] <= one[1]
        or one[5] <= other[2]
        or other[5] <= one[2]
    )


def _describe(probe: Solid) -> _ProbeGeometry | None:
    """Measure the probe once, and name it exactly -- or decline to name it at all.

    The memo's key has to distinguish two probes that would get different answers, and nothing
    weaker than exact floats will do: no rounding, no tolerance. The name here is the probe's
    type, its orientation, its face count and *every vertex coordinate*, each through ``repr``,
    which round-trips a float exactly and so gives distinct floats distinct names.

    ``volume`` is in the name as well, and it is the half found by measurement rather than by
    argument. Two probes built by different callers over the same eight corners -- one a ``Box``,
    one an extrusion -- can carry surface parameterisations that differ, and OCCT's boolean then
    answers them a few units in the last place apart. Eleven probe pairs in the corpus did
    exactly that. Keying on the volume as well separates them, and with it every one of the 523
    repeats in a corpus dump agreed with the answer it was memoising.

    Over the whole test suite one of 5197 did not, by a single unit in the last place -- and the
    cause is not the key. ``BRepAlgoAPI_Common`` is run with ``SetRunParallel``, and asking the
    kernel the *identical* question twice about the *same* two shapes disagrees 80 times in 12160
    over that suite, once by 0.06 mm3 in 17428914. So the boolean does not have one answer to
    memoise; the memo picks the first of them and holds it, which makes a run more repeatable than
    it was, not less.

    ``None`` means "do not memoise and do not short-circuit": a probe with no live shape behind
    it -- one of the duck-typed stubs the boundary tests hand these helpers -- has no exact key
    and no geometry to reason about, and must reach the boolean exactly as it did before.

    **A reversed probe is one of those.** ``Solid(probe.wrapped.Reversed())`` is the complement of
    the region it looks like: its ``volume`` is ``-V``, and ``BRepAlgoAPI_Common`` intersects with
    everything *outside* it, so the contained case would answer ``-64.0`` where the kernel answers
    ``+7936`` and ``material_fraction`` would swing from ``-124`` to ``+1`` -- past every gate that
    reads it. Nothing builds one today, exactly as nothing reverses a solid in
    :func:`quiddity._bevel._material_at`; this is the same line that module holds for the same
    reason, and an orientation that is not ``TopAbs_FORWARD`` goes to the kernel, which has never
    been confused by one.

    The ``except`` is narrow on purpose. It catches what a *stub* raises when asked for geometry it
    does not have; OCP's ``Standard_Failure`` is deliberately not in it, so a kernel error inside
    ``probe.volume`` or ``probe.bounding_box`` propagates to the caller's proof boundary as this
    module's opening docstring says kernel errors do, rather than being quietly reclassified as
    "this probe cannot be named".
    """

    wrapped = getattr(probe, "wrapped", None)
    if wrapped is None:
        return None
    try:
        orientation = wrapped.Orientation()
        if orientation != TopAbs_FORWARD:
            return None
        corners = tuple(sorted(cast(Vector3, tuple(vertex)) for vertex in probe.vertices()))
        faces = len(probe.faces())
        volume = float(probe.volume)
        box = _box_of(probe.bounding_box(optimal=False))
    except (AssertionError, AttributeError, TypeError, ValueError):
        return None
    if not corners:
        return None
    centre = cast(Vector3, tuple((box[i] + box[i + 3]) / 2 for i in range(3)))
    identity = repr((type(probe).__name__, orientation, faces, corners, volume))
    return _ProbeGeometry(box, (*corners, centre), volume, identity)


def _face_bounds(solid: Part) -> tuple[_Box, ...]:
    """The loose box of every face of *solid* -- the run's one walk of its boundary."""

    return tuple(_box_of(face.bounding_box(optimal=False)) for face in solid.faces())


def _classifier(solid: Part) -> BRepClass3d_SolidClassifier:
    """Load one solid into a point classifier -- the expensive half of a material question."""

    return BRepClass3d_SolidClassifier(solid.wrapped)


def _shortcut(body: Part, geometry: _ProbeGeometry, memo: SolidProperties) -> float | None:
    """The boolean's answer, when it can be had without the boolean. ``None`` means run it.

    Both proofs need the body to be a real solid: the boxes have to bound something, and the
    point classifier is defined on solids. Anything else -- a stub with an ``intersect`` method,
    a shape narrowed to something that is not a solid -- takes the boolean, as before.
    """

    wrapped = getattr(body, "wrapped", None)
    if wrapped is None or wrapped.ShapeType() != TopAbs_SOLID:
        return None

    # Nothing to intersect: the probe stands outside the body's own box.
    if _apart(geometry.box, _box_of(memo.bounding_box(body))):
        return 0.0

    # The probe's box meets no face, so it meets no part of the body's boundary; a box is convex
    # and so connected, so all of it -- and the probe inside it -- is on one side. Which side is
    # then a question about one point, and every point of the box answers it. They are all asked
    # because the classifier is not an oracle: see the module docstring for the two corpus probes
    # where a vertex says IN of a region the kernel and BRepExtrema both call empty.
    if any(
        not _apart(geometry.box, face) for face in memo.derived(_FACE_BOUNDS, body, _face_bounds)
    ):
        return None
    classifier = memo.derived(_CLASSIFIER, body, _classifier)
    states = set()
    for point in geometry.points:
        classifier.Perform(gp_Pnt(*point), _CLASSIFIER_TOLERANCE)
        states.add(classifier.State())
    if states == {TopAbs_OUT}:
        return 0.0
    if states == {TopAbs_IN}:
        return geometry.volume
    return None


def _measure(
    part: Part, probe: Solid, geometry: _ProbeGeometry | None, memo: SolidProperties
) -> float:
    """One probe against every body of *part*, short-circuited per body where it can be."""

    total = 0.0
    for body in probe_solids(part, properties=memo):
        shortcut = None if geometry is None else _shortcut(body, geometry, memo)
        total += intersection_volume(body.intersect(probe)) if shortcut is None else shortcut
    return float(total)


def probe_volume(
    part: Part,
    probe: Solid,
    *,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> float:
    """The volume *probe* shares with the material of *part* -- its solids, and all of them.

    The answer is memoised on the run's cache under the shape asked about and an exact
    description of the probe, so the quarter of a run's probes that repeat an earlier question
    return that question's identical float rather than asking the kernel again. Both halves of
    the key matter: :meth:`SolidProperties.derived` pins the shape and separates orientations,
    and :func:`_describe` refuses to key anything it cannot name exactly.

    **A probe with no cache to share takes the plain boolean**, which is the same code this
    function was before. Both short-circuits are answered out of per-solid values -- the solid's
    box, the box of each of its faces, its point classifier -- that cost far more to build than
    one boolean and only pay for themselves when a whole run's probes read them. Handed a cache
    of its own, a probe would rebuild all three and hand them straight back to the garbage
    collector: measured, that turned a 30 s corpus into an 81 s one. So the caller that has a run
    passes it (every recogniser does, through ``properties=``), and the caller that has none --
    a public recogniser called on its own -- asks the kernel exactly what it asked before.
    """

    if properties is None:
        return _measure(part, probe, None, SolidProperties())
    memo = solid_properties(properties)
    geometry = _describe(probe)
    if geometry is None or getattr(part, "wrapped", None) is None:
        return _measure(part, probe, geometry, memo)
    return memo.derived(
        f"{_PROBE_VOLUME}:{geometry.identity}",
        part,
        lambda target: _measure(target, probe, geometry, memo),
    )


def material_fraction(
    part: Part,
    probe: Solid,
    *,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> float:
    """Measure occupied fraction of the supplied probe; callers own admission policy."""
    return probe_volume(part, probe, properties=properties) / float(probe.volume)


def prism_material_fraction(
    spans: dict[str, tuple[float, float]],
    part: Part,
    *,
    inset: float,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> float:
    """Return the fraction of an inset axis-aligned prism occupied by ``part``.

    This measures geometry only. Consumers separately own whether they require exact emptiness
    or permit a named material fraction, and they supply their own inset policy.
    """

    size: dict[str, float] = {}
    centre: dict[str, float] = {}
    for axis, (low, high) in spans.items():
        axis_inset = min(inset, (high - low) / 4)
        size[axis] = (high - low) - 2 * axis_inset
        centre[axis] = (low + high) / 2
    if min(size.values()) <= 0:
        raise ValueError("prism spans must have positive extent")
    if min(size.values()) <= PRISM_PROBE_FLOOR:
        # Nominally disjoint face bounds can overlap by a final bit while remaining below the
        # kernel's constructible-solid floor. Such a sliver cannot prove an empty region.
        return 1.0
    probe = Pos(centre["x"], centre["y"], centre["z"]) * Box(size["x"], size["y"], size["z"])
    occupied = probe_volume(part, probe, properties=properties)
    return float(occupied / (size["x"] * size["y"] * size["z"]))


def prism_is_empty(
    spans: dict[str, tuple[float, float]],
    part: Part,
    *,
    inset: float,
    properties: SolidProperties | SolidPropertyOwner | None = None,
) -> bool:
    """Whether the inset prism has exactly zero volumetric intersection with ``part``."""

    return prism_material_fraction(spans, part, inset=inset, properties=properties) == 0.0
