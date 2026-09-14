"""Shared semantic snapshot orchestration for old/new golden comparison."""

from __future__ import annotations


def recognition_snapshot(recognition, feature_census, part):
    """Run every public recogniser and required substrate with injected shared evidence."""

    if getattr(recognition, "__name__", None) == "quiddity":
        from tools._legacy_recognition import feature_census, namespace

        recognition = namespace()
    cylinders = recognition.analyse_cylinders(part)
    countersinks = recognition.recognise_countersinks(part)
    holes = recognition.recognise_holes(part, cyls=cylinders, csinks=countersinks)
    bosses = recognition.recognise_bosses(part, cyls=cylinders)
    slots = recognition.recognise_slots(part)
    pockets = recognition.recognise_pockets(part)

    individual = {
        "recognise_bosses": bosses,
        "recognise_chamfers": recognition.recognise_chamfers(part),
        "recognise_channels": recognition.recognise_channels(part),
        "recognise_countersinks": countersinks,
        "recognise_double_d_bores": recognition.recognise_double_d_bores(part),
        "recognise_face_levels": recognition.recognise_face_levels(part),
        "recognise_fillets": recognition.recognise_fillets(part),
        "recognise_flats": recognition.recognise_flats(part, cyls=cylinders),
        "recognise_grooves": recognition.recognise_grooves(part, cyls=cylinders),
        "recognise_hole_patterns": recognition.recognise_hole_patterns(holes),
        "recognise_holes": holes,
        "recognise_plates": recognition.recognise_plates(part),
        "recognise_pocket_patterns": recognition.recognise_pocket_patterns(pockets),
        "recognise_pockets": pockets,
        "recognise_polygonal_bosses": recognition.recognise_polygonal_bosses(part),
        "recognise_polygonal_stock": recognition.recognise_polygonal_stock(part),
        "recognise_rectangular_pads": recognition.recognise_rectangular_pads(part),
        "recognise_repeating_radial_profiles": (
            recognition.recognise_repeating_radial_profiles(part)
        ),
        "recognise_risers": recognition.recognise_risers(part),
        "recognise_slot_patterns": recognition.recognise_slot_patterns(slots),
        "recognise_slots": slots,
        "recognise_turned_steps": recognition.recognise_turned_steps(part, cyls=cylinders),
    }
    # Families originated in this package have no counterpart in the pinned Draftwright
    # baseline, so asking that module for them raises rather than returning nothing. Adding
    # them unconditionally broke `capture_draftwright_goldens` for the whole corpus, not only
    # for the new family — it snapshots `draftwright.recognition`, which has no such
    # attribute. Keying off the module being snapshotted keeps both callers working, and the
    # inventory check below still fails closed: a name present in `__all__` but skipped here
    # is a mismatch, so this cannot quietly drop a family from a package that does have it.
    for name in (
        "recognise_angled_steps",
        "recognise_blends",
        "recognise_gusset_ribs",
        "recognise_circular_blind_steps",
        "recognise_paired_ramp_steps",
        "recognise_passages",
        "recognise_prismatic_pockets",
        "recognise_rectangular_blind_slots",
        "recognise_round_bottom_blind_slots",
        "recognise_through_steps",
    ):
        recognise = getattr(recognition, name, None)
        if recognise is not None:
            individual[name] = recognise(part)

    recognise_gusset_patterns = getattr(recognition, "recognise_gusset_rib_patterns", None)
    if recognise_gusset_patterns is not None:
        individual["recognise_gusset_rib_patterns"] = recognise_gusset_patterns(
            individual["recognise_gusset_ribs"]
        )

    # Added in the 0.4 rich-schema transition and pinned by its own schema/oracle goldens.
    # This legacy snapshot deliberately stays byte-identical to the Draftwright-era surface.
    post_baseline = {
        "recognise_section_passages",
        "recognise_section_recesses",
        "recognise_edge_open_circular_pockets",
        "recognise_edge_open_prismatic_recesses",
        "recognise_oriented_slots",
        "recognise_oriented_slot_patterns",
    }
    public_recognisers = {
        name
        for name in recognition.__all__
        if name.startswith("recognise_") and name not in post_baseline
    }
    if set(individual) != public_recognisers:
        missing = sorted(public_recognisers - set(individual))
        extra = sorted(set(individual) - public_recognisers)
        raise RuntimeError(f"snapshot inventory mismatch: missing={missing}, extra={extra}")

    return {
        "individual": individual,
        "substrates": {
            "analyse_cylinders": cylinders,
            "feature_diameters": recognition.feature_diameters(
                part, cyls=cylinders, holes=holes, bosses=bosses
            ),
            "full_cylinders": [
                recognition.full_cylinders(cylinders[0]),
                recognition.full_cylinders(cylinders[1]),
            ],
            "step_level_records": recognition.step_level_records(part),
        },
        "aggregate": {
            "prismatic": recognition.build_recognition_result(
                part, cylinders=cylinders, rotational=False
            ),
            "rotational": recognition.build_recognition_result(
                part, cylinders=cylinders, rotational=True
            ),
        },
        "feature_census": feature_census(part),
    }
