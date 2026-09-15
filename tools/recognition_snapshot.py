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
    # Every other exported recogniser is found in `__all__` rather than a hand-kept list, so
    # adding a family cannot leave the snapshot stale. The registry says which are derived: a
    # physical recogniser takes the part; a derived one takes its source family's records. The
    # pinned Draftwright baseline has no such attributes, so a missing name is skipped there.
    # For the package, a recogniser that cannot run raises here rather than being omitted.
    from quiddity._registry import DERIVED_DEFINITIONS, PHYSICAL_DEFINITIONS

    entrypoint_of_family = {
        definition.family: definition.public_entrypoint for definition in PHYSICAL_DEFINITIONS
    }
    derived_sources = {
        definition.public_entrypoint: [
            entrypoint_of_family[source] for source in definition.sources
        ]
        for definition in DERIVED_DEFINITIONS
        if definition.public_entrypoint is not None
    }
    pending = [
        name
        for name in sorted(recognition.__all__)
        if name.startswith("recognise_") and name not in individual and name not in post_baseline
    ]
    for name in pending:
        if name in derived_sources:
            continue
        recognise = getattr(recognition, name, None)
        if recognise is not None:
            individual[name] = recognise(part)
    for name in pending:
        if name not in derived_sources:
            continue
        recognise = getattr(recognition, name, None)
        if recognise is None:
            continue
        missing_sources = [s for s in derived_sources[name] if s not in individual]
        if missing_sources:
            raise RuntimeError(
                f"{name}: sources {missing_sources} are not in the legacy snapshot; add it to "
                "post_baseline beside them or snapshot the source first"
            )
        individual[name] = recognise(*(individual[source] for source in derived_sources[name]))

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
