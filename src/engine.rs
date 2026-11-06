// Shared Rust engine source. Diagnostic examples compile this in their own crate so
// caller-space evaluation stays private without a public feature or a second algorithm.
// Caller-space algorithms are implementation details. Unit tests exercise them inside this crate.
macro_rules! internal_entry {
    ($(#[$meta:meta])* pub $($rest:tt)*) => {
        $(#[$meta])*
        #[allow(dead_code, unused_imports)]
        pub(crate) $($rest)*
    };
}

#[path = "body.rs"]
pub(crate) mod body;
#[cfg(feature = "cli")]
#[path = "cli.rs"]
pub mod cli;
#[path = "correspondence/mod.rs"]
pub mod correspondence;
#[path = "evidence_view.rs"]
pub mod evidence_view;
#[path = "features/mod.rs"]
pub mod features;
#[path = "file_api.rs"]
pub mod file_api;
#[path = "framed_records.rs"]
pub mod framed_records;
#[path = "frames.rs"]
pub mod frames;
#[path = "part_input.rs"]
pub mod part_input;
#[path = "pmi_json.rs"]
pub mod pmi_json;
#[path = "published_records.rs"]
pub mod published_records;
#[path = "recognition.rs"]
pub mod recognition;
#[path = "serve.rs"]
pub mod serve;
/// The geometry kernel, [`haecceity`], under the name the recognisers use.
pub use haecceity as kernel;

pub use correspondence::{Correspondence, Fingerprints, correspond};
pub use evidence_view::RecognitionEvidence;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use evidence_view::build_recognition_evidence;
pub use features::angled_steps::AngledStep;
pub use features::blends::{Blend, BlendPath, CircularBlendPath, StraightBlendPath};
pub use features::bosses::BossRecord;
pub use features::chamfers::{Chamfer, ChamferOptions};
pub use features::channels::Channel;
pub use features::circular_blind_steps::CircularBlindStep;
pub use features::circular_face_patterns::CircularFacePattern;
pub use features::countersinks::CounterSink;
pub use features::edge_open_circular::EdgeOpenCircularPocket;
pub use features::edge_open_prismatic::EdgeOpenPrismaticRecess;
pub use features::fillets::{Fillet, FilletAxis, FilletOptions};
pub use features::flats::Flat;
pub use features::freeform_surfaces::{
    BSplineSurfaceSupport, FreeformSurface, SurfaceContinuityLink,
};
pub use features::grooves::Groove;
pub use features::gussets::{GussetRib, GussetRibPattern};
pub use features::helical_springs::{HelicalSpring, SpringTolerances};
pub use features::hole_patterns::HolePattern;
pub use features::holes::{HoleOptions, HoleRecord};
pub use features::interior_voids::InteriorVoid;
pub use features::levels::{FaceLevel, FaceLevelOptions, RiserEvidence, RiserOptions};
pub use features::oblique_through_steps::ObliqueThroughStep;
pub use features::oriented_chamfers::{OrientedChamfer, OrientedChamferOptions};
pub use features::oriented_slots::{OrientedSlot, OrientedSlotPattern};
pub use features::outer_profile::{
    OuterProfileRefusalReason, PlanarOuterProfile, PlanarOuterProfileEvidence, ProfileArc,
    ProfileLine, ProfileSupport, RefusedPlanarOuterProfile,
};
pub use features::pads::{PadError, PadOptions, RaisedPad};
pub use features::paired_ramp_steps::PairedRampStep;
pub use features::passages::{Passage, PassageError, SectionPassage};
pub use features::plates::{Plate, PlateOptions};
pub use features::pockets::Pocket;
pub use features::polygonal_bosses::{
    PolygonalBoss, PolygonalOptions, PolygonalPrism, PolygonalStock,
};
pub use features::prismatic_pockets::PrismaticPocket;
pub use features::profiled_bores::{DoubleDBore, DoubleDBoreOptions};
pub use features::recess_patterns::{PocketPattern, SlotPattern};
pub use features::rectangular_blind_slots::RectangularBlindSlot;
pub use features::repeating_profiles::{RepeatingRadialProfile, RepeatingRadialProfileOptions};
pub use features::round_bottom_slots::RoundBottomBlindSlot;
pub use features::section_recess_family::SectionRecessFamilyError;
pub use features::sheet_metal::{SheetMetalBody, SheetMetalOptions};
pub use features::slots::Slot;
pub use features::thin_walls::ThinWallBody;
pub use features::through_steps::ThroughStep;
pub use features::turned::TurnedProfileKey;
pub use features::turned_bodies::TurnedBody;
pub use features::turned_steps::TurnedStep;
pub use kernel::brep::Part;
pub use kernel::step::{read_step, read_step_file};

pub use features::modelled_threads::{
    ModelledThread, ModelledThreadRefusal, ThreadAnalysis, ThreadAxis, ThreadTolerances,
};

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::angled_steps::recognise_angled_steps;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::blends::recognise_blends;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::bosses::recognise_bosses;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::chamfers::recognise_chamfers;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::channels::recognise_channels;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::circular_blind_steps::recognise_circular_blind_steps;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::circular_face_patterns::recognise_circular_face_patterns;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::countersinks::recognise_countersinks;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::edge_open_circular::recognise_edge_open_circular_pockets;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::edge_open_prismatic::recognise_edge_open_prismatic_recesses;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::fillets::recognise_fillets;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::flats::recognise_flats;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::freeform_surfaces::recognise_freeform_surfaces;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::grooves::recognise_grooves;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::gussets::{recognise_gusset_rib_patterns, recognise_gusset_ribs};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::hole_patterns::recognise_hole_patterns;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::holes::recognise_holes;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::interior_voids::recognise_interior_voids;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::levels::{recognise_face_levels, recognise_risers};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::modelled_threads::recognise_modelled_threads;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::oblique_through_steps::recognise_oblique_through_steps;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::oriented_chamfers::recognise_oriented_chamfers;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::oriented_slots::{
    recognise_oriented_slot_patterns, recognise_oriented_slots,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::pads::recognise_rectangular_pads;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::paired_ramp_steps::recognise_paired_ramp_steps;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::passages::{recognise_passages, recognise_section_passages};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::plates::recognise_plates;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::pockets::recognise_pockets;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::polygonal_bosses::{
    recognise_polygonal_bosses, recognise_polygonal_stock,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::prismatic_pockets::recognise_prismatic_pockets;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::profiled_bores::recognise_double_d_bores;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::recess_patterns::{recognise_pocket_patterns, recognise_slot_patterns};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::rectangular_blind_slots::recognise_rectangular_blind_slots;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::repeating_profiles::recognise_repeating_radial_profiles;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::round_bottom_slots::recognise_round_bottom_blind_slots;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::section_recess_family::recognise_section_recesses;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::sheet_metal::recognise_sheet_metal_bodies;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::slots::recognise_slots;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::thin_walls::recognise_thin_wall_bodies;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::through_steps::recognise_through_steps;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::turned_steps::recognise_turned_steps;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::outer_profile::planar_outer_profile;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::section_recess_family::build_section_recess_document;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use features::modelled_threads::analyse_modelled_threads;
