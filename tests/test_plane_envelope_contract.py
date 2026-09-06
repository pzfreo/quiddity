"""Kernel-free observed two-plane end contract (ADR 0024)."""

import json
from dataclasses import replace

import pytest

import quiddity
from quiddity._section_recess import (
    ClosedSectionProfile,
    CylindricalEndSurface,
    OpenSectionProfile,
    PlanarEndSurface,
    PlanarEndTerm,
    PlanarEnvelopeEndSurface,
    SectionEnd,
    SectionRecessEnds,
    SectionRecessGeometry,
)
from quiddity.passages import PassageFrame, PassageSectionVertex


def geometry(*, reverse=False):
    sign = -1 if reverse else 1
    envelope = PlanarEnvelopeEndSurface(
        "plane_envelope",
        "max" if reverse else "min",
        tuple(sorted((PlanarEndTerm(sign * 10, (0, 0)), PlanarEndTerm(sign * 10, (0.2, 0))))),
    )
    curved = SectionEnd("open", envelope)
    flat = SectionEnd("open")
    return SectionRecessGeometry(
        "section_recess",
        PassageFrame((0, 0, 0), (0, 0, 1), (1, 0, 0), (0, 1, 0)),
        (-10, 0) if reverse else (0, 10),
        ClosedSectionProfile(
            "closed",
            tuple(PassageSectionVertex(p, 0) for p in ((-1, -1), (1, -1), (1, 1), (-1, 1))),
        ),
        SectionRecessEnds(curved, flat) if reverse else SectionRecessEnds(flat, curved),
    )


@pytest.mark.parametrize("reverse", [False, True])
def test_convex_roof_and_reversed_lower_envelope(reverse):
    record = geometry(reverse=reverse)
    surface = (record.ends.low if reverse else record.ends.high).surface
    assert surface.height((0, 0)) == (-10 if reverse else 10)
    assert surface.height((-1, 0)) == (-10 if reverse else 9.8)


def test_oblique_off_centre_crease_has_two_active_terms():
    record = geometry()
    surface = PlanarEnvelopeEndSurface(
        "plane_envelope",
        "min",
        (
            PlanarEndTerm(10, (0, 0)),
            PlanarEndTerm(10.1, (0.2, 0.3)),
        ),
    )
    updated = replace(record, ends=SectionRecessEnds(record.ends.low, SectionEnd("open", surface)))
    assert updated.ends.high.surface.height((-1, -1)) == pytest.approx(9.6)
    assert updated.ends.high.surface.height((1, 1)) == 10


def test_public_exports_and_json_terms_have_one_reconstruction():
    assert quiddity.PlanarEndTerm is PlanarEndTerm
    assert quiddity.PlanarEnvelopeEndSurface is PlanarEnvelopeEndSurface
    for reverse in (False, True):
        record = geometry(reverse=reverse)
        surface = (record.ends.low if reverse else record.ends.high).surface
        value = json.loads(json.dumps(surface.to_dict(), allow_nan=False))
        assert value["type"] == "plane_envelope"
        select = min if value["operator"] == "min" else max
        for u in (-1, -0.2, 0, 0.4, 1):
            for v in (-1, 0, 1):
                reconstructed = select(
                    term["height"] + term["gradient"][0] * u + term["gradient"][1] * v
                    for term in value["terms"]
                )
                assert reconstructed == surface.height((u, v))


def test_open_curved_and_concave_profiles_are_not_admitted():
    record = geometry()
    chain = record.profile.boundary
    profiles = (
        OpenSectionProfile("open", chain, (chain[-1].point, chain[0].point)),
        ClosedSectionProfile(
            "closed",
            (
                PassageSectionVertex((-1, 0), 1),
                PassageSectionVertex((1, 0), 1),
            ),
        ),
        ClosedSectionProfile(
            "closed",
            tuple(
                PassageSectionVertex(p, 0)
                for p in (
                    (-2, -1),
                    (-1, -1),
                    (-1, -2),
                    (1, -2),
                    (1, -1),
                    (2, -1),
                    (2, 1),
                    (1, 1),
                    (1, 2),
                    (-1, 2),
                    (-1, 1),
                    (-2, 1),
                )
            ),
        ),
    )
    for profile in profiles:
        with pytest.raises(ValueError, match="closed line-only|convex polygon"):
            replace(record, profile=profile)


def test_two_envelopes_and_mixed_cylinder_are_not_admitted():
    record = geometry()
    for opposite in (
        geometry(reverse=True).ends.low,
        SectionEnd("open", CylindricalEndSurface("cylinder", (0, 0, -10), (1, 0), 2, "positive")),
    ):
        with pytest.raises(ValueError, match="exactly one|opposite planar mouth"):
            replace(record, ends=SectionRecessEnds(opposite, record.ends.high))


@pytest.mark.parametrize(
    "changes",
    [
        {"type": "plane"},
        {"operator": "union"},
        {"terms": ()},
        {"terms": [PlanarEndTerm(10, (0, 0)), PlanarEndTerm(10, (1, 0))]},
        {"terms": (PlanarEndTerm(10, (0, 0)), PlanarEndTerm(10, (0, 0)))},
        {"terms": (PlanarEndTerm(10, (0, 0)), PlanarEndTerm(11, (0, 0)))},
        {"terms": (PlanarEndTerm(10, (1, 0)), PlanarEndTerm(10, (0, 0)))},
    ],
)
def test_invalid_envelope_terms(changes):
    with pytest.raises(ValueError):
        replace(geometry().ends.high.surface, **changes)


@pytest.mark.parametrize(
    "height,gradient",
    [
        (True, (0, 0)),
        (float("inf"), (0, 0)),
        (10.0000001, (0, 0)),
        (10, (float("nan"), 0)),
        (10, (0.0000001, 0)),
        (10, [0, 0]),
    ],
)
def test_invalid_plane_term(height, gradient):
    with pytest.raises(ValueError):
        PlanarEndTerm(height, gradient)


@pytest.mark.parametrize("height", [10.2, 11])
def test_boundary_only_or_inactive_term_refuses(height):
    record = geometry()
    surface = PlanarEnvelopeEndSurface(
        "plane_envelope",
        "min",
        (
            PlanarEndTerm(10, (0, 0)),
            PlanarEndTerm(height, (0.2, 0)),
        ),
    )
    with pytest.raises(ValueError, match="positive-area"):
        replace(record, ends=SectionRecessEnds(record.ends.low, SectionEnd("open", surface)))


def test_concave_valley_capped_mouth_sloped_mouth_and_centroid_mismatch_refuse():
    record = geometry()
    with pytest.raises(ValueError, match="convex roof"):
        replace(
            record,
            ends=SectionRecessEnds(
                record.ends.low,
                SectionEnd("open", replace(record.ends.high.surface, operator="max")),
            ),
        )
    for mouth in (SectionEnd("capped"), SectionEnd("open", PlanarEndSurface(gradient=(0.1, 0)))):
        with pytest.raises(ValueError, match="opposite planar mouth"):
            replace(record, ends=SectionRecessEnds(mouth, record.ends.high))
    with pytest.raises(ValueError, match="centroid"):
        replace(record, run_interval=(0, 9))
    with pytest.raises(ValueError, match="separated"):
        replace(record, run_interval=(9.9, 10))
