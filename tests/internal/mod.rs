//! Internal regression and independent reference tests; caller-space algorithms stay private.
#[path = "../common/mod.rs"]
mod common;
#[macro_use]
#[path = "../support/slices.rs"]
mod slices;
#[path = "captured.rs"]
mod captured;
#[path = "corpus.rs"]
mod corpus;
#[path = "correspondence.rs"]
mod correspondence;
#[path = "cylindrical_profiles.rs"]
mod cylindrical_profiles;
#[path = "effective_surfaces.rs"]
mod effective_surfaces;
#[path = "evidence.rs"]
mod evidence;
#[path = "fillet_convexity.rs"]
mod fillet_convexity;
#[path = "frames.rs"]
mod frames;
#[path = "inside_rounds.rs"]
mod inside_rounds;
#[path = "invariance.rs"]
mod invariance;
#[path = "inventory_floor.rs"]
mod inventory_floor;
#[path = "local_degradation.rs"]
mod local_degradation;
#[path = "mfinstseg.rs"]
mod mfinstseg;
#[path = "modelled_threads.rs"]
mod modelled_threads;
#[path = "outer_profiles.rs"]
mod outer_profiles;
#[path = "pads.rs"]
mod pads;
#[path = "partial_hole_exits.rs"]
mod partial_hole_exits;
#[path = "passages.rs"]
mod passages;
#[path = "prismatic_pockets.rs"]
mod prismatic_pockets;
#[path = "publication.rs"]
mod publication;
#[path = "recognition.rs"]
mod recognition;
#[path = "recognition_evidence.rs"]
mod recognition_evidence;
#[path = "reconcile.rs"]
mod reconcile;
#[path = "section_geometry.rs"]
mod section_geometry;
#[path = "section_recess.rs"]
mod section_recess;
#[path = "section_recess_geometry.rs"]
mod section_recess_geometry;
#[path = "section_recess_helpers.rs"]
mod section_recess_helpers;
#[path = "section_recess_publication.rs"]
mod section_recess_publication;
#[path = "section_recesses.rs"]
mod section_recesses;
#[path = "sections.rs"]
mod sections;
#[path = "step_levels.rs"]
mod step_levels;
#[path = "tie_breaks.rs"]
mod tie_breaks;
#[path = "trust_contract.rs"]
mod trust_contract;
#[path = "volume_probe.rs"]
mod volume_probe;
#[path = "wiring.rs"]
mod wiring;

#[path = "vector_publication.rs"]
mod vector_publication;

#[path = "face_grooves.rs"]
mod face_grooves;
#[path = "turned_bodies.rs"]
mod turned_bodies;

#[path = "edge_trimming.rs"]
mod edge_trimming;
