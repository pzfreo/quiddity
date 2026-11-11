//! Revision-stable fingerprints of a recognition result's features and of the part's faces
//! (`docs/correspondence.md`, "Fingerprints"). Everything in them is unchanged by a rigid motion
//! of the part except the placements (positions, axes, normals), which move with it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::features::Features;
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{self, Surface, V3};
use crate::kernel::py;

/// Fingerprint format version: correspondence refuses different formats.
/// Value changes within this format bump [`FINGERPRINT_REVISION`] and add an entry to
/// [`VALUE_CHANGES`]. Changes not expressible as excluded keys require fingerprint/4.
pub const FINGERPRINT_VERSION: &str = "quiddity-rust/fingerprint/3";

/// Vintage of the current values, independent of the crate's release version.
pub const FINGERPRINT_REVISION: u32 = 1;

/// A known value change within fingerprint/3. Keys use correspondence's `traits.` / `sizes.`
/// paths, or `position`, `position.axial` (the component along the feature's axis only),
/// `axis`, `neighbourhood`; a trailing `.*` covers indexed descendants.
/// Revision 0 means "before revision 1": the precise historical vintage was not recorded.
pub struct FingerprintValueChange {
    pub revision: u32,
    pub families: &'static [&'static str],
    pub keys: &'static [&'static str],
    pub evidence: &'static str,
}

/// Known key-level corrections, all predating recorded revisions. This is executable migration
/// data, not a list of recogniser additions/removals: exclusions cannot recreate missing records
/// or migrate face topology. Family renames additionally need `merge_legacy_families` below.
/// Only changes between released versions belong here: a revision-0 fingerprint cannot hold the
/// values of unreleased code. Prefer a narrow exclusion (one component, one key) that sometimes
/// reports a real old difference over a broad one that hides a design change.
pub const VALUE_CHANGES: &[FingerprintValueChange] = &[
    FingerprintValueChange {
        revision: 0,
        families: &["plates"],
        keys: &["sizes.thickness"],
        evidence: "CHANGELOG 0.1.1 #71: supporting face planes; 0.2.0-alpha.1 #84/#85: exact separation and whistle's envelope",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["slot_patterns"],
        keys: &["sizes.pitch", "sizes.row_pitch", "sizes.col_pitch", "axis"],
        evidence: "CHANGELOG 0.2.0-alpha.2 #92: raw member centres for pitches and directions",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["holes", "polygonal_bosses"],
        keys: &["position.axial"],
        evidence: "CHANGELOG 0.2.0 and 0.2.1-alpha.1 #100: drill-point cones and polygonal boss free-end cap join the defining faces; 0.2.0-alpha.1 #83 and 0.2.2 #118: hole mouths and outer steps. These faces are coaxial, so the centroid moves along the feature's axis only; the graph is kept comparable. hole_patterns are not listed: their axis is the row direction, not the members' drilling axis",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["slots", "slot_patterns"],
        keys: &["neighbourhood"],
        evidence: "CHANGELOG 0.2.0 #100: rounded rectangular slots claim both proved end walls; symmetric end walls preserve the centroid, while their graph changes; derived_faces propagates claims to patterns",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["pockets"],
        keys: &["traits.rectangular_boundary"],
        evidence: "CHANGELOG 0.2.0: proved rectangular pocket boundaries; FAMILIES pockets",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["holes"],
        keys: &[
            "traits.cbore",
            "traits.spotface",
            "sizes.cbore.diameter",
            "sizes.cbore.depth",
            "sizes.spotface.diameter",
            "sizes.spotface.depth",
            "sizes.outer_steps.*",
        ],
        evidence: "CHANGELOG 0.2.2 #118: counterbore/spotface threshold and outer_steps; derived_sizes holes",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["bosses"],
        keys: &["sizes.height", "position.axial", "neighbourhood"],
        evidence: "CHANGELOG 0.2.5 #134; fingerprint_with_axes: complete boss height and axial midpoint, contracted treatment graph",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["plates"],
        keys: &["position", "neighbourhood"],
        evidence: "CHANGELOG 0.2.5 #134; plate_position and supporting_neighbourhood: restored projected strips and contracted graph",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["fillets"],
        keys: &["traits.side"],
        evidence: "CHANGELOG 0.2.5 #134: concave turned fillets report their actual side",
    },
    FingerprintValueChange {
        revision: 0,
        families: &["interior_voids"],
        keys: &[
            "sizes.estimated_volume",
            "traits.volume_method",
            "position",
            "neighbourhood",
        ],
        evidence: "CHANGELOG 0.4.7 #189; interior_voids::analyse: closed_shell exact volume and complete shell defining faces; sample fields are ignored, openings remain zero",
    },
];

/// The fingerprints of one recognition result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fingerprints {
    pub fingerprint_version: String,
    /// 0 is an unrecorded fingerprint/3 vintage (through 0.4.x); never a crate version.
    #[serde(default)]
    pub fingerprint_revision: u32,
    /// The part's size, mm: the square root of its faces' total area (unchanged by motion,
    /// unlike a box). Tolerances and placement costs are relative to it.
    pub scale: f64,
    pub features: Vec<FeatureFingerprint>,
    pub faces: Vec<FaceFingerprint>,
}

/// A recognised feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeatureFingerprint {
    /// `<family>/<index>`: the record's place in the family's list, so stable within one result
    /// only.
    pub id: String,
    pub family: String,
    /// Semantics, the subtype: the record's categorical fields (a hole's `bottom`, a fillet's
    /// `side`, whether a counterbore is present, a pattern's shape), by name.
    pub traits: BTreeMap<String, String>,
    /// Semantics, the intrinsic sizes: the record's size fields by path (a counterbore's diameter
    /// is `cbore.diameter`), a pattern's member `count`, and sizes derived from fields that move
    /// with the part (a plate's `thickness`), as [`FAMILIES`] lists them.
    pub sizes: BTreeMap<String, f64>,
    /// Placement: the feature's axis, normal or run, as a unit vector (its sign is not
    /// significant).
    pub axis: Option<V3>,
    /// Placement: the area-weighted centroid of its faces; bosses use their axial midpoint,
    /// plates restore the projected strips removed by convex edge treatments (#134).
    pub position: Option<V3>,
    /// The part's faces that define it, ascending (a pattern's are its members').
    pub faces: Vec<usize>,
    /// Neighbourhood: for each of its faces and each face that one meets,
    /// `type>in|out:type:convex|concave|smooth|unknown` (`in` when the neighbour is its own),
    /// sorted: its subgraph of the attributed adjacency graph, in draftwright's bridge format (with
    /// the arc labels of `arc_label`, which differ from the bridge's on nearly tangent edges).
    pub neighbourhood: Vec<String>,
}

/// A face of the part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceFingerprint {
    pub index: usize,
    /// `plane`, `cylinder`, `cone`, `sphere`, `torus`, or the file's name for a freeform surface
    /// that is not exactly one of those.
    pub surface: String,
    /// Intrinsic surface parameters: `radius`, `semi_angle` (degrees), `major_radius`,
    /// `minor_radius`.
    pub parameters: BTreeMap<String, f64>,
    /// The surface's axis (cylinder, cone, torus) or a plane's outward normal, as a unit vector.
    pub axis: Option<V3>,
    /// A point the surface is placed by: a cylinder's or torus's axis point, a cone's apex, a
    /// sphere's centre, a plane's centroid.
    pub support: Option<V3>,
    /// None where the boundary quadrature cannot integrate the face.
    pub area: Option<f64>,
    /// The area-weighted centroid; where the area is unknown, the mean of its edge samples.
    pub centroid: V3,
    /// The mean outward normal (the unit area vector), where the face has one.
    pub normal: Option<V3>,
    /// Its neighbours across shared edges, with how the solid turns there.
    pub neighbours: Vec<(usize, String)>,
    /// Its adjacency signature: `type:arc` for each neighbour, sorted.
    pub adjacency: Vec<String>,
}

/// What a record field is to a feature's fingerprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// An intrinsic size, under its path (`cbore.diameter`). Several numbers (a list, or a field
    /// of each item of a list) are ordered by the frame or the record's canonical order, so they
    /// are kept smallest first as `<path>.<i>` (a ramp's two half-widths), `[]` left out.
    Size,
    /// The feature's axis, a unit vector or an axis letter: the first such field present.
    /// Placement.
    Axis,
    /// Categorical, the subtype: a string, boolean or null; a nested record's presence (`present`
    /// or `none`); a list's items, sorted and joined.
    Trait,
    /// A list of member records (a pattern's holes): its length is the size of this name.
    Count(&'static str),
    /// Moves with the part: positions, frame-aligned bounds, spans and points, directions, axis
    /// letters and signs. The feature's placement is its axis and its faces' centroid.
    Placement,
    /// Read through the family's derived sizes only (`derived_sizes`): canonically grouped
    /// size records (a hole's outer steps), bounds whose difference
    /// is a size (a plate's `lo` and `hi`), a section's points.
    Derived,
    /// Not a property of the design: face and body indices, sampling diagnostics, heuristic
    /// hints, a fit's residual.
    Ignored,
}

/// How one family's records are fingerprinted.
pub struct FamilyFingerprint {
    /// The family's field in [`Features`], its serde JSON key.
    pub family: &'static str,
    /// For a derived family (patterns of another family's records), which has no defining faces
    /// of its own: the record field holding its members, and the family they are records of,
    /// whose faces it takes.
    pub members: Option<(&'static str, &'static str)>,
    /// The records are untagged variants (a bolt circle, a grid): the record's field names are
    /// its `shape` trait.
    pub variants: bool,
    /// Every field path the records have and its role: `a.b` is a nested record's field, `a[]`
    /// each item of a list. A leaf of a record takes the role of the longest path that is it or
    /// leads to it (`tests/wiring.rs` checks that every leaf has one and every path is used).
    pub fields: &'static [(&'static str, Role)],
}

use Role::{Axis, Count, Derived, Ignored, Placement, Size, Trait};

/// The families, in [`Features`]' order.
pub const FAMILIES: &[FamilyFingerprint] = &[
    FamilyFingerprint {
        family: "fillets",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("radius", Size),
            ("side", Trait),
            ("turned", Trait),
            ("at", Placement),
        ],
    },
    FamilyFingerprint {
        family: "cylindrical_profiles",
        members: None,
        variants: false,
        fields: &[
            ("radius", Size),
            ("axis", Axis),
            ("axis_point", Placement),
            ("side", Trait),
            ("sweep", Size),
            ("faces", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "chamfers",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("leg1", Size),
            ("leg2", Size),
            ("angle", Size),
            ("turned", Trait),
            ("at", Placement),
            // The virtual sharp corner; absent exactly when `turned`.
            ("corner", Placement),
        ],
    },
    FamilyFingerprint {
        family: "bosses",
        members: None,
        variants: false,
        fields: &[
            ("superseded_by", Ignored),
            ("axis", Axis),
            ("diameter", Size),
            ("height", Size),
            ("location", Placement),
        ],
    },
    FamilyFingerprint {
        family: "angled_steps",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("leg1", Size),
            ("leg2", Size),
            ("angle", Size),
            ("length", Size),
            ("at", Placement),
            ("corner", Placement),
        ],
    },
    FamilyFingerprint {
        family: "flats",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("across", Size),
            ("at", Placement),
            ("axis_direction", Placement),
            ("axis_line", Placement),
            // The stock cylinder's extent along its axis, not the flat's.
            ("stock_span", Placement),
        ],
    },
    FamilyFingerprint {
        family: "paired_ramp_steps",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("angle", Size),
            ("length", Size),
            ("half_width", Size),
            // Low side first: ordered by the frame, so sorted.
            ("half_widths", Size),
            ("at", Placement),
            ("opening_direction", Placement),
        ],
    },
    FamilyFingerprint {
        family: "oriented_chamfers",
        members: None,
        variants: false,
        fields: &[
            ("run", Axis),
            ("length", Size),
            ("leg1", Size),
            ("leg2", Size),
            ("angle", Size),
            ("at", Placement),
            ("corner", Placement),
            ("leg1_direction", Placement),
            ("leg2_direction", Placement),
            ("support_spans", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "circular_face_patterns",
        members: None,
        variants: false,
        fields: &[
            ("axis_direction", Axis),
            ("count", Size),
            ("pitch_degrees", Size),
            ("axis_origin", Placement),
            ("seed_index", Ignored),
            ("fit_error", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "oblique_through_steps",
        members: None,
        variants: false,
        fields: &[
            ("run", Axis),
            ("length", Size),
            ("depth", Size),
            ("at", Placement),
            ("depth_direction", Placement),
            ("across_direction", Placement),
            ("wall_outline", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "polygonal_oblique_through_steps",
        members: None,
        variants: false,
        fields: &[
            ("run", Axis),
            ("length", Size),
            ("slant_degrees", Size),
            ("through_axis", Placement),
            ("depth", Size),
            ("at", Placement),
            ("depth_direction", Placement),
            ("across_direction", Placement),
            ("floor_outline", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "circular_blind_steps",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("radius", Size),
            ("length", Size),
            ("centreline", Placement),
            // Arc endpoint, centre, arc endpoint: the radius is its size.
            ("section", Placement),
        ],
    },
    FamilyFingerprint {
        family: "holes",
        members: None,
        variants: false,
        fields: &[
            ("superseded_by", Ignored),
            ("axis", Axis),
            ("diameter", Size),
            ("depth", Size),
            ("bottom", Trait),
            ("cbore", Trait),
            ("cbore.diameter", Size),
            ("cbore.depth", Size),
            ("spotface", Trait),
            ("spotface.diameter", Size),
            ("spotface.depth", Size),
            // Keep each diameter paired with its depth in canonical mouth-to-bore order.
            ("outer_steps", Derived),
            // Edge-break annotations do not redefine the cylindrical hole's identity.
            ("mouth_chamfers", Ignored),
            ("exit_obstructions", Derived),
            ("csink", Trait),
            ("csink.major_diameter", Size),
            ("csink.drill_diameter", Size),
            ("csink.included_angle", Size),
            ("csink.depth", Size),
            ("csink.axis", Placement),
            ("csink.location", Placement),
            ("location", Placement),
        ],
    },
    FamilyFingerprint {
        family: "countersinks",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("major_diameter", Size),
            ("drill_diameter", Size),
            ("included_angle", Size),
            ("depth", Size),
            ("location", Placement),
        ],
    },
    FamilyFingerprint {
        family: "hole_patterns",
        members: Some(("holes", "holes")),
        variants: true,
        fields: &[
            ("direction", Axis),
            ("holes", Count("count")),
            ("rows", Size),
            ("cols", Size),
            ("row_pitch", Size),
            ("col_pitch", Size),
            ("width", Size),
            ("height", Size),
            ("diameter", Size),
            ("pitch", Size),
            // The pattern's orientation in the part's frame.
            ("angle", Placement),
            ("center", Placement),
        ],
    },
    FamilyFingerprint {
        family: "gusset_ribs",
        members: None,
        variants: false,
        fields: &[
            ("thickness_axis", Axis),
            ("legs", Size),
            ("thickness_bounds", Derived),
            ("supports", Placement),
            ("directions", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "gusset_rib_patterns",
        members: Some(("ribs", "gusset_ribs")),
        variants: true,
        fields: &[
            ("axis", Axis),
            ("ribs", Count("count")),
            ("pitch", Size),
            ("mirror_plane", Placement),
        ],
    },
    FamilyFingerprint {
        family: "thin_wall_bodies",
        members: None,
        variants: false,
        fields: &[
            ("thickness", Size),
            ("face_pairs", Count("pairs")),
            ("body_key", Placement),
            // Face and body indices, which a revision renumbers.
            ("body_index", Ignored),
            ("unpaired_faces", Ignored),
            ("unpaired_face_classes", Ignored),
            ("rim_regions", Ignored),
            // How well the faces pair, and a heuristic guess at the modelling history.
            ("paired_area_fraction", Ignored),
            ("history_hint", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "interior_voids",
        members: None,
        variants: false,
        fields: &[
            ("estimated_volume", Size),
            ("volume_method", Trait),
            ("openings", Count("openings")),
            ("body_key", Placement),
            ("body_index", Ignored),
            ("void_faces", Ignored),
            // The sampling the volume was estimated with.
            ("grid_pitch", Ignored),
            ("enclosed_samples", Ignored),
            ("air_samples", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "internal_bevels",
        members: None,
        variants: false,
        fields: &[
            ("run", Axis),
            ("length", Size),
            ("at", Placement),
            ("corner", Placement),
            ("leg1", Size),
            ("leg2", Size),
            // Placements contribute through the run and defining-face centroid/graph, not
            // named direction slots. Equal legs are therefore an unordered pair, including
            // geometrically symmetric supports for which no intrinsic ordering is proved.
            ("leg1_direction", Placement),
            ("leg2_direction", Placement),
            ("angle", Size),
        ],
    },
    FamilyFingerprint {
        family: "local_steps",
        members: None,
        variants: false,
        fields: &[
            ("run_direction", Axis),
            ("run_start", Placement),
            ("run_end", Placement),
            ("length", Size),
            ("height_direction", Axis),
            ("height", Derived),
            ("depth_direction", Axis),
            ("depth", Derived),
            ("ends[].scope", Trait),
            ("ends[].faces", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "through_steps",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("length", Size),
            ("endpoint_scopes", Trait),
            // Boundary endpoint, concave corner, boundary endpoint: its two legs.
            ("section", Derived),
            ("at", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "turned_steps",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("diameter", Size),
            ("lo", Derived),
            ("hi", Derived),
            ("profile", Placement),
        ],
    },
    FamilyFingerprint {
        family: "face_grooves",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("axis_point", Placement),
            ("outer_diameter", Size),
            ("inner_diameter", Size),
            ("depth", Size),
            ("outer_wall_angle", Size),
            ("inner_wall_angle", Size),
            ("floor", Ignored),
            ("faces", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "annular_grooves",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("floor_center", Placement),
            ("inner_radius", Size),
            ("outer_radius", Size),
            ("depth", Size),
        ],
    },
    FamilyFingerprint {
        family: "grooves",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("width", Size),
            ("diameter", Size),
            ("at", Placement),
            ("profile", Placement),
        ],
    },
    FamilyFingerprint {
        family: "turned_bodies",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("axis_point", Placement),
            ("od", Size),
            ("length", Size),
            ("bores[].diameter", Size),
            ("bores[].span", Derived),
            ("profile[].kind", Trait),
            ("profile[].side", Trait),
            ("profile[].start", Derived),
            ("profile[].end", Derived),
            ("profile[].arc_center", Derived),
            ("profile[].arc_radius", Size),
            ("profile[].arc_sweep", Derived),
            ("profile[].face", Ignored),
            ("body_key", Placement),
            ("profile_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "plates",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("lo", Derived),
            ("hi", Derived),
            ("u", Placement),
            ("v", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "semicircular_blind_slots",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("opening", Placement),
            ("at", Placement),
            ("length", Size),
            ("radius", Size),
            ("straight_height", Size),
        ],
    },
    FamilyFingerprint {
        family: "round_bottom_blind_slots",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("length", Size),
            ("flat_width", Size),
            ("radius", Size),
            ("at", Placement),
            ("width_axis", Placement),
            ("depth_axis", Placement),
            ("depth_sign", Placement),
            ("open_sign", Placement),
        ],
    },
    FamilyFingerprint {
        family: "rectangular_blind_slots",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("length", Size),
            ("width", Size),
            ("depth", Size),
            ("at", Placement),
            ("width_axis", Placement),
            ("depth_axis", Placement),
            ("depth_sign", Placement),
            ("open_sign", Placement),
        ],
    },
    FamilyFingerprint {
        family: "double_d_bores",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("major_diameter", Size),
            ("across_flats", Size),
            ("depth", Size),
            ("through", Trait),
            ("location", Placement),
            ("flat_direction", Placement),
        ],
    },
    FamilyFingerprint {
        family: "edge_open_circular_pockets",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("run_interval", Derived),
            ("open_sign", Placement),
            ("section.segments[].kind", Trait),
            ("section.segments[].radius", Size),
            ("section.segments[].sweep", Derived),
            ("section.segments[].start", Derived),
            ("section.segments[].end", Derived),
            ("section.segments[].center", Placement),
            ("section.opening", Derived),
        ],
    },
    FamilyFingerprint {
        family: "edge_open_prismatic_recesses",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("run_interval", Derived),
            ("open_sign", Placement),
            ("section.wall_chain", Derived),
            ("section.opening.start", Derived),
            ("section.opening.end", Derived),
        ],
    },
    FamilyFingerprint {
        family: "blends",
        members: None,
        variants: false,
        fields: &[
            ("radius", Size),
            ("side", Trait),
            // A straight path's cylinder axis, or a circular path's torus axis: the one present.
            ("path.direction", Axis),
            ("path.normal", Axis),
            // A circular path's centre-line radius; a straight path has none.
            ("path.radius", Size),
            ("path.at", Placement),
            ("path.center", Placement),
        ],
    },
    FamilyFingerprint {
        family: "sheet_metal_bodies",
        members: None,
        variants: false,
        fields: &[
            ("thickness", Size),
            ("flanges", Count("flanges")),
            ("bends", Count("bends")),
            ("formed_features", Count("formed_features")),
            ("flat_pattern_status", Trait),
            ("body_key", Placement),
            // Face and body indices, which a revision renumbers.
            ("body_index", Ignored),
            ("first_side_faces", Ignored),
            ("second_side_faces", Ignored),
            ("cut_edge_faces", Ignored),
            ("edge_treatments", Ignored),
            // How well the faces pair, and the development laid out in the base flange's frame
            // (its triangulation is the kernel's).
            ("paired_area_fraction", Ignored),
            ("flat_pattern", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "slots",
        members: None,
        variants: false,
        fields: &[
            ("long_axis", Axis),
            ("width", Size),
            ("length", Size),
            // Proved radii only; an unproved one is null and adds nothing.
            ("end_radius", Size),
            ("corner_radius", Size),
            ("width_axis", Placement),
            ("w_center", Placement),
            ("lo", Placement),
            ("hi", Placement),
            ("d_lo", Placement),
            ("d_hi", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "pockets",
        members: None,
        variants: false,
        fields: &[
            ("long_axis", Axis),
            ("width", Size),
            ("length", Size),
            ("depth", Size),
            ("end_radius", Size),
            ("corner_radius", Size),
            ("edge_anchored", Trait),
            ("rectangular_boundary", Trait),
            ("floor_radius", Size),
            ("floor_fillets", Ignored),
            ("width_axis", Placement),
            ("w_center", Placement),
            ("lo", Placement),
            ("hi", Placement),
            ("d_lo", Placement),
            ("d_hi", Placement),
            ("open_sign", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "v_channels",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("opening", Placement),
            ("at", Placement),
            ("length", Size),
            ("width", Size),
            ("depth", Size),
            ("included_angle_degrees", Size),
            // Absent for a sharp trough, which adds nothing.
            ("trough_radius", Size),
        ],
    },
    FamilyFingerprint {
        family: "rectangular_through_notches",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("opening", Placement),
            ("at", Placement),
            ("width", Size),
            ("depth", Size),
            ("thickness", Size),
            // Null for sharp corners, which adds nothing.
            ("corner_radius", Size),
        ],
    },
    FamilyFingerprint {
        family: "channels",
        members: None,
        variants: false,
        fields: &[
            ("long_axis", Axis),
            ("width", Size),
            ("width_axis", Placement),
            ("w_center", Placement),
            // Python's `length` and `depth` are differences of these published (two-decimal)
            // coordinates, which a translation moves by a grid step: placements, not sizes.
            ("lo", Placement),
            ("hi", Placement),
            ("d_lo", Placement),
            ("d_hi", Placement),
            ("open_sign", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "slot_patterns",
        members: Some(("slots", "slots")),
        variants: true,
        fields: &[
            ("direction", Axis),
            ("slots", Count("count")),
            ("rows", Size),
            ("cols", Size),
            ("row_pitch", Size),
            ("col_pitch", Size),
            ("pitch", Size),
            // The pattern's orientation in the part's frame.
            ("angle", Placement),
            ("center", Placement),
        ],
    },
    FamilyFingerprint {
        family: "pocket_patterns",
        members: Some(("pockets", "pockets")),
        variants: true,
        fields: &[
            ("direction", Axis),
            ("pockets", Count("count")),
            ("rows", Size),
            ("cols", Size),
            ("row_pitch", Size),
            ("col_pitch", Size),
            ("pitch", Size),
            ("angle", Placement),
            ("center", Placement),
        ],
    },
    FamilyFingerprint {
        family: "repeating_radial_profiles",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("repeat_count", Size),
            ("edge_count", Size),
            ("centre", Placement),
            ("span", Placement),
            // Evidence that the count came from the complete wire; Python's consumers
            // correspond the occurrence by axis, centre and span, not by these samples.
            ("sector_signature", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "freeform_surfaces",
        members: None,
        variants: false,
        fields: &[
            ("support_kind", Trait),
            ("support.u_degree", Trait),
            ("support.v_degree", Trait),
            ("support.u_periodic", Trait),
            ("support.v_periodic", Trait),
            // Read for the distances between the net's corners (`derived_sizes`).
            ("support.poles", Derived),
            // The parameterisation: one surface has many.
            ("support.weights", Ignored),
            ("support.u_knots", Ignored),
            ("support.v_knots", Ignored),
            ("support.u_multiplicities", Ignored),
            ("support.v_multiplicities", Ignored),
            ("construction_kind", Trait),
            // A parameter direction, not a direction in space.
            ("construction_axis", Trait),
            ("construction_vector", Placement),
            ("continuity_links[].kind", Trait),
            ("offset_distance", Size),
            ("offset_basis", Trait),
            // Face indices, which a revision renumbers.
            ("face", Ignored),
            ("continuity_group", Ignored),
            ("continuity_links[].other_face", Ignored),
            ("offset_partner", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "helical_springs",
        members: None,
        variants: false,
        fields: &[
            ("axis.direction", Axis),
            ("axis.point", Placement),
            ("wire_diameter", Size),
            ("mean_diameter", Size),
            ("outer_diameter", Size),
            ("inner_diameter", Size),
            ("free_length", Size),
            ("total_coils", Size),
            ("total_coils_basis", Trait),
            ("active_coils", Size),
            ("pitch", Size),
            ("hand", Trait),
            ("end_type", Trait),
            ("active_coils_basis", Trait),
            ("start", Placement),
            ("end", Placement),
            ("tolerances", Ignored),
            ("faces", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "modelled_threads",
        members: None,
        variants: false,
        fields: &[
            ("axis.direction", Axis),
            ("axis.point", Placement),
            ("side", Trait),
            ("major_diameter", Size),
            ("minor_diameter", Size),
            ("pitch", Size),
            ("hand", Trait),
            ("starts", Size),
            ("start", Placement),
            ("end", Placement),
            ("length", Size),
            ("flank_faces", Ignored),
            ("crest_faces", Ignored),
            ("root_faces", Ignored),
            ("runout_faces", Ignored),
            ("tolerances", Ignored),
        ],
    },
    FamilyFingerprint {
        family: "polygonal_bosses",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("side_count", Trait),
            ("across_flats", Size),
            // Their difference is the height.
            ("base", Derived),
            ("top", Derived),
            ("center", Placement),
            ("flat_directions", Placement),
            ("flat_centres", Placement),
        ],
    },
    FamilyFingerprint {
        family: "polygonal_stock",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("side_count", Trait),
            ("across_flats", Size),
            // Their difference is the length.
            ("base", Derived),
            ("top", Derived),
            ("center", Placement),
            ("flat_directions", Placement),
            ("flat_centres", Placement),
        ],
    },
    FamilyFingerprint {
        family: "section_passages",
        members: None,
        variants: false,
        fields: &[
            ("frame.run", Axis),
            ("frame.origin", Placement),
            ("frame.u", Placement),
            ("frame.v", Placement),
            ("run_interval", Derived),
            ("section.boundary[].point", Derived),
            ("section.boundary[].bulge", Size),
            ("ends.low_capped", Trait),
            ("ends.high_capped", Trait),
            // Across the section in the frame's u and v, which turn with the part.
            ("ends.low_gradient", Placement),
            ("ends.high_gradient", Placement),
        ],
    },
    FamilyFingerprint {
        family: "prismatic_through_cuts",
        members: None,
        variants: false,
        fields: &[
            ("cut.frame.run", Axis),
            ("cut.frame.origin", Placement),
            ("cut.frame.u", Placement),
            ("cut.frame.v", Placement),
            ("cut.run_interval", Derived),
            ("cut.section.boundary[].point", Derived),
            ("cut.section.boundary[].bulge", Size),
            ("cut.ends.low_capped", Trait),
            ("cut.ends.high_capped", Trait),
            ("cut.ends.low_gradient", Placement),
            ("cut.ends.high_gradient", Placement),
            ("core_interval", Placement),
            ("extent", Trait),
        ],
    },
    FamilyFingerprint {
        family: "prismatic_pockets",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("sides", Trait),
            ("depth", Size),
            ("open_sign", Placement),
            ("at", Placement),
            // Corners in part coordinates across the axis: their chords are the sizes.
            ("section", Derived),
        ],
    },
    FamilyFingerprint {
        family: "oriented_slots",
        members: None,
        variants: false,
        fields: &[
            ("long_direction", Axis),
            ("width", Size),
            ("length", Size),
            ("width_direction", Placement),
            ("center", Placement),
            ("body_key", Placement),
            // The source passage: its run and frame move with the part, its run interval gives
            // the depth, and its section is the rectangle measured by width and length.
            ("source.frame", Placement),
            ("source.run_interval", Derived),
            ("source.section", Ignored),
            ("source.ends.low_capped", Trait),
            ("source.ends.high_capped", Trait),
            ("source.ends.low_gradient", Placement),
            ("source.ends.high_gradient", Placement),
        ],
    },
    FamilyFingerprint {
        family: "oriented_slot_patterns",
        members: Some(("slots", "oriented_slots")),
        variants: true,
        fields: &[
            ("direction", Axis),
            ("slots", Count("count")),
            ("rows", Size),
            ("cols", Size),
            ("row_pitch", Size),
            ("col_pitch", Size),
            ("pitch", Size),
            ("angle", Placement),
            ("center", Placement),
        ],
    },
    FamilyFingerprint {
        family: "pads",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("direction", Placement),
            // The box: its extent along the axis is the height, across it the footprint.
            ("x0", Derived),
            ("x1", Derived),
            ("y0", Derived),
            ("y1", Derived),
            ("z0", Derived),
            ("z1", Derived),
        ],
    },
    FamilyFingerprint {
        family: "step_levels",
        members: None,
        variants: false,
        fields: &[
            // A horizontal plane's height, and its faces' extent, whose widths are its sizes.
            ("z", Placement),
            ("x_span", Derived),
            ("y_span", Derived),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "risers",
        members: None,
        variants: false,
        fields: &[
            ("axis", Axis),
            ("vertical", Trait),
            // Which end is low follows the part's sense of z.
            ("lo_at_envelope", Placement),
            ("hi_at_envelope", Placement),
            ("positions", Placement),
            ("other_axis", Placement),
            ("other_positions", Placement),
            // Its height is their difference.
            ("z_lo", Derived),
            ("z_hi", Derived),
            // The scan's tolerance, the same for every riser.
            ("tol", Ignored),
            ("body_levels", Placement),
            ("body_key", Placement),
        ],
    },
    FamilyFingerprint {
        family: "section_recesses",
        members: None,
        variants: false,
        fields: &[
            ("source", Ignored),
            // The record's place in the family, its body's and the faces' indices, its tag.
            ("index", Ignored),
            ("body", Ignored),
            ("geometry.type", Ignored),
            ("evidence", Ignored),
            ("geometry.frame.run", Axis),
            ("geometry.frame.origin", Placement),
            ("geometry.frame.u", Placement),
            ("geometry.frame.v", Placement),
            ("geometry.run_interval", Derived),
            ("geometry.profile.closure", Trait),
            ("geometry.profile.boundary[].point", Derived),
            // A bulge's sign follows the boundary's direction, which the frame chooses.
            ("geometry.profile.boundary[].bulge", Derived),
            ("geometry.profile.opening", Derived),
            // The chain's canonical direction is chosen in the frame's u and v.
            ("geometry.profile.material_side", Placement),
            // Which end is low (so which is capped, curved or an envelope) follows the run's
            // canonical sense; heights are along it, gradients and the cylinder's axis across the
            // section in u and v. How many ends are capped is the feature kind.
            ("geometry.ends.low", Placement),
            ("geometry.ends.high", Placement),
            ("geometry.ends.low.surface.radius", Derived),
            ("geometry.ends.high.surface.radius", Derived),
            ("classification.feature_kind", Trait),
            ("classification.section_shape", Trait),
        ],
    },
];

/// *features*' published records as JSON by family, as [`fingerprint`] reads them.
pub fn records_json(features: &Features) -> Value {
    serde_json::to_value(features).expect("records serialise")
}

/// The fingerprint table of the family with this serde key.
pub fn family(key: &str) -> Option<&'static FamilyFingerprint> {
    FAMILIES.iter().find(|f| f.family == key)
}

/// The field path, and its role, that a record leaf at *path* (`a.b`, `a[]`) of family *key*
/// takes: the longest listed path that is it or leads to it.
pub fn classify(key: &str, path: &str) -> Option<(&'static str, Role)> {
    let leads = |p: &str| {
        path.strip_prefix(p)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
    };
    family(key)?
        .fields
        .iter()
        .filter(|(p, _)| leads(p))
        .max_by_key(|(p, _)| p.len())
        .copied()
}

/// The fingerprints of *features*, recognised on *part*.
///
/// Panics on a family missing from [`FAMILIES`], or one with neither defining faces in
/// `features.defining` nor members: a wiring defect, not an input.
pub fn fingerprint(part: &Part, features: &Features) -> Fingerprints {
    fingerprint_with_axes(
        &crate::features::Context::new(part),
        part,
        features,
        features,
        None,
        |_, _, _| None,
    )
}

#[cfg(test)]
pub(crate) fn published_fingerprint(
    part: &Part,
    features: &crate::published_records::PublishedFeatures,
) -> Fingerprints {
    published_fingerprint_impl(
        &crate::features::Context::new(part),
        part,
        features,
        features.working(),
        None,
    )
}

pub(crate) fn published_fingerprint_in(
    part: &Part,
    features: &crate::published_records::PublishedFeatures,
    ctx: &crate::features::Context<'_>,
) -> Fingerprints {
    let (source, motion) = features.measurement_source();
    published_fingerprint_impl(ctx, part, features, source, Some(motion))
}

fn published_fingerprint_impl(
    ctx: &crate::features::Context<'_>,
    part: &Part,
    features: &crate::published_records::PublishedFeatures,
    source: &Features,
    motion: Option<&crate::framed_records::Rigid>,
) -> Fingerprints {
    let mut fingerprints = fingerprint_with_axes(
        ctx,
        part,
        features.working(),
        source,
        motion,
        |id, path, value| features.fingerprint_axis(id, path, value),
    );
    for f in &mut fingerprints.features {
        let n =
            f.id.rsplit_once('/')
                .expect("record ID")
                .1
                .parse()
                .expect("index");
        f.id = features.published_id(&f.family, n);
        merge_fingerprint(f);
    }
    fingerprints
}

/// Adapt the three former namespaces without changing intrinsic measurements. Optional
/// source detail is not a semantic distinction between two instances of the same kind.
pub(crate) fn merge_fingerprint(f: &mut FeatureFingerprint) {
    match f.family.as_str() {
        "oriented_chamfers" => {
            f.traits.insert("turned".into(), "false".into());
        }
        "oriented_slots" => {
            f.traits.remove("source.ends.low_capped");
            f.traits.remove("source.ends.high_capped");
        }
        _ => {}
    }
    f.family = crate::published_records::publication_family(&f.family).into();
}

/// Match 0.1.1's oriented namespaces against the merged families within v3.
/// Keep old IDs so saved references can follow the resulting correspondence pairs.
pub(crate) fn merge_legacy_families(fingerprints: &mut Fingerprints) {
    if fingerprints.fingerprint_version == FINGERPRINT_VERSION {
        for f in &mut fingerprints.features {
            merge_fingerprint(f);
        }
    }
}

fn fingerprint_with_axes(
    ctx: &crate::features::Context<'_>,
    part: &Part,
    features: &Features,
    source: &Features,
    motion: Option<&crate::framed_records::Rigid>,
    axis: impl Fn(&str, &str, &Value) -> Option<V3>,
) -> Fingerprints {
    let faces: Vec<FaceFingerprint> = (0..part.faces.len())
        .map(|i| {
            // Axial cone-strip projection already integrated these same surface moments.
            // Carry that measurement into file coordinates instead of integrating it again.
            let measured = matches!(ctx.part.faces[i].surface, Surface::Cone { .. })
                .then(|| ctx.measured_surface_moments(i))
                .flatten()
                .map(|m| FaceMeasure {
                    area: m.area,
                    centroid: motion.map_or(m.centroid, |r| r.point(m.centroid)),
                    area_vector: motion.map_or(m.area_vector, |r| r.direction(m.area_vector)),
                });
            face(part, i, measured)
        })
        .collect();
    let scale = faces
        .iter()
        .filter_map(|f| f.area)
        .sum::<f64>()
        .sqrt()
        .max(1e-3);
    let json = records_json(features);
    let mut out = Vec::new();
    for (key, records) in json.as_object().expect("an object") {
        let Some(records) = records.as_array() else {
            continue;
        };
        let table = family(key)
            .unwrap_or_else(|| panic!("family {key} has no fingerprint table (FAMILIES)"));
        for (n, record) in records.iter().enumerate() {
            let faces_of = match (features.defining.get(key.as_str()), table.members) {
                (Some(defining), _) => defining.get(n).cloned().unwrap_or_else(|| {
                    panic!(
                        "family {key} has {} records but defining faces for {}",
                        records.len(),
                        defining.len()
                    )
                }),
                (None, Some(members)) => derived_faces(&json, features, key, members, record),
                (None, None) => panic!(
                    "family {key} has no defining faces in Features::defining and is not derived"
                ),
            };
            let mut fp = feature(&faces, format!("{key}/{n}"), table, record, faces_of, &axis);
            if !fp.faces.is_empty() && (key == "bosses" || key == "plates") {
                fp.neighbourhood = supporting_neighbourhood(&faces, &fp.faces, ctx);
                let position = if key == "bosses" {
                    Some(crate::features::bosses::midpoint(
                        ctx,
                        &source.bosses[n],
                        &fp.faces,
                    ))
                } else {
                    plate_position(ctx, &fp.faces)
                };
                fp.position = position
                    .map(|p| motion.map_or(p, |m| m.point(p)))
                    .or(fp.position);
            }
            out.push(fp);
        }
    }
    Fingerprints {
        fingerprint_version: FINGERPRINT_VERSION.into(),
        fingerprint_revision: FINGERPRINT_REVISION,
        scale,
        features: out,
        faces,
    }
}

/// Contract proved convex treatment faces when reading the supporting feature's graph.
/// Each real neighbouring support is counted once, even when reached directly and through
/// a bevel. Face fingerprints retain the complete physical graph and trimmed measurements.
fn supporting_neighbourhood(
    faces: &[FaceFingerprint],
    own: &[usize],
    ctx: &crate::features::Context<'_>,
) -> Vec<String> {
    let treatments: BTreeSet<_> = ctx
        .edge_treatments()
        .iter()
        .flat_map(|o| o.defining.iter().copied())
        .filter(|f| !own.contains(f))
        .collect();
    let mut out = Vec::new();
    for &f in own {
        let mut neighbours: BTreeMap<usize, &str> = faces[f]
            .neighbours
            .iter()
            .map(|(g, arc)| (*g, arc.as_str()))
            .collect();
        // Contract one treatment, without connecting separate patches of this feature
        // or unrelated bores along a large conical band. A boss's virtual end support
        // is transverse and planar; its existing longitudinal neighbours stay direct.
        for (g, _) in &faces[f].neighbours {
            if !treatments.contains(g) {
                continue;
            }
            let mut supports = BTreeSet::new();
            for treatment in ctx
                .edge_treatments()
                .iter()
                .filter(|o| o.defining.contains(g))
            {
                for &patch in &treatment.defining {
                    for (h, _) in &faces[patch].neighbours {
                        if own.contains(h) || treatments.contains(h) {
                            continue;
                        }
                        if faces[f].surface == "cylinder"
                            && !(faces[*h].surface == "plane"
                                && matches!((faces[f].axis, faces[*h].axis),
                                (Some(a), Some(n)) if geom::dot(a, n).abs() >= crate::features::policy::AXIS_ALIGNED_COS))
                        {
                            continue;
                        }
                        supports.insert(*h);
                    }
                }
            }
            if !supports.is_empty() {
                neighbours.remove(g);
                for h in supports {
                    neighbours.entry(h).or_insert("convex");
                }
            }
        }
        for (g, arc) in neighbours {
            let side = if own.contains(&g) { "in" } else { "out" };
            out.push(format!(
                "{}>{side}:{}:{arc}",
                faces[f].surface, faces[g].surface
            ));
        }
    }
    out.sort();
    out
}

/// The area centre of the two supporting skins, restoring projected treatment strips on
/// each real skin plane. This is intrinsic under rotation and does not move when an edge
/// break removes area from one skin. A moved plane or a changed footprint still moves it.
fn plate_position(ctx: &crate::features::Context<'_>, own: &[usize]) -> Option<V3> {
    let m = crate::features::plates::supporting_moments(ctx, own)?;
    (m.area > 0.).then(|| geom::scale(m.sum, 1. / m.area))
}

/// Additive duplicate references do not change an embedded pattern member's identity.
/// Patterns keep the original records captured before the final annotation pass.
pub(crate) fn same_member(family: &str, a: &Value, b: &Value) -> bool {
    let marker = match family {
        "holes" | "bosses" => "superseded_by",
        "section_recesses" => "source",
        _ => return a == b,
    };
    match (a.as_object(), b.as_object()) {
        (Some(a), Some(b)) => a
            .iter()
            .filter(|(k, _)| k.as_str() != marker)
            .eq(b.iter().filter(|(k, _)| k.as_str() != marker)),
        _ => a == b,
    }
}

/// A derived record's faces: its members' defining faces, each member found among the base
/// family's records (the first not yet taken that is equal to it).
fn derived_faces(
    json: &Value,
    features: &Features,
    family: &str,
    (field, base): (&str, &str),
    record: &Value,
) -> Vec<usize> {
    let (Some(pool), Some(defining)) = (
        json.get(base).and_then(Value::as_array),
        features.defining.get(base),
    ) else {
        panic!("family {family} takes its faces from {base}, which has no defining faces");
    };
    let members = record.get(field).and_then(Value::as_array);
    let mut taken = vec![false; pool.len()];
    let mut out = Vec::new();
    for m in members.into_iter().flatten() {
        if let Some(k) = (0..pool.len()).find(|&k| !taken[k] && same_member(base, &pool[k], m)) {
            taken[k] = true;
            out.extend(defining.get(k).into_iter().flatten().copied());
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The values at *path* in *v*: `a.b` steps into a nested record, `a[]` into each item of a
/// list. A missing field gives none.
fn resolve<'a>(v: &'a Value, path: &str, out: &mut Vec<&'a Value>) {
    let (head, rest) = match path.split_once('.') {
        Some((h, r)) => (h, Some(r)),
        None => (path, None),
    };
    let (name, each) = match head.strip_suffix("[]") {
        Some(n) => (n, true),
        None => (head, false),
    };
    let Some(x) = v.get(name) else {
        return;
    };
    let items: Vec<&Value> = match (each, x) {
        (true, Value::Array(items)) => items.iter().collect(),
        (true, _) => return,
        (false, x) => vec![x],
    };
    for item in items {
        match rest {
            Some(r) => resolve(item, r, out),
            None => out.push(item),
        }
    }
}

fn at<'a>(v: &'a Value, path: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    resolve(v, path, &mut out);
    out
}

/// A trait's text: a scalar as written, null `none`, a nested record `present`, a list its
/// items' texts sorted and joined.
fn trait_text(v: &Value) -> String {
    match v {
        Value::Null => "none".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(x) => x.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => joined(items.iter()),
        Value::Object(_) => "present".into(),
    }
}

fn joined<'a>(items: impl Iterator<Item = &'a Value>) -> String {
    let mut texts: Vec<String> = items.map(trait_text).collect();
    texts.sort();
    texts.join(",")
}

/// Sizes kept smallest first, as `<name>.<i>`.
fn insert_sorted(sizes: &mut BTreeMap<String, f64>, name: &str, mut values: Vec<f64>) {
    values.sort_by(f64::total_cmp);
    for (i, x) in values.into_iter().enumerate() {
        sizes.insert(format!("{name}.{i}"), x);
    }
}

fn point(v: &Value) -> Option<Vec<f64>> {
    v.as_array()?.iter().map(Value::as_f64).collect()
}

fn distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// The lengths of a chain of points' segments.
fn chain_lengths(points: &Value) -> Vec<f64> {
    let points: Vec<Vec<f64>> = points
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(point)
        .collect();
    points.windows(2).map(|w| distance(&w[0], &w[1])).collect()
}

/// The sizes a family's [`Role::Derived`] fields give: differences of frame-aligned bounds, and
/// distances and angles within a section, which a rigid motion leaves unchanged. Section lists
/// are kept smallest first since their order follows the frame; hole outer steps retain their
/// canonical mouth-to-bore order so diameter/depth pairs stay associated.
fn derived_sizes(family: &str, r: &Value, sizes: &mut BTreeMap<String, f64>) {
    let number = |path: &str| at(r, path).first().and_then(|v| v.as_f64());
    let span = |lo: &str, hi: &str| Some((number(hi)? - number(lo)?).abs());
    let interval = |path: &str| {
        let ends = at(r, path).first().and_then(|v| point(v))?;
        (ends.len() == 2).then(|| (ends[1] - ends[0]).abs())
    };
    let mut put = |name: &str, x: Option<f64>| {
        if let Some(x) = x {
            sizes.insert(name.into(), x);
        }
    };
    match family {
        "holes" => {
            if let Some(steps) = r.get("outer_steps").and_then(Value::as_array) {
                put("outer_steps.count", Some(steps.len() as f64));
                for (i, step) in steps.iter().enumerate() {
                    for dimension in ["diameter", "depth"] {
                        put(
                            &format!("outer_steps.{i}.{dimension}"),
                            step[dimension].as_f64(),
                        );
                    }
                }
            }
        }
        "turned_bodies" => {
            // Relative meridian dimensions have no world origin. Compare both axis senses
            // together, preserving diameter/station associations; face indices and splits'
            // traversal order are not dimensions. The comparison grid chooses a sense only;
            // published measurements and the existing numeric comparison bands stay exact.
            let signature = |sign: f64| {
                let mut rows = Vec::new();
                for p in r["profile"].as_array().into_iter().flatten() {
                    let kind = match p["kind"].as_str() {
                        Some("cylinder") => 0.,
                        Some("cone") => 1.,
                        Some("torus") => 2.,
                        Some("sphere") => 3.,
                        _ => 4.,
                    };
                    let mut ends = [
                        [
                            sign * p["start"][0].as_f64().unwrap(),
                            p["start"][1].as_f64().unwrap(),
                        ],
                        [
                            sign * p["end"][0].as_f64().unwrap(),
                            p["end"][1].as_f64().unwrap(),
                        ],
                    ];
                    ends.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
                    let arc_midpoint = p["arc_center"]
                        .as_array()
                        .map(|centre| {
                            let cs = centre[0].as_f64().unwrap();
                            let cr = centre[1].as_f64().unwrap();
                            let theta = (p["start"][1].as_f64().unwrap() - cr)
                                .atan2(p["start"][0].as_f64().unwrap() - cs);
                            let middle = theta + p["arc_sweep"].as_f64().unwrap() / 2.;
                            let radius = p["arc_radius"].as_f64().unwrap();
                            [
                                sign * (cs + radius * middle.cos()),
                                cr + radius * middle.sin(),
                            ]
                        })
                        .unwrap_or([0.; 2]);
                    rows.push(vec![
                        kind,
                        if p["side"] == "internal" { 1. } else { 0. },
                        ends[0][0],
                        ends[0][1],
                        ends[1][0],
                        ends[1][1],
                        sign * p["arc_center"][0].as_f64().unwrap_or(0.),
                        p["arc_center"][1].as_f64().unwrap_or(0.),
                        p["arc_radius"].as_f64().unwrap_or(0.),
                        p["arc_sweep"].as_f64().unwrap_or(0.).abs(),
                        arc_midpoint[0],
                        arc_midpoint[1],
                    ]);
                }
                rows.sort_by(|a, b| {
                    a.iter()
                        .zip(b)
                        .map(|(a, b)| py::round_to(*a, 8).total_cmp(&py::round_to(*b, 8)))
                        .find(|o| !o.is_eq())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                rows
            };
            let forward = signature(1.);
            let reverse = signature(-1.);
            let order = forward
                .iter()
                .flatten()
                .zip(reverse.iter().flatten())
                .map(|(a, b)| py::round_to(*a, 8).total_cmp(&py::round_to(*b, 8)))
                .find(|o| !o.is_eq())
                .unwrap_or(std::cmp::Ordering::Equal);
            let rows = if order.is_gt() { reverse } else { forward };
            for (i, row) in rows.iter().enumerate() {
                for (j, v) in row.iter().enumerate().skip(2) {
                    put(&format!("profile.{i}.{j}"), Some(*v));
                }
            }
            let mut spans: Vec<_> = r["bores"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|b| (b["span"][1].as_f64().unwrap() - b["span"][0].as_f64().unwrap()).abs())
                .collect();
            spans.sort_by(f64::total_cmp);
            for (i, v) in spans.iter().enumerate() {
                put(&format!("bores.length.{i}"), Some(*v));
            }
        }
        "plates" => put("thickness", span("lo", "hi")),
        "turned_steps" => put("length", span("lo", "hi")),
        "polygonal_bosses" => put("height", span("base", "top")),
        "polygonal_stock" => put("length", span("base", "top")),
        "gusset_ribs" => put("thickness", interval("thickness_bounds")),
        "freeform_surfaces" => {
            // The distances between the control net's four corner poles (a clamped support's
            // corners): they move together with the surface, so are sizes.
            let net: Option<Vec<Vec<Vec<f64>>>> = at(r, "support.poles").first().and_then(|p| {
                p.as_array()?
                    .iter()
                    .map(|row| row.as_array()?.iter().map(point).collect())
                    .collect()
            });
            if let Some(net) = net.filter(|n| n.first().is_some_and(|row| !row.is_empty())) {
                let (u, v) = (net.len() - 1, net[0].len() - 1);
                let corners = [&net[0][0], &net[0][v], &net[u][0], &net[u][v]];
                let lengths = (0..4)
                    .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
                    .map(|(i, j)| distance(corners[i], corners[j]))
                    .collect();
                insert_sorted(sizes, "support.corners", lengths);
            }
        }
        "local_steps" => {
            // Canonical section ordering is a presentation gauge: rotating principal
            // axes can exchange the names height/depth without changing either leg.
            insert_sorted(
                sizes,
                "section.legs",
                [number("height"), number("depth")]
                    .into_iter()
                    .flatten()
                    .collect(),
            );
        }
        "through_steps" => {
            let legs = at(r, "section").first().map(|s| chain_lengths(s));
            insert_sorted(sizes, "section.legs", legs.unwrap_or_default());
        }
        "edge_open_circular_pockets" => {
            put("run_length", interval("run_interval"));
            let opening = at(r, "section.opening").first().map(|s| chain_lengths(s));
            put("opening_width", opening.and_then(|o| o.first().copied()));
            // A sweep's sign follows the section's handedness in the frame.
            let sweeps = at(r, "section.segments[].sweep");
            let sweeps = sweeps.iter().filter_map(|v| v.as_f64()).map(f64::abs);
            insert_sorted(sizes, "section.segments.sweep", sweeps.collect());
            let mut lines = Vec::new();
            for s in at(r, "section.segments[]") {
                if s.get("kind").and_then(Value::as_str) == Some("line")
                    && let (Some(a), Some(b)) =
                        (s.get("start").and_then(point), s.get("end").and_then(point))
                {
                    lines.push(distance(&a, &b));
                }
            }
            insert_sorted(sizes, "section.segments.line", lines);
        }
        "edge_open_prismatic_recesses" => {
            put("run_length", interval("run_interval"));
            let (start, end) = (
                at(r, "section.opening.start")
                    .first()
                    .and_then(|v| point(v)),
                at(r, "section.opening.end").first().and_then(|v| point(v)),
            );
            put(
                "opening_width",
                start.zip(end).map(|(a, b)| distance(&a, &b)),
            );
            let walls = at(r, "section.wall_chain")
                .first()
                .map(|s| chain_lengths(s));
            insert_sorted(sizes, "section.walls", walls.unwrap_or_default());
        }
        "oriented_slots" => put("depth", interval("source.run_interval")),
        "risers" => put("height", span("z_lo", "z_hi")),
        "step_levels" => {
            let widths = [interval("x_span"), interval("y_span")];
            insert_sorted(sizes, "footprint", widths.into_iter().flatten().collect());
        }
        "pads" => {
            let extents = ["x", "y", "z"].map(|a| span(&format!("{a}0"), &format!("{a}1")));
            let axis = at(r, "axis")
                .first()
                .and_then(|v| v.as_str())
                .and_then(|a| ["x", "y", "z"].iter().position(|&n| n == a));
            if let (Some(axis), [Some(_), Some(_), Some(_)]) = (axis, extents) {
                put("height", extents[axis]);
                let across = (0..3).filter(|&i| i != axis).filter_map(|i| extents[i]);
                insert_sorted(sizes, "footprint", across.collect());
            }
        }
        "section_passages" => {
            put("run_length", interval("run_interval"));
            // The closed boundary's chords (an arc's too), whatever vertex it starts at.
            let mut points: Vec<Vec<f64>> = at(r, "section.boundary[].point")
                .iter()
                .filter_map(|v| point(v))
                .collect();
            if let Some(first) = points.first().cloned() {
                points.push(first);
            }
            let sides = points.windows(2).map(|w| distance(&w[0], &w[1])).collect();
            insert_sorted(sizes, "section.sides", sides);
        }
        "prismatic_through_cuts" => {
            put("run_length", interval("cut.run_interval"));
            // The closed boundary's chords (an arc's too), whatever vertex it starts at.
            let mut points: Vec<Vec<f64>> = at(r, "cut.section.boundary[].point")
                .iter()
                .filter_map(|v| point(v))
                .collect();
            if let Some(first) = points.first().cloned() {
                points.push(first);
            }
            let sides = points.windows(2).map(|w| distance(&w[0], &w[1])).collect();
            insert_sorted(sizes, "section.sides", sides);
        }
        "section_recesses" => {
            put("run_length", interval("geometry.run_interval"));
            // The boundary's chords (an arc's too), closed where the profile is, whatever vertex
            // it starts at; an open profile's opening is its width.
            let mut points: Vec<Vec<f64>> = at(r, "geometry.profile.boundary[].point")
                .iter()
                .filter_map(|v| point(v))
                .collect();
            let closed = at(r, "geometry.profile.closure")
                .first()
                .and_then(|v| v.as_str())
                == Some("closed");
            if closed && let Some(first) = points.first().cloned() {
                points.push(first);
            }
            let opening = at(r, "geometry.profile.opening")
                .first()
                .map(|s| chain_lengths(s));
            put("opening_width", opening.and_then(|o| o.first().copied()));
            // A cylindrical end's radius, at whichever end the run's sense puts it.
            let radius = ["low", "high"]
                .iter()
                .find_map(|end| number(&format!("geometry.ends.{end}.surface.radius")));
            put("end_radius", radius);
            let bulges = at(r, "geometry.profile.boundary[].bulge");
            let bulges = bulges.iter().filter_map(|v| v.as_f64()).map(f64::abs);
            insert_sorted(sizes, "section.bulges", bulges.collect());
            let sides = points.windows(2).map(|w| distance(&w[0], &w[1])).collect();
            insert_sorted(sizes, "section.sides", sides);
        }
        "prismatic_pockets" => {
            // The closed polygon's sides, whatever corner it starts at.
            let mut points: Vec<Vec<f64>> = at(r, "section")
                .first()
                .and_then(|s| s.as_array())
                .into_iter()
                .flatten()
                .filter_map(point)
                .collect();
            if let Some(first) = points.first().cloned() {
                points.push(first);
            }
            let sides = points.windows(2).map(|w| distance(&w[0], &w[1])).collect();
            insert_sorted(sizes, "section.sides", sides);
        }
        _ => {}
    }
}

fn feature(
    faces: &[FaceFingerprint],
    id: String,
    table: &FamilyFingerprint,
    record: &Value,
    mut own: Vec<usize>,
    published_axis: &impl Fn(&str, &str, &Value) -> Option<V3>,
) -> FeatureFingerprint {
    own.sort_unstable();
    own.dedup();
    let mut traits = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    let mut axis = None;
    if table.variants
        && let Some(fields) = record.as_object()
    {
        let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
        traits.insert("shape".into(), keys.join(","));
    }
    for &(path, role) in table.fields {
        let values = at(record, path);
        if values.is_empty() {
            continue;
        }
        let each = path.contains("[]");
        match role {
            Size => {
                let mut numbers = Vec::new();
                for v in &values {
                    match v {
                        Value::Number(x) => numbers.extend(x.as_f64()),
                        Value::Array(items) => {
                            numbers.extend(items.iter().filter_map(Value::as_f64))
                        }
                        _ => {}
                    }
                }
                match (each, values[0]) {
                    (false, Value::Number(_)) => {
                        sizes.insert(path.into(), numbers[0]);
                    }
                    _ => insert_sorted(&mut sizes, &path.replace("[]", ""), numbers),
                }
            }
            Axis => {
                if axis.is_none() {
                    axis = published_axis(&id, path, values[0]).or_else(|| read_axis(values[0]));
                }
            }
            Trait => {
                let text = if each {
                    joined(values.into_iter())
                } else {
                    trait_text(values[0])
                };
                traits.insert(path.replace("[]", ""), text);
            }
            Count(name) => {
                let n = values[0].as_array().map_or(0, Vec::len);
                sizes.insert(name.into(), n as f64);
            }
            Placement | Derived | Ignored => {}
        }
    }
    derived_sizes(table.family, record, &mut sizes);
    let position = weighted_centroid(faces, &own);
    let neighbourhood = neighbourhood(faces, &own);
    FeatureFingerprint {
        id,
        family: table.family.into(),
        traits,
        sizes,
        axis,
        position,
        faces: own,
        neighbourhood,
    }
}

/// A unit vector from a 3-vector or an axis letter.
fn read_axis(v: &Value) -> Option<V3> {
    match v {
        Value::String(s) => match s.as_str() {
            "x" => Some([1.0, 0.0, 0.0]),
            "y" => Some([0.0, 1.0, 0.0]),
            "z" => Some([0.0, 0.0, 1.0]),
            _ => None,
        },
        Value::Array(items) if items.len() == 3 => {
            let c: Vec<f64> = items.iter().filter_map(Value::as_f64).collect();
            (c.len() == 3)
                .then(|| geom::unit([c[0], c[1], c[2]]))
                .flatten()
        }
        _ => None,
    }
}

/// The area-weighted centroid of the faces (each face's centroid weighted equally where an
/// area is unknown and no face has one).
fn weighted_centroid(faces: &[FaceFingerprint], own: &[usize]) -> Option<V3> {
    if own.is_empty() {
        return None;
    }
    let (mut sum, mut weight) = ([0.0; 3], 0.0);
    for &f in own {
        if let Some(a) = faces[f].area {
            sum = geom::add(sum, geom::scale(faces[f].centroid, a));
            weight += a;
        }
    }
    if weight > 0.0 {
        return Some(geom::scale(sum, 1.0 / weight));
    }
    let sum = own
        .iter()
        .fold([0.0; 3], |s, &f| geom::add(s, faces[f].centroid));
    Some(geom::scale(sum, 1.0 / own.len() as f64))
}

fn neighbourhood(faces: &[FaceFingerprint], own: &[usize]) -> Vec<String> {
    let mut out = Vec::new();
    for &f in own {
        for (g, arc) in &faces[f].neighbours {
            let side = if own.contains(g) { "in" } else { "out" };
            out.push(format!(
                "{}>{side}:{}:{arc}",
                faces[f].surface, faces[*g].surface
            ));
        }
    }
    out.sort();
    out
}

/// Normals at an edge closer than this to parallel (or to opposite) leave convex and concave
/// to round-off: about 2.6 degrees (a B-spline blend exported as an approximate tangent, a knife
/// edge where a face folds back onto its neighbour).
const NEAR_TANGENT: f64 = 1e-3;

/// How the solid turns where faces *a* and *b* meet, as the fingerprint records it: the kernel's
/// [`Part::arc`], except that where every shared edge is nearly tangent the pair is `smooth`,
/// and where every one is nearly folded `unknown`, since which way such an edge turns can change
/// when the part is merely moved. This departs from the kernel's labels, and so from
/// draftwright's bridge, on those edges only: a genuine crease shallower than about 2.6 degrees
/// is recorded `smooth`.
fn arc_label(part: &Part, a: usize, b: usize) -> &'static str {
    let arc = part.arc(a, b);
    let (mut tangent, mut folded, mut other) = (0, 0, 0);
    for e in part.shared_edges(a, b) {
        let p = part.edges[e].midpoint();
        let normal = |f: usize| {
            let (u, v) = part.faces[f].surface.parameters(p, None)?;
            part.face_normal(f, u, v)
        };
        match (normal(a), normal(b)) {
            (Some(x), Some(y)) => {
                let c = geom::dot(x, y);
                if 1.0 - c < NEAR_TANGENT {
                    tangent += 1;
                } else if 1.0 + c < NEAR_TANGENT {
                    folded += 1;
                } else {
                    other += 1;
                }
            }
            _ => other += 1,
        }
    }
    match (tangent, folded, other) {
        (t, 0, 0) if t > 0 => "smooth",
        (0, f, 0) if f > 0 => "unknown",
        _ => arc_name(arc),
    }
}

fn arc_name(arc: Option<Arc>) -> &'static str {
    match arc {
        Some(Arc::Convex) => "convex",
        Some(Arc::Concave) => "concave",
        Some(Arc::Smooth) => "smooth",
        _ => "unknown",
    }
}

/// The face's surface as the recognisers see it: a freeform face that is exactly an analytic
/// surface counts as that surface.
fn effective(part: &Part, i: usize) -> &Surface {
    match &part.faces[i].surface {
        Surface::Freeform { .. } => part.recovered(i).unwrap_or(&part.faces[i].surface),
        s => s,
    }
}

fn surface_name(s: &Surface) -> String {
    match s {
        Surface::Plane { .. } => "plane".into(),
        Surface::Cylinder { .. } => "cylinder".into(),
        Surface::Cone { .. } => "cone".into(),
        Surface::Sphere { .. } => "sphere".into(),
        Surface::Torus { .. } => "torus".into(),
        Surface::Freeform { kind, .. } | Surface::Other { kind } => kind.to_lowercase(),
    }
}

#[derive(Clone, Copy)]
struct FaceMeasure {
    area: f64,
    centroid: V3,
    area_vector: V3,
}

fn face(part: &Part, i: usize, measured: Option<FaceMeasure>) -> FaceFingerprint {
    let surface = effective(part, i);
    // A face whose quadrature gives no area (a kernel failure: such faces exist) is fingerprinted
    // as one of unknown area.
    let moments = measured
        .or_else(|| {
            part.face_moments(i).map(|m| FaceMeasure {
                area: m.area,
                centroid: m.centroid,
                area_vector: m.area_vector,
            })
        })
        .filter(|m| m.area > 1e-9);
    let centroid = moments.map(|m| m.centroid).unwrap_or_else(|| {
        let (mut sum, mut n) = ([0.0; 3], 0usize);
        for e in part.face_edges(i) {
            for p in &part.edges[e].samples {
                sum = geom::add(sum, *p);
                n += 1;
            }
        }
        geom::scale(sum, 1.0 / n.max(1) as f64)
    });
    let normal = moments.and_then(|m| {
        let length = geom::norm(m.area_vector);
        (length > 1e-6 * m.area).then(|| geom::scale(m.area_vector, 1.0 / length))
    });
    let mut parameters = BTreeMap::new();
    let (axis, support) = match surface {
        Surface::Plane { frame } => {
            let n = if part.faces[i].reversed {
                geom::scale(frame.z, -1.0)
            } else {
                frame.z
            };
            // A recovered plane is unoriented: take the side the area vector is on.
            let n = match normal {
                Some(m) if geom::dot(m, n) < 0.0 => geom::scale(n, -1.0),
                _ => n,
            };
            (Some(n), Some(centroid))
        }
        Surface::Cylinder { frame, radius } => {
            parameters.insert("radius".into(), *radius);
            (Some(frame.z), Some(frame.origin))
        }
        Surface::Cone {
            frame,
            radius,
            semi_angle,
        } => {
            parameters.insert("semi_angle".into(), semi_angle.abs().to_degrees());
            let apex = surface.cone_apex().unwrap_or_else(|| {
                geom::add(
                    frame.origin,
                    geom::scale(frame.z, -radius / semi_angle.tan()),
                )
            });
            (Some(frame.z), Some(apex))
        }
        Surface::Sphere { frame, radius } => {
            parameters.insert("radius".into(), *radius);
            (None, Some(frame.origin))
        }
        Surface::Torus {
            frame,
            major,
            minor,
        } => {
            parameters.insert("major_radius".into(), *major);
            parameters.insert("minor_radius".into(), *minor);
            (Some(frame.z), Some(frame.origin))
        }
        _ => (None, None),
    };
    let name = surface_name(surface);
    let neighbours: Vec<(usize, String)> = part
        .neighbours(i)
        .into_iter()
        .map(|g| (g, arc_label(part, i, g).to_string()))
        .collect();
    let mut adjacency: Vec<String> = neighbours
        .iter()
        .map(|(g, arc)| format!("{}:{arc}", surface_name(effective(part, *g))))
        .collect();
    adjacency.sort();
    FaceFingerprint {
        index: i,
        surface: name,
        parameters,
        axis,
        support,
        area: moments.map(|m| m.area),
        centroid,
        normal,
        neighbours,
        adjacency,
    }
}
