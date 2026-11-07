//! Records recognised in the part's own frame, reported in the file's coordinates (the
//! maintainer's decision on review M8: recognition canonicalises the frame first, so results do
//! not depend on how the part was placed).
//!
//! [`map_features`] carries every record of a [`Features`] through a rigid motion: points by the
//! whole motion, directions, normals and axes by its rotation, sizes, counts, faces and other
//! scalars unchanged. Many records also name a principal axis by letter (`'x'`, `"y"`) and give
//! coordinates measured along such axes (a plate's `lo` and `hi`, a slot's `w_center`, a
//! through step's two-dimensional section). Where the rotation is a signed permutation, as it is
//! for a part whose frame is aligned with the file's axes, those are carried exactly: the letter
//! renamed, the coordinate moved and, along a reversed axis, negated (an interval's ends swapped,
//! a sign flipped). Other directions are published as file-coordinate unit vectors by
//! [`crate::published_records`]. Distances retain a record-owned measuring basis and an explicit
//! file-coordinate origin. Transient pending field paths connect the exhaustive typed mapper to
//! that publication boundary; they are consumed there and are never part of the public payload.
//!
//! Every record is destructured in full, with no `..`: a field or family added later fails to
//! compile here until its mapping is decided.
//!
//! Positions are published once, in the file's coordinates: a position the recogniser rounds
//! keeps the value it rounded from ([`Placed`]), the mapper carries that unrounded value through
//! the motion and rounds it on the field's grid there (a decimal half-way point within the
//! coordinate floor taking the even neighbour), so a published position is on its grid, never
//! a frame-rounded value plus the frame's origin. A position derived from published ones (a
//! pattern's centre, a mirror plane, an oriented slot's centre) is derived again from the
//! published values in the file. Plate face planes stay exact during recognition and for
//! fingerprints; their endpoints are published once on the file's 0.001 mm grid. Other exact
//! positions (a boss's location) are carried unrounded. [`Mapped::fingerprinted`] keeps the published values
//! as found in the frame, carried without rounding again, so fingerprints stay intrinsic to the
//! part. Orders and signs a recogniser chooses by coordinates (records
//! within a family, a direction's canonical sense, which end of a through hole is its entry, a
//! section's starting vertex and direction) are the frame's and are kept, so they do not depend
//! on placement either; they can differ from caller-space recognition's, which chooses them by
//! the file's axes (`tests/fixtures/references/framed_document.json` lists each such difference).

use std::collections::BTreeMap;

use crate::features::Features;
use crate::features::angled_steps::AngledStep;
use crate::features::blends::{Blend, BlendPath, CircularBlendPath, StraightBlendPath};
use crate::features::body::BodyKey;
use crate::features::bosses::BossRecord;
use crate::features::chamfers::Chamfer;
use crate::features::circular_blind_steps::CircularBlindStep;
use crate::features::circular_face_patterns::CircularFacePattern;
use crate::features::countersinks::CounterSink;
use crate::features::cylindrical_end_surface::CylindricalEndSurface;
use crate::features::edge_open_circular::{
    EdgeOpenCircularPocket, OpenCircularSection, OpenCircularSectionSegment,
};
use crate::features::edge_open_prismatic::{
    EdgeOpenPrismaticRecess, OpenPolygonalSection, OpenSectionOpening,
};
use crate::features::fillets::{Fillet, FilletAxis};
use crate::features::flats::Flat;
use crate::features::freeform_surfaces::{
    BSplineSurfaceSupport, FreeformSurface, SurfaceContinuityLink,
};
use crate::features::grooves::Groove;
use crate::features::gussets::{GussetRib, GussetRibPattern};
use crate::features::hole_patterns::HolePattern;
use crate::features::holes::{CounterBore, HoleRecord};
use crate::features::interior_voids::InteriorVoid;
use crate::features::levels::{FaceLevel, RiserEvidence};
use crate::features::oblique_through_steps::ObliqueThroughStep;
use crate::features::oriented_chamfers::OrientedChamfer;
use crate::features::oriented_slots::{OrientedSlot, OrientedSlotPattern};
use crate::features::pads::RaisedPad;
use crate::features::paired_ramp_steps::PairedRampStep;
use crate::features::passages::{
    PassageEnds, PassageFrame, PassageSection, PassageSectionVertex, SectionPassage,
};
use crate::features::pattern_geometry::{Located, mean_location, plane_uv};
use crate::features::placed::{Coordinates, Grid, Placed};
use crate::features::plates::Plate;
use crate::features::polygonal_bosses::PolygonalPrism;
use crate::features::prismatic_pockets::PrismaticPocket;
use crate::features::profiled_bores::DoubleDBore;
use crate::features::recess_patterns::{PocketPattern, SlotPattern};
use crate::features::recess_records::{Channel, Pocket, Recess, Slot};
use crate::features::rectangular_blind_slots::RectangularBlindSlot;
use crate::features::repeating_profiles::RepeatingRadialProfile;
use crate::features::round_bottom_slots::RoundBottomBlindSlot;
use crate::features::section_recess::{
    EndSurface, PlanarEndTerm, PlanarEnvelopeEndSurface, SectionEnd, SectionRecess,
    SectionRecessArray, SectionRecessEnds, SectionRecessGeometry, SectionRecessPattern,
};
use crate::features::sheet_metal::{
    FlatOverlapWitness, FlatPatternPlan, FormedSheetFeature, SheetBend, SheetEdgeTreatment,
    SheetFlange, SheetMetalBody, UnfoldedBendStrip, UnfoldedFlangeFace,
};
use crate::features::thin_walls::{ShellHistoryHint, ThinWallBody, UnpairedWallFace, WallFacePair};
use crate::features::through_steps::ThroughStep;
use crate::features::turned::TurnedProfileKey;
use crate::features::turned_steps::TurnedStep;
use crate::frames::PartFrame;
use crate::kernel::brep::Part;
use crate::kernel::geom::{COORD_FLOOR, V3, cross};
use crate::kernel::py;
use crate::kernel::step::Placement;

/// A rigid motion, `p ↦ rotation · p + translation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rigid {
    pub rotation: [[f64; 3]; 3],
    pub translation: V3,
}

impl Rigid {
    /// The motion taking the frame's relative coordinates to the caller's ([`PartFrame::to_world`]).
    pub fn from_frame(frame: &PartFrame) -> Rigid {
        let (x, y, z) = (frame.x, frame.y, frame.z);
        Rigid {
            rotation: [0, 1, 2].map(|i| [x[i], y[i], z[i]]),
            translation: frame.origin,
        }
    }

    /// The motion a reader's placement applies.
    pub fn from_placement(p: &Placement) -> Rigid {
        Rigid {
            rotation: p.map(|row| [row[0], row[1], row[2]]),
            translation: p.map(|row| row[3]),
        }
    }

    /// Apply `before`, then this motion.
    pub fn compose(&self, before: &Rigid) -> Rigid {
        Rigid {
            rotation: std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    (0..3)
                        .map(|k| self.rotation[i][k] * before.rotation[k][j])
                        .sum()
                })
            }),
            translation: self.point(before.translation),
        }
    }

    pub fn inverse(&self) -> Rigid {
        let r = self.rotation;
        let rotation = [0, 1, 2].map(|i| [r[0][i], r[1][i], r[2][i]]);
        let translation = [0, 1, 2].map(|i| -py::dot(&rotation[i], &self.translation));
        Rigid {
            rotation,
            translation,
        }
    }

    pub fn point(&self, p: V3) -> V3 {
        [0, 1, 2].map(|i| py::dot(&self.rotation[i], &p) + self.translation[i])
    }

    pub fn direction(&self, d: V3) -> V3 {
        [0, 1, 2].map(|i| py::dot(&self.rotation[i], &d))
    }

    /// This motion with its rotation replaced by the nearest signed permutation, when that moves
    /// no point within *reach* of the origin by more than the kernel's coordinate floor
    /// ([`COORD_FLOOR`], the round-off publication already treats as no movement); otherwise
    /// this motion. A part aligned with its file's axes up to round-off (#70: sm-hanger, 2.4e-9
    /// rad) is then published in the file's axes, while its recognition, in the frame as
    /// inferred, is untouched.
    pub fn aligned_within(&self, reach: f64) -> Rigid {
        let r = self.rotation;
        let mut p = [[0.0; 3]; 3];
        for i in 0..3 {
            let w = (0..3)
                .max_by(|&a, &b| r[a][i].abs().total_cmp(&r[b][i].abs()))
                .expect("three rows");
            p[w][i] = 1f64.copysign(r[w][i]);
        }
        let det = p[0][0] * (p[1][1] * p[2][2] - p[1][2] * p[2][1])
            - p[0][1] * (p[1][0] * p[2][2] - p[1][2] * p[2][0])
            + p[0][2] * (p[1][0] * p[2][1] - p[1][1] * p[2][0]);
        // The Frobenius norm bounds how far the change moves a unit vector.
        let deviation = (0..3)
            .flat_map(|w| (0..3).map(move |i| (w, i)))
            .map(|(w, i)| (r[w][i] - p[w][i]).powi(2))
            .sum::<f64>()
            .sqrt();
        if det == 1.0 && (deviation == 0.0 || deviation * reach <= COORD_FLOOR) {
            Rigid {
                rotation: p,
                translation: self.translation,
            }
        } else {
            *self
        }
    }

    /// Each source axis's target axis and sign, when the rotation is a signed permutation
    /// (every entry exactly 0 or ±1). A frame within round-off of one is published through it
    /// ([`Rigid::aligned_within`]).
    pub fn axes(&self) -> Option<[(usize, f64); 3]> {
        let r = self.rotation;
        let mut out = [(0, 0.0); 3];
        let mut used = [false; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            let rows: Vec<usize> = (0..3).filter(|&w| r[w][i] != 0.0).collect();
            let &[w] = rows.as_slice() else {
                return None;
            };
            if r[w][i].abs() != 1.0 || used[w] {
                return None;
            }
            used[w] = true;
            *slot = (w, r[w][i]);
        }
        Some(out)
    }
}

/// Records carried through a motion, with self-contained file-coordinate payloads.
pub struct Mapped {
    pub features: crate::published_records::PublishedFeatures,
    /// Exact-motion view for intrinsic fingerprints, without publication rounding.
    pub fingerprinted: crate::published_records::PublishedFeatures,
    pub motion: Rigid,
}

/// *features*, recognised on *working* (the framed working part), in the file's coordinates.
/// They are published through the frame's motion, or through the signed permutation it is
/// within round-off of across the working part ([`Rigid::aligned_within`]).
pub fn to_file(features: Features, frame: &PartFrame, working: &Part) -> Mapped {
    let exact = Rigid::from_frame(frame);
    let bounds = working.bounds();
    let reach = (0..3)
        .map(|i| bounds.min[i].abs().max(bounds.max[i].abs()).powi(2))
        .sum::<f64>()
        .sqrt();
    let published = exact.aligned_within(reach);
    carry_both(features, &published, &exact, reach)
}

const LETTERS: [char; 3] = ['x', 'y', 'z'];

/// A record's axis letter as an index.
fn index(letter: char) -> usize {
    LETTERS
        .iter()
        .position(|&l| l == letter)
        .unwrap_or_else(|| panic!("{letter:?} is not an axis letter"))
}

fn index_str(letter: &str) -> usize {
    let mut chars = letter.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => index(c),
        _ => panic!("{letter:?} is not an axis letter"),
    }
}

/// The two axes other than *axis*, in increasing order.
fn others(axis: usize) -> [usize; 2] {
    match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    }
}

/// One record's mapping: the motion, and the paths it left relative.
struct Out<'a> {
    motion: &'a Rigid,
    /// Whether positions are published in the target space ([`Placed::carried`]): their
    /// unrounded values carried and rounded once there. Otherwise published values are carried
    /// as they are, as fingerprints read them.
    publish: bool,
    axes: Option<[(usize, f64); 3]>,
    /// Prepended to every path (a pattern's members: `holes[].`).
    prefix: &'static str,
    relative: Vec<String>,
}

impl<'a> Out<'a> {
    fn new(motion: &'a Rigid, publish: bool, prefix: &'static str) -> Out<'a> {
        Out {
            motion,
            publish,
            axes: motion.axes(),
            prefix,
            relative: Vec::new(),
        }
    }

    fn mark(&mut self, path: &str) {
        self.relative.push(format!("{}{path}", self.prefix));
    }

    fn point(&self, p: V3) -> V3 {
        self.motion.point(p)
    }

    fn direction(&self, d: V3) -> V3 {
        self.motion.direction(d)
    }

    fn points(&self, ps: Vec<V3>) -> Vec<V3> {
        ps.into_iter().map(|p| self.point(p)).collect()
    }

    fn translation(&self, w: usize) -> f64 {
        self.motion.translation[w]
    }

    /// Source axis *axis*'s target axis and sign, or a pending vector conversion when the rotation
    /// is not a signed permutation.
    fn target(&mut self, axis: usize, path: &str) -> Option<(usize, f64)> {
        match self.axes {
            Some(axes) => Some(axes[axis]),
            None => {
                self.mark(path);
                None
            }
        }
    }

    fn letter(&mut self, letter: char, path: &str) -> char {
        match self.target(index(letter), path) {
            Some((w, _)) => LETTERS[w],
            None => letter,
        }
    }

    fn letter_string(&mut self, letter: String, path: &str) -> String {
        match self.target(index_str(&letter), path) {
            Some((w, _)) => LETTERS[w].to_string(),
            None => letter,
        }
    }

    /// A coordinate along source axis *axis*.
    fn coordinate(&mut self, axis: usize, v: f64, path: &str) -> f64 {
        match self.target(axis, path) {
            Some((w, s)) => self.translation(w) + s * v,
            None => v,
        }
    }

    /// An ordered interval along source axis *axis*.
    fn interval(&mut self, axis: usize, (lo, hi): (f64, f64), path: &str) -> (f64, f64) {
        match self.target(axis, path) {
            Some((w, s)) => {
                let (a, b) = (self.translation(w) + s * lo, self.translation(w) + s * hi);
                if s > 0.0 { (a, b) } else { (b, a) }
            }
            None => (lo, hi),
        }
    }

    fn interval2(&mut self, axis: usize, [lo, hi]: [f64; 2], path: &str) -> [f64; 2] {
        let (lo, hi) = self.interval(axis, (lo, hi), path);
        [lo, hi]
    }

    /// A sign (±1) along source axis *axis*.
    fn sign(&mut self, axis: usize, sign: i32, path: &str) -> i32 {
        match self.target(axis, path) {
            Some((_, s)) if s < 0.0 => -sign,
            _ => sign,
        }
    }

    /// A point in the two axes other than source axis *axis* (absolute coordinates, in
    /// increasing axis order), in the target's two axes other than *axis*'s image.
    fn across(&mut self, axis: usize, p: [f64; 2], path: &str) -> [f64; 2] {
        let [a, b] = others(axis);
        let Some(axes) = self.axes else {
            self.mark(path);
            return p;
        };
        let ((wa, sa), (wb, sb)) = (axes[a], axes[b]);
        let (va, vb) = (
            self.translation(wa) + sa * p[0],
            self.translation(wb) + sb * p[1],
        );
        if wa < wb { [va, vb] } else { [vb, va] }
    }

    /// Whether [`Out::across`] reverses the section's handedness (1 or −1; 1 when it is not
    /// mapped).
    fn across_turn(&self, axis: usize) -> f64 {
        let [a, b] = others(axis);
        match self.axes {
            Some(axes) => {
                let ((wa, sa), (wb, sb)) = (axes[a], axes[b]);
                sa * sb * if wa < wb { 1.0 } else { -1.0 }
            }
            None => 1.0,
        }
    }

    /// A box in source axes: `min` then `max` per axis, as [`BodyKey`]'s first six entries
    /// (`interleaved` false) or a turned profile's `[min x, max x, min y, ...]` (`true`).
    fn bounds(&mut self, b: &[f64], interleaved: bool, path: &str) -> Vec<f64> {
        let Some(axes) = self.axes else {
            self.mark(path);
            return b.to_vec();
        };
        let at = |axis: usize, high: bool| {
            if interleaved {
                2 * axis + usize::from(high)
            } else {
                3 * usize::from(high) + axis
            }
        };
        let mut out = b.to_vec();
        for (i, &(w, s)) in axes.iter().enumerate() {
            let (lo, hi) = (b[at(i, false)], b[at(i, true)]);
            let t = self.translation(w);
            let (lo, hi) = if s > 0.0 {
                (t + lo, t + hi)
            } else {
                (t - hi, t - lo)
            };
            out[at(w, false)] = lo;
            out[at(w, true)] = hi;
        }
        out
    }

    /// A solid's key: its box, then its volume and area.
    fn body_key(&mut self, key: Option<BodyKey>, path: &str) -> Option<BodyKey> {
        key.map(|k| {
            k.carried(self.publish, |k| {
                let mut out = self.bounds(&k[..6], false, path);
                out.extend_from_slice(&k[6..]);
                out
            })
        })
    }

    /// A position carried by *f* ([`Placed::carried`]).
    fn placed<T: Coordinates>(
        &mut self,
        p: Placed<T>,
        mut f: impl FnMut(&mut Self, T) -> T,
    ) -> Placed<T> {
        let publish = self.publish;
        p.carried(publish, |v| f(self, v))
    }

    /// An ordered interval of placed coordinates along source axis *axis*: each end carried,
    /// and the ends swapped along a reversed axis, as [`Out::interval`].
    fn placed_interval(
        &mut self,
        axis: usize,
        (lo, hi): (Placed<f64>, Placed<f64>),
        path: &str,
    ) -> (Placed<f64>, Placed<f64>) {
        let lo = self.placed(lo, |out, v| out.coordinate(axis, v, path));
        let hi = self.placed(hi, |out, v| out.coordinate(axis, v, path));
        match self.axes {
            Some(axes) if axes[axis].1 < 0.0 => (hi, lo),
            _ => (lo, hi),
        }
    }

    /// A pattern's centre, the mean of its members' locations (*members*, already carried):
    /// published, it is that mean of their published locations, as the pattern measured it;
    /// otherwise the centre is carried as it is.
    fn pattern_center<M: Located>(&self, center: V3, members: &[M]) -> V3 {
        if self.publish {
            mean_location(&members.iter().collect::<Vec<_>>())
        } else {
            self.point(center)
        }
    }

    /// A placed point across source axis *axis* ([`Out::across`]).
    fn placed_across(&mut self, axis: usize, p: Placed<[f64; 2]>, path: &str) -> Placed<[f64; 2]> {
        self.placed(p, |out, p| out.across(axis, p, path))
    }

    /// A placed point carried by the whole motion.
    fn placed_point(&mut self, p: Placed<V3>) -> Placed<V3> {
        self.placed(p, |out, v| out.point(v))
    }

    /// A pattern's orientation, in degrees in its plane's basis ([`plane_uv`] of *normal*), and
    /// reduced modulo *period*: the same direction in the target's basis of the turned normal.
    fn pattern_angle(&self, normal: V3, degrees: f64, period: f64) -> f64 {
        let (u, v) = plane_uv(normal);
        let (s, c) = degrees.to_radians().sin_cos();
        let direction = self.direction([0, 1, 2].map(|i| c * u[i] + s * v[i]));
        let (u, v) = plane_uv(self.direction(normal));
        let turned = py::dot(&direction, &v)
            .atan2(py::dot(&direction, &u))
            .to_degrees();
        // Round-off is cleared before the reduction, so 0 does not come back as 180.
        let reduced = py::round_to(py::modulo(py::round_to(turned, 9), period), 2);
        if reduced >= period {
            reduced - period
        } else {
            reduced
        }
    }

    fn done(mut self) -> Vec<String> {
        self.relative.sort();
        self.relative.dedup();
        self.relative
    }
}

/// *features*, carried through *motion*: see the module documentation.
pub fn map_features(
    features: impl Into<crate::published_records::MappingSource>,
    motion: &Rigid,
) -> Mapped {
    let source = features.into();
    let motion = motion.compose(&source.motion);
    let published = motion.aligned_within(source.reach);
    carry_both(source.features, &published, &motion, source.reach)
}

/// *features* published through *published*, and carried as found through *exact*.
fn carry_both(features: Features, published: &Rigid, exact: &Rigid, reach: f64) -> Mapped {
    let source = std::sync::Arc::new(features.clone());
    let fingerprinted = carry(features.clone(), exact, false, source.clone(), reach);
    let features = carry(features, published, true, source, reach);
    Mapped {
        features,
        fingerprinted,
        motion: *published,
    }
}

/// [`map_features`]'s records, their positions published in the target space or carried as
/// they are (*publish*), and the fields left relative.
fn carry(
    features: Features,
    motion: &Rigid,
    publish: bool,
    source: std::sync::Arc<Features>,
    reach: f64,
) -> crate::published_records::PublishedFeatures {
    let Features {
        failures,
        run_mode,
        discovery_refusals,
        evidence_status,
        fillets,
        cylindrical_profiles,
        chamfers,
        bosses,
        angled_steps,
        flats,
        paired_ramp_steps,
        oriented_chamfers,
        circular_face_patterns,
        oblique_through_steps,
        circular_blind_steps,
        holes,
        countersinks,
        hole_patterns,
        gusset_ribs,
        gusset_rib_patterns,
        thin_wall_bodies,
        interior_voids,
        interior_void_refusals,
        through_steps,
        local_steps,
        turned_steps,
        grooves,
        annular_grooves,
        face_grooves,
        plates,
        turned_bodies,
        round_bottom_blind_slots,
        semicircular_blind_slots,
        rectangular_blind_slots,
        double_d_bores,
        edge_open_circular_pockets,
        edge_open_prismatic_recesses,
        blends,
        sheet_metal_bodies,
        slots,
        pockets,
        channels,
        v_channels,
        rectangular_through_notches,
        slot_patterns,
        pocket_patterns,
        repeating_radial_profiles,
        freeform_surfaces,
        modelled_threads,
        helical_springs,
        modelled_thread_refusals,
        polygonal_bosses,
        polygonal_stock,
        section_passages,
        prismatic_pockets,
        oriented_slots,
        oriented_slot_patterns,
        pads,
        step_levels,
        risers,
        section_recesses,
        section_recess_refusals,
        section_recess_patterns,
        section_recess_refusal,
        defining,
    } = features;
    let mut relative = BTreeMap::new();
    let mut family = |name: &str, n: usize, paths: Vec<String>| {
        if !paths.is_empty() {
            relative.insert(format!("{name}/{n}"), paths);
        }
    };
    macro_rules! each {
        ($name:literal, $records:expr, $f:expr) => {
            $records
                .into_iter()
                .enumerate()
                .map(|(n, r)| {
                    let mut out = Out::new(motion, publish, "");
                    let r = ($f)(&mut out, r);
                    family($name, n, out.done());
                    r
                })
                .collect()
        };
    }
    let void_count = interior_voids.len();
    let features = Features {
        failures,
        run_mode,
        discovery_refusals,
        evidence_status,
        fillets: each!("fillets", fillets, fillet),
        cylindrical_profiles: each!(
            "cylindrical_profiles",
            cylindrical_profiles,
            cylindrical_profile
        ),
        chamfers: each!("chamfers", chamfers, chamfer),
        bosses: each!("bosses", bosses, boss),
        angled_steps: each!("angled_steps", angled_steps, angled_step),
        flats: each!("flats", flats, flat),
        paired_ramp_steps: each!("paired_ramp_steps", paired_ramp_steps, paired_ramp_step),
        oriented_chamfers: each!("oriented_chamfers", oriented_chamfers, oriented_chamfer),
        circular_face_patterns: each!(
            "circular_face_patterns",
            circular_face_patterns,
            circular_face_pattern
        ),
        oblique_through_steps: each!(
            "oblique_through_steps",
            oblique_through_steps,
            oblique_through_step
        ),
        circular_blind_steps: each!(
            "circular_blind_steps",
            circular_blind_steps,
            circular_blind_step
        ),
        holes: each!("holes", holes, hole),
        countersinks: each!("countersinks", countersinks, countersink),
        hole_patterns: each!("hole_patterns", hole_patterns, hole_pattern),
        gusset_ribs: each!("gusset_ribs", gusset_ribs, gusset_rib),
        gusset_rib_patterns: each!(
            "gusset_rib_patterns",
            gusset_rib_patterns,
            gusset_rib_pattern
        ),
        thin_wall_bodies: each!("thin_wall_bodies", thin_wall_bodies, thin_wall_body),
        interior_voids: each!("interior_voids", interior_voids, interior_void),
        interior_void_refusals: interior_void_refusals
            .into_iter()
            .enumerate()
            .map(|(n, mut r)| {
                let mut out = Out::new(motion, publish, "");
                r.candidate = interior_void(&mut out, r.candidate);
                family("interior_voids", void_count + n, out.done());
                r
            })
            .collect(),
        through_steps: each!("through_steps", through_steps, through_step),
        local_steps: each!("local_steps", local_steps, local_step),
        turned_steps: each!("turned_steps", turned_steps, turned_step),
        grooves: each!("grooves", grooves, groove),
        annular_grooves: each!(
            "annular_grooves",
            annular_grooves,
            |out: &mut Out, mut r: crate::features::annular_grooves::AnnularGroove| {
                r.axis = out.direction(r.axis);
                r.floor_center = out.point(r.floor_center);
                r
            }
        ),
        face_grooves: each!(
            "face_grooves",
            face_grooves,
            |out: &mut Out, mut r: crate::features::face_grooves::FaceGroove| {
                r.axis = out.direction(r.axis);
                r.axis_point = out.point(r.axis_point);
                r
            }
        ),
        plates: each!("plates", plates, plate),
        turned_bodies: each!("turned_bodies", turned_bodies, turned_body),
        round_bottom_blind_slots: each!(
            "round_bottom_blind_slots",
            round_bottom_blind_slots,
            round_bottom_blind_slot
        ),
        semicircular_blind_slots: each!(
            "semicircular_blind_slots",
            semicircular_blind_slots,
            |out: &mut Out,
             mut r: crate::features::semicircular_blind_slots::SemicircularBlindSlot| {
                r.axis = out.direction(r.axis);
                r.opening = out.direction(r.opening);
                r.at = out.point(r.at);
                r
            }
        ),
        rectangular_blind_slots: each!(
            "rectangular_blind_slots",
            rectangular_blind_slots,
            rectangular_blind_slot
        ),
        double_d_bores: each!("double_d_bores", double_d_bores, double_d_bore),
        edge_open_circular_pockets: each!(
            "edge_open_circular_pockets",
            edge_open_circular_pockets,
            edge_open_circular_pocket
        ),
        edge_open_prismatic_recesses: each!(
            "edge_open_prismatic_recesses",
            edge_open_prismatic_recesses,
            edge_open_prismatic_recess
        ),
        blends: each!("blends", blends, blend),
        sheet_metal_bodies: each!("sheet_metal_bodies", sheet_metal_bodies, sheet_metal_body),
        slots: each!("slots", slots, slot),
        pockets: each!("pockets", pockets, pocket),
        channels: each!("channels", channels, channel),
        v_channels: each!(
            "v_channels",
            v_channels,
            |out: &mut Out, mut r: crate::features::v_channels::VChannel| {
                r.axis = out.direction(r.axis);
                r.opening = out.direction(r.opening);
                r.at = out.point(r.at);
                r
            }
        ),
        rectangular_through_notches: each!(
            "rectangular_through_notches",
            rectangular_through_notches,
            |out: &mut Out, mut r: crate::features::through_notches::RectangularThroughNotch| {
                r.axis = out.direction(r.axis);
                r.opening = out.direction(r.opening);
                r.at = out.point(r.at);
                r
            }
        ),
        slot_patterns: each!("slot_patterns", slot_patterns, |out, r| slot_pattern(
            out, r, &source
        )),
        pocket_patterns: each!("pocket_patterns", pocket_patterns, pocket_pattern),
        repeating_radial_profiles: each!(
            "repeating_radial_profiles",
            repeating_radial_profiles,
            repeating_radial_profile
        ),
        freeform_surfaces: each!("freeform_surfaces", freeform_surfaces, freeform_surface),
        helical_springs: each!(
            "helical_springs",
            helical_springs,
            |out: &mut Out, mut r: crate::features::helical_springs::HelicalSpring| {
                let (axis, shift, sign) = thread_axis(out, r.axis);
                r.axis = axis;
                let (start, end) = (r.start + shift, r.end + shift);
                (r.start, r.end) = if sign > 0. {
                    (start, end)
                } else {
                    (-end, -start)
                };
                r
            }
        ),
        modelled_threads: each!("modelled_threads", modelled_threads, modelled_thread),
        modelled_thread_refusals: each!(
            "modelled_thread_refusals",
            modelled_thread_refusals,
            modelled_thread_refusal
        ),
        polygonal_bosses: each!("polygonal_bosses", polygonal_bosses, polygonal_prism),
        polygonal_stock: each!("polygonal_stock", polygonal_stock, polygonal_prism),
        section_passages: each!("section_passages", section_passages, section_passage),
        prismatic_pockets: each!("prismatic_pockets", prismatic_pockets, prismatic_pocket),
        oriented_slots: each!("oriented_slots", oriented_slots, oriented_slot),
        oriented_slot_patterns: each!(
            "oriented_slot_patterns",
            oriented_slot_patterns,
            oriented_slot_pattern
        ),
        pads: each!("pads", pads, raised_pad),
        step_levels: each!("step_levels", step_levels, face_level),
        risers: each!("risers", risers, riser),
        section_recesses: each!("section_recesses", section_recesses, section_recess),
        section_recess_refusals,
        section_recess_patterns: each!(
            "section_recess_patterns",
            section_recess_patterns,
            section_recess_pattern
        ),
        section_recess_refusal,
        // The working part's faces are the caller's, under the same indices.
        defining,
    };
    crate::published_records::PublishedFeatures::new(features, relative, *motion, source, reach)
}

fn fillet(out: &mut Out, r: Fillet) -> Fillet {
    let Fillet {
        axis,
        radius,
        at,
        turned,
        side,
    } = r;
    Fillet {
        axis: match axis {
            FilletAxis::Principal(axis) => FilletAxis::Principal(out.letter(axis, "axis")),
            FilletAxis::Direction(direction) => FilletAxis::Direction(out.direction(direction)),
        },
        radius,
        at: out.placed_point(at),
        turned,
        side,
    }
}

fn turned_body(
    out: &mut Out,
    r: crate::features::turned_bodies::TurnedBody,
) -> crate::features::turned_bodies::TurnedBody {
    use crate::features::turned_bodies::{self, TurnedBody};
    let TurnedBody {
        axis,
        axis_point,
        od,
        length,
        mut bores,
        mut profile,
        body_key,
        profile_key,
    } = r;
    let moved = out.direction(turned_bodies::direction(axis));
    let canonical = turned_bodies::canonical(moved);
    if crate::kernel::geom::dot(moved, canonical) < 0. {
        for b in &mut bores {
            b.span = [-b.span[1], -b.span[0]];
        }
        for p in &mut profile {
            let start = p.start;
            p.start = [-p.end[0], p.end[1]];
            p.end = [-start[0], start[1]];
            p.arc_center = p.arc_center.map(|c| [-c[0], c[1]]);
            // Reflection reverses the arc sense, exchanging endpoints reverses it again.
        }
        turned_bodies::sort_profile(&mut profile);
        bores.sort_by(|a, b| {
            a.diameter
                .total_cmp(&b.diameter)
                .then(a.span[0].total_cmp(&b.span[0]))
        });
    }
    TurnedBody {
        axis: turned_bodies::axis(canonical),
        axis_point: out.placed_point(axis_point),
        od,
        length,
        bores,
        profile,
        body_key: out.body_key(body_key, "body_key"),
        profile_key: profile_key.map(|p| {
            // Reuse the existing mapping and its pending publication paths under this field.
            let mut nested = Out::new(out.motion, out.publish, "");
            let p = turned_profile(&mut nested, p);
            out.relative.extend(
                nested
                    .done()
                    .into_iter()
                    .map(|s| s.replacen("profile.", "profile_key.", 1)),
            );
            p
        }),
    }
}

fn cylindrical_profile(
    out: &mut Out,
    mut r: crate::features::cylindrical_profiles::CylindricalProfile,
) -> crate::features::cylindrical_profiles::CylindricalProfile {
    use crate::features::cylindrical_profiles::ProfileAxis;
    r.axis = match r.axis {
        ProfileAxis::Principal(axis) => ProfileAxis::Principal(out.letter(axis, "axis")),
        ProfileAxis::Direction(direction) => ProfileAxis::Direction(out.direction(direction)),
    };
    r.axis_point = out.placed_point(r.axis_point);
    r
}

fn chamfer(out: &mut Out, r: Chamfer) -> Chamfer {
    let Chamfer {
        axis,
        leg1,
        leg2,
        angle,
        at,
        turned,
        corner,
    } = r;
    Chamfer {
        axis: out.letter(axis, "axis"),
        leg1,
        leg2,
        angle,
        at: out.placed_point(at),
        turned,
        corner: corner.map(|c| out.placed_point(c)),
    }
}

fn boss(out: &mut Out, r: BossRecord) -> BossRecord {
    let BossRecord {
        axis,
        location,
        diameter,
        height,
    } = r;
    BossRecord {
        axis: out.direction(axis),
        location: out.point(location),
        diameter,
        height,
    }
}

fn angled_step(out: &mut Out, r: AngledStep) -> AngledStep {
    let AngledStep {
        axis,
        leg1,
        leg2,
        angle,
        length,
        at,
        corner,
    } = r;
    AngledStep {
        axis: out.letter(axis, "axis"),
        leg1,
        leg2,
        angle,
        length,
        at: out.placed_point(at),
        corner: corner.map(|c| out.placed_point(c)),
    }
}

/// `axis_direction` is the stock cylinder's direction (canonical along `axis` in the frame),
/// `stock_span` is measured along it from the origin, and `axis_line` is where the axis line
/// crosses the plane through the origin, in the other two axes.
fn flat(out: &mut Out, r: Flat) -> Flat {
    let Flat {
        axis,
        across,
        at,
        axis_line,
        stock_span,
        axis_direction,
    } = r;
    let i = index(axis);
    let [a, b] = others(i);
    let d = axis_direction;
    let target = out.target(i, "axis");
    let direction = out.direction(d);
    let shift = py::dot(&out.motion.translation, &direction);
    let stock_span = out.placed(stock_span, |_, (lo, hi)| (lo + shift, hi + shift));
    let (axis, axis_line) = match target {
        Some((w, _)) => {
            let axis_line = out.placed(axis_line, |out, (u, v)| {
                let mut foot = [0.0; 3];
                foot[a] = u;
                foot[b] = v;
                foot[i] = -(foot[a] * d[a] + foot[b] * d[b]) / d[i];
                let q = out.point(foot);
                let along = py::dot(&q, &direction);
                let foot = [0, 1, 2].map(|k| q[k] - along * direction[k]);
                let [wa, wb] = others(w);
                (foot[wa], foot[wb])
            });
            (LETTERS[w], axis_line)
        }
        None => {
            out.mark("axis_line");
            (axis, axis_line)
        }
    };
    Flat {
        axis,
        across,
        at: out.placed_point(at),
        axis_line,
        stock_span,
        axis_direction: direction,
    }
}

/// `half_widths` is low side first along the axis the two ramps oppose across, which the record
/// does not name: it is kept when both axes across the run keep their sense, swapped when both
/// reverse, and left relative otherwise.
fn paired_ramp_step(out: &mut Out, r: PairedRampStep) -> PairedRampStep {
    let PairedRampStep {
        axis,
        angle,
        length,
        at,
        opening_direction,
        half_width,
        half_widths,
        half_width_axis,
    } = r;
    let i = index(axis);
    let half_widths = half_widths.map(|(low, high)| {
        let [a, b] = others(i);
        match out.axes.map(|axes| (axes[a].1, axes[b].1)) {
            Some((sa, sb)) if sa > 0.0 && sb > 0.0 => (low, high),
            Some((sa, sb)) if sa < 0.0 && sb < 0.0 => (high, low),
            _ => {
                out.mark("half_widths");
                (low, high)
            }
        }
    });
    PairedRampStep {
        axis: out.letter(axis, "axis"),
        angle,
        length,
        at: out.placed_point(at),
        opening_direction: out.direction(opening_direction),
        half_width,
        half_widths,
        half_width_axis: out.axes.map_or(half_width_axis, |axes| {
            LETTERS[axes[index(half_width_axis)].0]
        }),
    }
}

fn oriented_chamfer(out: &mut Out, r: OrientedChamfer) -> OrientedChamfer {
    let OrientedChamfer {
        run,
        length,
        at,
        corner,
        leg1,
        leg2,
        leg1_direction,
        leg2_direction,
        support_spans,
        angle,
        body_key,
    } = r;
    OrientedChamfer {
        run: out.direction(run),
        length,
        at: out.placed_point(at),
        corner: out.placed_point(corner),
        leg1,
        leg2,
        leg1_direction: out.direction(leg1_direction),
        leg2_direction: out.direction(leg2_direction),
        // Measured along `run` from the chamfer's mid-station: they turn with it.
        support_spans,
        angle,
        body_key: out.body_key(body_key, "body_key"),
    }
}

fn circular_face_pattern(out: &mut Out, r: CircularFacePattern) -> CircularFacePattern {
    let CircularFacePattern {
        axis_origin,
        axis_direction,
        count,
        pitch_degrees,
        seed_index,
        fit_error,
    } = r;
    CircularFacePattern {
        axis_origin: out.placed_point(axis_origin),
        axis_direction: out.direction(axis_direction),
        count,
        pitch_degrees,
        seed_index,
        fit_error,
    }
}

fn oblique_through_step(out: &mut Out, r: ObliqueThroughStep) -> ObliqueThroughStep {
    let ObliqueThroughStep {
        run,
        length,
        at,
        depth_direction,
        depth,
        across_direction,
        wall_outline,
        body_key,
    } = r;
    ObliqueThroughStep {
        run: out.direction(run),
        length,
        at: out.placed_point(at),
        depth_direction: out.direction(depth_direction),
        depth,
        across_direction: out.direction(across_direction),
        // Along `run` and across the wall from the wall's start: they turn with the record.
        wall_outline,
        body_key: out.body_key(body_key, "body_key"),
    }
}

fn circular_blind_step(out: &mut Out, r: CircularBlindStep) -> CircularBlindStep {
    let CircularBlindStep {
        axis,
        radius,
        length,
        centreline,
        section,
    } = r;
    let i = index(axis);
    CircularBlindStep {
        axis: out.letter(axis, "axis"),
        radius,
        length,
        centreline: centreline.map(|p| out.placed_point(p)),
        section: section.map(|p| out.placed_across(i, p, "section")),
    }
}

fn hole(out: &mut Out, r: HoleRecord) -> HoleRecord {
    let HoleRecord {
        axis,
        location,
        diameter,
        depth,
        bottom,
        cbore,
        spotface,
        outer_steps,
        csink,
        exit_obstructions,
    } = r;
    HoleRecord {
        axis: out.direction(axis),
        location: out.placed_point(location),
        diameter,
        depth,
        bottom,
        cbore: cbore.map(counterbore),
        spotface: spotface.map(counterbore),
        outer_steps: outer_steps.into_iter().map(counterbore).collect(),
        csink: csink.map(|c| countersink(out, c)),
        exit_obstructions: exit_obstructions
            .into_iter()
            .map(|mut obstruction| {
                obstruction.location = out.placed_point(obstruction.location);
                obstruction
            })
            .collect(),
    }
}

fn countersink(out: &mut Out, r: CounterSink) -> CounterSink {
    let CounterSink {
        axis,
        location,
        major_diameter,
        drill_diameter,
        included_angle,
        depth,
    } = r;
    CounterSink {
        axis: out.direction(axis),
        location: out.placed_point(location),
        major_diameter,
        drill_diameter,
        included_angle,
        depth,
    }
}

fn holes(out: &mut Out, holes: Vec<HoleRecord>) -> Vec<HoleRecord> {
    holes.into_iter().map(|h| hole(out, h)).collect()
}

/// The members' common drilling axis, which the grid's plane basis is taken from.
fn drilling_axis(holes: &[HoleRecord]) -> V3 {
    holes.first().expect("a hole pattern has members").axis
}

fn hole_pattern(out: &mut Out, r: HolePattern) -> HolePattern {
    match r {
        HolePattern::RectGrid {
            holes: members,
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            center,
        } => {
            let angle = out.pattern_angle(drilling_axis(&members), angle, 180.0);
            let holes = holes(out, members);
            HolePattern::RectGrid {
                center: out.pattern_center(center, &holes),
                angle,
                holes,
                rows,
                cols,
                row_pitch,
                col_pitch,
            }
        }
        HolePattern::RectangularHoleSet {
            holes: members,
            center,
            width,
            height,
            angle,
        } => {
            // A square set's angle is reduced modulo 90 degrees.
            let angle = out.pattern_angle(
                drilling_axis(&members),
                angle,
                if width == height { 90.0 } else { 180.0 },
            );
            let holes = holes(out, members);
            HolePattern::RectangularHoleSet {
                center: out.pattern_center(center, &holes),
                angle,
                holes,
                width,
                height,
            }
        }
        HolePattern::BoltCircle {
            holes: members,
            center,
            diameter,
        } => {
            let holes = holes(out, members);
            HolePattern::BoltCircle {
                center: out.pattern_center(center, &holes),
                holes,
                diameter,
            }
        }
        HolePattern::LinearArray {
            holes: members,
            pitch,
            direction,
        } => HolePattern::LinearArray {
            holes: holes(out, members),
            pitch,
            direction: out.direction(direction),
        },
    }
}

/// `supports` are the two support planes across the thickness axis (letter and coordinate) in
/// axis order; `legs` and `directions` (the side of each support the rib stands on) follow them.
fn gusset_rib(out: &mut Out, r: GussetRib) -> GussetRib {
    let GussetRib {
        thickness_axis,
        thickness_bounds,
        supports: ((la, ca), (lb, cb)),
        legs,
        directions,
        body_key,
    } = r;
    let t = index(thickness_axis);
    let thickness_bounds = out.placed(thickness_bounds, |out, b| {
        out.interval(t, b, "thickness_bounds")
    });
    let (ia, ib) = (index(la), index(lb));
    let first = (
        (
            out.letter(la, "supports"),
            out.placed(ca, |out, v| out.coordinate(ia, v, "supports")),
        ),
        legs.0,
        out.sign(ia, directions.0, "directions"),
    );
    let second = (
        (
            out.letter(lb, "supports"),
            out.placed(cb, |out, v| out.coordinate(ib, v, "supports")),
        ),
        legs.1,
        out.sign(ib, directions.1, "directions"),
    );
    let (first, second) = if out.axes.is_none() {
        out.mark("legs");
        (first, second)
    } else if first.0.0 > second.0.0 {
        (second, first)
    } else {
        (first, second)
    };
    GussetRib {
        thickness_axis: out.letter(thickness_axis, "thickness_axis"),
        thickness_bounds,
        supports: (first.0, second.0),
        legs: (first.1, second.1),
        directions: (first.2, second.2),
        body_key: out.body_key(body_key, "body_key"),
    }
}

fn gusset_rib_pattern(out: &mut Out, r: GussetRibPattern) -> GussetRibPattern {
    let ribs = |out: &mut Out, ribs: Vec<GussetRib>| {
        let mut members = Out::new(out.motion, out.publish, "ribs[].");
        let ribs = ribs
            .into_iter()
            .map(|r| gusset_rib(&mut members, r))
            .collect();
        out.relative.extend(members.done());
        ribs
    };
    match r {
        GussetRibPattern::Array {
            ribs: members,
            axis,
            pitch,
        } => GussetRibPattern::Array {
            ribs: ribs(out, members),
            axis: out.letter(axis, "axis"),
            pitch,
        },
        GussetRibPattern::MirrorPair {
            ribs: members,
            mirror_plane: (letter, at),
        } => {
            let ribs: Vec<GussetRib> = ribs(out, members);
            let i = index(letter);
            // The plane is the body's centre (its key's box) on the three-decimal grid:
            // published, it is measured from the ribs' published key, as the pattern did.
            let at = match (out.publish, out.axes, &ribs[0].body_key) {
                (true, Some(axes), Some(key)) => {
                    let w = axes[i].0;
                    Grid::Decimals(3).publish((key[w] + key[w + 3]) / 2.0)
                }
                _ => out.coordinate(i, at, "mirror_plane"),
            };
            GussetRibPattern::MirrorPair {
                ribs,
                mirror_plane: (out.letter(letter, "mirror_plane"), at),
            }
        }
    }
}

fn thin_wall_body(out: &mut Out, r: ThinWallBody) -> ThinWallBody {
    let ThinWallBody {
        body_index,
        body_key,
        thickness,
        face_pairs,
        unpaired_faces,
        rim_regions,
        paired_area_fraction,
        history_hint,
        unpaired_face_classes,
    } = r;
    ThinWallBody {
        body_index,
        body_key: out.body_key(body_key, "body_key"),
        thickness,
        face_pairs: wall_pairs(face_pairs),
        unpaired_faces,
        rim_regions,
        paired_area_fraction,
        history_hint: shell_history_hint(history_hint),
        unpaired_face_classes: unpaired_face_classes
            .into_iter()
            .map(unpaired_wall_face)
            .collect(),
    }
}

fn interior_void(out: &mut Out, r: InteriorVoid) -> InteriorVoid {
    let InteriorVoid {
        body_index,
        body_key,
        void_faces,
        openings,
        estimated_volume,
        volume_method,
        grid_pitch,
        enclosed_samples,
        air_samples,
    } = r;
    InteriorVoid {
        body_index,
        body_key: out.body_key(body_key, "body_key"),
        void_faces,
        openings,
        estimated_volume,
        volume_method,
        grid_pitch,
        enclosed_samples,
        air_samples,
    }
}

/// `section` is boundary endpoint, concave corner, boundary endpoint (the lesser direction in the
/// frame first), and `endpoint_scopes` follow its two endpoints.
fn through_step(out: &mut Out, r: ThroughStep) -> ThroughStep {
    let ThroughStep {
        axis,
        length,
        at,
        section,
        body_key,
        endpoint_scopes,
    } = r;
    let i = index(axis);
    let section = section.map(|p| out.placed_across(i, p, "section"));
    if out.axes.is_none() {
        out.mark("endpoint_scopes");
    }
    ThroughStep {
        axis: out.letter(axis, "axis"),
        length,
        at: out.placed_point(at),
        section,
        body_key: out.body_key(body_key, "body_key"),
        endpoint_scopes,
    }
}

fn local_step(
    out: &mut Out,
    r: crate::features::local_steps::LocalStep,
) -> crate::features::local_steps::LocalStep {
    let crate::features::local_steps::LocalStep {
        run_direction,
        run_start,
        run_end,
        length,
        height_direction,
        height,
        depth_direction,
        depth,
        ends,
    } = r;
    crate::features::local_steps::LocalStep {
        run_direction: out.direction(run_direction),
        run_start: out.placed_point(run_start),
        run_end: out.placed_point(run_end),
        length,
        height_direction: out.direction(height_direction),
        height,
        depth_direction: out.direction(depth_direction),
        depth,
        ends,
    }
}

fn turned_step(out: &mut Out, r: TurnedStep) -> TurnedStep {
    let TurnedStep {
        axis,
        lo,
        hi,
        diameter,
        profile,
    } = r;
    let (lo, hi) = out.placed_interval(index(axis), (lo, hi), "lo");
    if out.axes.is_none() {
        out.mark("hi");
    }
    TurnedStep {
        axis: out.letter(axis, "axis"),
        lo,
        hi,
        diameter,
        profile: profile.map(|p| turned_profile(out, p)),
    }
}

/// `axis_origin` is a point of the turning axis with its own coordinate zeroed; `body_bounds`
/// the body's box, interleaved.
fn turned_profile(out: &mut Out, p: TurnedProfileKey) -> TurnedProfileKey {
    let TurnedProfileKey {
        axis,
        axis_origin,
        body_bounds,
        body_key,
    } = p;
    let axis_origin = match out.target(index(axis), "profile.axis_origin") {
        Some((w, _)) => out.placed(axis_origin, |out, p| {
            let mut q = out.point(p);
            q[w] = 0.0;
            q
        }),
        None => axis_origin,
    };
    let body_bounds = out.placed(body_bounds, |out, b| {
        out.bounds(&b, true, "profile.body_bounds")
            .try_into()
            .expect("six bounds")
    });
    TurnedProfileKey {
        axis: out.letter(axis, "profile.axis"),
        axis_origin,
        body_bounds,
        body_key: out.body_key(body_key, "profile.body_key"),
    }
}

fn groove(out: &mut Out, r: Groove) -> Groove {
    let Groove {
        axis,
        width,
        diameter,
        at,
        profile,
    } = r;
    Groove {
        axis: out.letter(axis, "axis"),
        width,
        diameter,
        at: out.placed_point(at),
        profile: profile.map(|p| turned_profile(out, p)),
    }
}

/// `lo` and `hi` are along the axis, `u` and `v` the faces' area-weighted centre in the other
/// two axes.
fn plate(out: &mut Out, r: Plate) -> Plate {
    let Plate {
        axis,
        lo,
        hi,
        u,
        v,
        body_key,
    } = r;
    let i = index(axis);
    let (lo, hi) = out.placed_interval(i, (lo, hi), "lo");
    let [u, v] = out.across(i, [u, v], "u");
    if out.axes.is_none() {
        out.mark("hi");
        out.mark("v");
    }
    Plate {
        axis: out.letter(axis, "axis"),
        lo,
        hi,
        u,
        v,
        body_key: out.body_key(body_key, "body_key"),
    }
}

fn round_bottom_blind_slot(out: &mut Out, r: RoundBottomBlindSlot) -> RoundBottomBlindSlot {
    let RoundBottomBlindSlot {
        axis,
        open_sign,
        length,
        width_axis,
        depth_axis,
        depth_sign,
        radius,
        flat_width,
        at,
    } = r;
    RoundBottomBlindSlot {
        open_sign: out.sign(index(axis), open_sign, "open_sign"),
        axis: out.letter(axis, "axis"),
        length,
        width_axis: out.letter(width_axis, "width_axis"),
        depth_sign: out.sign(index(depth_axis), depth_sign, "depth_sign"),
        depth_axis: out.letter(depth_axis, "depth_axis"),
        radius,
        flat_width,
        at: out.placed_point(at),
    }
}

fn rectangular_blind_slot(out: &mut Out, r: RectangularBlindSlot) -> RectangularBlindSlot {
    let RectangularBlindSlot {
        axis,
        open_sign,
        length,
        width_axis,
        depth_axis,
        depth_sign,
        width,
        depth,
        at,
    } = r;
    RectangularBlindSlot {
        open_sign: out.sign(index(axis), open_sign, "open_sign"),
        axis: out.letter(axis, "axis"),
        length,
        width_axis: out.letter(width_axis, "width_axis"),
        depth_sign: out.sign(index(depth_axis), depth_sign, "depth_sign"),
        depth_axis: out.letter(depth_axis, "depth_axis"),
        width,
        depth,
        at: out.placed_point(at),
    }
}

fn double_d_bore(out: &mut Out, r: DoubleDBore) -> DoubleDBore {
    let DoubleDBore {
        axis,
        location,
        major_diameter,
        across_flats,
        depth,
        through,
        flat_direction,
    } = r;
    DoubleDBore {
        axis: out.direction(axis),
        location: out.point(location),
        major_diameter,
        across_flats,
        depth,
        through,
        flat_direction: out.direction(flat_direction),
    }
}

/// The section's points are in the two axes across the run; an arc's sweep is signed by the
/// section's handedness there.
fn edge_open_circular_pocket(out: &mut Out, r: EdgeOpenCircularPocket) -> EdgeOpenCircularPocket {
    let EdgeOpenCircularPocket {
        axis,
        run_interval,
        open_sign,
        section: OpenCircularSection { segments, opening },
    } = r;
    let i = index(axis);
    let turn = out.across_turn(i);
    let segments = segments
        .into_iter()
        .map(|s| {
            let OpenCircularSectionSegment {
                kind,
                start,
                end,
                center,
                radius,
                sweep,
            } = s;
            OpenCircularSectionSegment {
                kind,
                start: out.placed_across(i, start, "section"),
                end: out.placed_across(i, end, "section"),
                center: center.map(|c| out.placed_across(i, c, "section")),
                radius,
                sweep: sweep.map(|w| turn * w),
            }
        })
        .collect();
    EdgeOpenCircularPocket {
        axis: out.letter(axis, "axis"),
        run_interval: out.placed(run_interval, |out, r| out.interval2(i, r, "run_interval")),
        open_sign: out.sign(i, open_sign, "open_sign"),
        section: OpenCircularSection {
            segments,
            opening: opening.map(|p| out.placed_across(i, p, "section")),
        },
    }
}

/// The wall chain runs in its lesser direction in the frame, and the opening is from its last
/// corner to its first.
fn edge_open_prismatic_recess(
    out: &mut Out,
    r: EdgeOpenPrismaticRecess,
) -> EdgeOpenPrismaticRecess {
    let EdgeOpenPrismaticRecess {
        axis,
        run_interval,
        open_sign,
        section:
            OpenPolygonalSection {
                wall_chain,
                opening: OpenSectionOpening { start, end },
            },
    } = r;
    let i = index(axis);
    let wall_chain = wall_chain
        .into_iter()
        .map(|p| out.placed_across(i, p, "section"))
        .collect();
    let (start, end) = (
        out.placed_across(i, start, "section"),
        out.placed_across(i, end, "section"),
    );
    EdgeOpenPrismaticRecess {
        axis: out.letter(axis, "axis"),
        run_interval: out.placed(run_interval, |out, r| out.interval2(i, r, "run_interval")),
        open_sign: out.sign(i, open_sign, "open_sign"),
        section: OpenPolygonalSection {
            wall_chain,
            opening: OpenSectionOpening { start, end },
        },
    }
}

fn blend(out: &mut Out, r: Blend) -> Blend {
    let Blend { radius, side, path } = r;
    let path = match path {
        BlendPath::Straight(StraightBlendPath { at, direction }) => {
            BlendPath::Straight(StraightBlendPath {
                at: out.placed_point(at),
                direction: out.direction(direction),
            })
        }
        BlendPath::Circular(CircularBlendPath {
            center,
            normal,
            radius,
        }) => BlendPath::Circular(CircularBlendPath {
            center: out.placed_point(center),
            normal: out.direction(normal),
            radius,
        }),
    };
    Blend { radius, side, path }
}

/// The flat pattern is laid out in the base flange's plane, in a basis taken from its normal and
/// a principal axis (`horizontal`), about the flange's origin: its points turn into the basis
/// the turned normal gives.
fn sheet_metal_body(out: &mut Out, r: SheetMetalBody) -> SheetMetalBody {
    let SheetMetalBody {
        body_index,
        body_key,
        thickness,
        first_side_faces,
        second_side_faces,
        cut_edge_faces,
        formed_features,
        flanges,
        bends,
        flat_pattern,
        paired_area_fraction,
        flat_pattern_status,
        edge_treatments,
    } = r;
    let flat_pattern = flat_pattern.map(|plan| {
        let base = flanges
            .iter()
            .find(|f| f.index == plan.base_flange)
            .expect("the flat pattern's base flange is a flange");
        flat_plan(out, plan, base.normal)
    });
    let flanges = flanges
        .into_iter()
        .map(|f| {
            let SheetFlange {
                index,
                reference_faces,
                mate_faces,
                origin,
                normal,
                area,
            } = f;
            SheetFlange {
                index,
                reference_faces,
                mate_faces,
                origin: out.point(origin),
                normal: out.direction(normal),
                area,
            }
        })
        .collect();
    let bends = bends
        .into_iter()
        .map(|b| {
            let SheetBend {
                index,
                face_pairs,
                first_flange,
                second_flange,
                axis_origin,
                axis_direction,
                angle_degrees,
                inner_radius,
                reference_skin,
                neutral_radius,
                bend_allowance,
            } = b;
            SheetBend {
                index,
                face_pairs: wall_pairs(face_pairs),
                first_flange,
                second_flange,
                axis_origin: out.point(axis_origin),
                axis_direction: out.direction(axis_direction),
                angle_degrees,
                inner_radius,
                reference_skin,
                neutral_radius,
                bend_allowance,
            }
        })
        .collect();
    SheetMetalBody {
        body_index,
        body_key: out.body_key(body_key, "body_key"),
        thickness,
        first_side_faces,
        second_side_faces,
        cut_edge_faces,
        formed_features: formed_features.into_iter().map(formed_feature).collect(),
        flanges,
        bends,
        flat_pattern,
        paired_area_fraction,
        flat_pattern_status,
        edge_treatments: edge_treatments.into_iter().map(edge_treatment).collect(),
    }
}

/// `sheet_metal`'s flat basis of a flange normal.
fn flat_basis(normal: V3) -> Option<(V3, V3)> {
    let horizontal = if normal[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let x = py::unit(cross(horizontal, normal))?;
    Some((x, cross(normal, x)))
}

fn flat_plan(out: &mut Out, plan: FlatPatternPlan, normal: V3) -> FlatPatternPlan {
    let FlatPatternPlan {
        k_factor,
        base_flange,
        tree_bends,
        flat_faces,
        bend_strips,
        tessellation_tolerance,
        valid_blank,
        overlap_witnesses,
    } = plan;
    let (from, to) = (flat_basis(normal), flat_basis(out.direction(normal)));
    let (Some((xs, ys)), Some((xt, yt))) = (from, to) else {
        panic!("a laid-out flange normal has a flat basis");
    };
    let turn = |p: [f64; 2]| {
        let d = out.direction([0, 1, 2].map(|i| p[0] * xs[i] + p[1] * ys[i]));
        [py::dot(&d, &xt), py::dot(&d, &yt)]
    };
    FlatPatternPlan {
        k_factor,
        base_flange,
        tree_bends,
        flat_faces: flat_faces
            .into_iter()
            .map(|f| {
                let UnfoldedFlangeFace {
                    source_face,
                    flange,
                    vertices,
                    triangles,
                } = f;
                UnfoldedFlangeFace {
                    source_face,
                    flange,
                    vertices: vertices.into_iter().map(turn).collect(),
                    triangles,
                }
            })
            .collect(),
        bend_strips: bend_strips
            .into_iter()
            .map(|s| {
                let UnfoldedBendStrip {
                    bend,
                    source_face_pair,
                    corners,
                } = s;
                UnfoldedBendStrip {
                    bend,
                    source_face_pair: wall_pair(source_face_pair),
                    corners: corners.map(turn),
                }
            })
            .collect(),
        tessellation_tolerance,
        valid_blank,
        overlap_witnesses: overlap_witnesses.into_iter().map(overlap_witness).collect(),
    }
}

/// A recess's three axes: `w_center` along the width axis, `lo` and `hi` along the long axis,
/// `d_lo` and `d_hi` (and `open_sign`) along the third, the depth axis.
struct RecessAxes {
    width: usize,
    long: usize,
    depth: usize,
}

impl RecessAxes {
    fn of(width_axis: char, long_axis: char) -> RecessAxes {
        let (width, long) = (index(width_axis), index(long_axis));
        RecessAxes {
            width,
            long,
            depth: 3 - width - long,
        }
    }
}

/// A recess's placement fields: `(w_center, (lo, hi), (d_lo, d_hi))`.
type RecessPlacement = (
    Placed<f64>,
    (Placed<f64>, Placed<f64>),
    (Placed<f64>, Placed<f64>),
);

/// The recess's placement fields, carried.
fn recess_placement(
    out: &mut Out,
    axes: &RecessAxes,
    w_center: Placed<f64>,
    run: (Placed<f64>, Placed<f64>),
    depth: (Placed<f64>, Placed<f64>),
) -> RecessPlacement {
    let w_center = out.placed(w_center, |out, v| out.coordinate(axes.width, v, "w_center"));
    let run = out.placed_interval(axes.long, run, "lo");
    let depth = out.placed_interval(axes.depth, depth, "d_lo");
    if out.axes.is_none() {
        out.mark("hi");
        out.mark("d_hi");
    }
    (w_center, run, depth)
}

fn slot(out: &mut Out, r: Slot) -> Slot {
    let Slot {
        width_axis,
        long_axis,
        width,
        length,
        w_center,
        lo,
        hi,
        d_lo,
        d_hi,
        body_key,
        end_radius,
        corner_radius,
    } = r;
    let axes = RecessAxes::of(width_axis, long_axis);
    // The legacy two-decimal grid describes absolute file-axis positions. In a
    // converted record these are distances in a measuring basis, not file-grid
    // coordinates. Keep the measured distances; snapping the basis is bounded
    // separately by the publication coordinate floor.
    let distance = |p: Placed<f64>| {
        if out.axes.is_none() && out.publish {
            Placed::exact(*p.raw())
        } else {
            p
        }
    };
    let (w_center, lo, hi, d_lo, d_hi) = (
        distance(w_center),
        distance(lo),
        distance(hi),
        distance(d_lo),
        distance(d_hi),
    );
    let (w_center, (lo, hi), (d_lo, d_hi)) =
        recess_placement(out, &axes, w_center, (lo, hi), (d_lo, d_hi));
    Slot {
        width_axis: out.letter(width_axis, "width_axis"),
        long_axis: out.letter(long_axis, "long_axis"),
        width,
        length,
        w_center,
        lo,
        hi,
        d_lo,
        d_hi,
        body_key: out.body_key(body_key, "body_key"),
        end_radius,
        corner_radius,
    }
}

fn pocket(out: &mut Out, r: Pocket) -> Pocket {
    let Pocket {
        width_axis,
        long_axis,
        width,
        length,
        depth,
        w_center,
        lo,
        hi,
        d_lo,
        d_hi,
        open_sign,
        edge_anchored,
        rectangular_boundary,
        body_key,
        end_radius,
        corner_radius,
    } = r;
    let axes = RecessAxes::of(width_axis, long_axis);
    let (w_center, (lo, hi), (d_lo, d_hi)) =
        recess_placement(out, &axes, w_center, (lo, hi), (d_lo, d_hi));
    Pocket {
        width_axis: out.letter(width_axis, "width_axis"),
        long_axis: out.letter(long_axis, "long_axis"),
        width,
        length,
        depth,
        w_center,
        lo,
        hi,
        d_lo,
        d_hi,
        open_sign: out.sign(axes.depth, open_sign, "open_sign"),
        edge_anchored,
        rectangular_boundary,
        body_key: out.body_key(body_key, "body_key"),
        end_radius,
        corner_radius,
    }
}

fn channel(out: &mut Out, r: Channel) -> Channel {
    let Channel {
        width_axis,
        long_axis,
        width,
        w_center,
        lo,
        hi,
        d_lo,
        d_hi,
        open_sign,
        body_key,
    } = r;
    let axes = RecessAxes::of(width_axis, long_axis);
    let (w_center, (lo, hi), (d_lo, d_hi)) =
        recess_placement(out, &axes, w_center, (lo, hi), (d_lo, d_hi));
    Channel {
        width_axis: out.letter(width_axis, "width_axis"),
        long_axis: out.letter(long_axis, "long_axis"),
        width,
        w_center,
        lo,
        hi,
        d_lo,
        d_hi,
        open_sign: out.sign(axes.depth, open_sign, "open_sign"),
        body_key: out.body_key(body_key, "body_key"),
    }
}

/// A recess grid's plane is across its members' depth axis (the positive unit vector along it).
fn depth_normal(width_axis: char, long_axis: char) -> V3 {
    let mut normal = [0.0; 3];
    normal[RecessAxes::of(width_axis, long_axis).depth] = 1.0;
    normal
}

/// Remeasure a proved grid without searching for another pattern. Its existing
/// angle and pitches identify each member's lattice cell; fit raw centres to
/// those same cells, so position rounding cannot change the pitches or basis.
fn measured_slot_grid(slots: &[Slot], row_pitch: f64, col_pitch: f64, angle: f64) -> (V3, V3, V3) {
    let first = slots.first().expect("slot grid members");
    let (u, v) = plane_uv(depth_normal(first.width_axis, first.long_axis));
    let (s, c) = angle.to_radians().sin_cos();
    let col = [0, 1, 2].map(|i| c * u[i] + s * v[i]);
    let row = [0, 1, 2].map(|i| -s * u[i] + c * v[i]);
    let base = first.region_center();
    let cells: Vec<[f64; 2]> = slots
        .iter()
        .map(|slot| {
            let delta = [0, 1, 2].map(|i| slot.region_center()[i] - base[i]);
            [
                py::dot(&delta, &col) / col_pitch,
                py::dot(&delta, &row) / row_pitch,
            ]
            .map(f64::round_ties_even)
        })
        .collect();
    let mean: V3 = std::array::from_fn(|i| {
        slots.iter().map(|s| s.raw_center()[i]).sum::<f64>() / slots.len() as f64
    });
    let step = |axis: usize| {
        let mid = cells.iter().map(|c| c[axis]).sum::<f64>() / cells.len() as f64;
        let variance = cells.iter().map(|c| (c[axis] - mid).powi(2)).sum::<f64>();
        std::array::from_fn(|i| {
            slots
                .iter()
                .zip(&cells)
                .map(|(s, c)| (s.raw_center()[i] - mean[i]) * (c[axis] - mid))
                .sum::<f64>()
                / variance
        })
    };
    (step(0), step(1), mean)
}

fn slot_pattern(out: &mut Out, r: SlotPattern, source: &Features) -> SlotPattern {
    let members = |out: &mut Out, slots: Vec<Slot>| {
        let mut each = Out::new(out.motion, out.publish, "slots[].");
        let slots: Vec<Slot> = slots.into_iter().map(|s| slot(&mut each, s)).collect();
        out.relative.extend(each.done());
        slots
    };
    match r {
        SlotPattern::SlotGrid {
            slots,
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            center: _,
        } => {
            let first = slots.first().expect("a slot pattern has members");
            let normal = depth_normal(first.width_axis, first.long_axis);
            let (col, row, center) = measured_slot_grid(&slots, row_pitch, col_pitch, angle);
            let direction = out.direction(col);
            let (u, v) = plane_uv(out.direction(normal));
            let angle = py::modulo(
                py::dot(&direction, &v)
                    .atan2(py::dot(&direction, &u))
                    .to_degrees(),
                180.,
            );
            let (row_pitch, col_pitch, center) =
                (py::hypot(&row), py::hypot(&col), out.point(center));
            let slots = members(out, slots);
            SlotPattern::SlotGrid {
                center,
                angle,
                slots,
                rows,
                cols,
                row_pitch,
                col_pitch,
            }
        }
        SlotPattern::SlotArray { mut slots, .. } => {
            // Keep the recogniser's membership. Only remeasure the
            // accepted array, before any position publication can skew its line.
            // Face identity fixes the sense when recognition reverses its frame.
            // This changes neither the membership nor any recognition predicate.
            let faces = |slot: &Slot| {
                let n = source.slots.iter().position(|s| s == slot)?;
                source.defining.get("slots")?.get(n)
            };
            if let (Some(first), Some(last)) = (faces(&slots[0]), faces(&slots[slots.len() - 1]))
                && first < last
            {
                slots.reverse();
            }
            let first = slots.first().expect("slot array members").raw_center();
            let last = slots.last().expect("slot array members").raw_center();
            let delta = [0, 1, 2].map(|i| last[i] - first[i]);
            let span = py::hypot(&delta);
            let pitch = span / (slots.len() - 1) as f64;
            let direction = delta.map(|v| v / span);
            SlotPattern::SlotArray {
                slots: members(out, slots),
                pitch,
                direction: out.direction(direction),
            }
        }
    }
}

fn pocket_pattern(out: &mut Out, r: PocketPattern) -> PocketPattern {
    let members = |out: &mut Out, pockets: Vec<Pocket>| {
        let mut each = Out::new(out.motion, out.publish, "pockets[].");
        let pockets: Vec<Pocket> = pockets.into_iter().map(|p| pocket(&mut each, p)).collect();
        out.relative.extend(each.done());
        pockets
    };
    match r {
        PocketPattern::PocketGrid {
            pockets,
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            center,
        } => {
            let first = pockets.first().expect("a pocket pattern has members");
            let normal = depth_normal(first.width_axis, first.long_axis);
            let angle = out.pattern_angle(normal, angle, 180.0);
            let pockets = members(out, pockets);
            PocketPattern::PocketGrid {
                center: out.pattern_center(center, &pockets),
                angle,
                pockets,
                rows,
                cols,
                row_pitch,
                col_pitch,
            }
        }
        PocketPattern::PocketArray {
            pockets,
            pitch,
            direction,
        } => PocketPattern::PocketArray {
            pockets: members(out, pockets),
            pitch,
            direction: out.direction(direction),
        },
    }
}

fn repeating_radial_profile(out: &mut Out, r: RepeatingRadialProfile) -> RepeatingRadialProfile {
    let RepeatingRadialProfile {
        axis,
        centre,
        span,
        repeat_count,
        edge_count,
        sector_signature,
    } = r;
    let i = index_str(&axis);
    RepeatingRadialProfile {
        span: out.interval2(i, span, "span"),
        axis: out.letter_string(axis, "axis"),
        centre: out.point(centre),
        repeat_count,
        edge_count,
        // Lengths and polar shapes about the centre, whatever the traversal and phase.
        sector_signature,
    }
}

fn freeform_surface(out: &mut Out, r: FreeformSurface) -> FreeformSurface {
    let FreeformSurface {
        face,
        support_kind,
        support:
            BSplineSurfaceSupport {
                u_degree,
                v_degree,
                poles,
                weights,
                u_knots,
                v_knots,
                u_multiplicities,
                v_multiplicities,
                u_periodic,
                v_periodic,
            },
        construction_kind,
        construction_axis,
        construction_vector,
        continuity_group,
        continuity_links,
        offset_partner,
        offset_distance,
        offset_basis,
    } = r;
    FreeformSurface {
        face,
        support_kind,
        support: BSplineSurfaceSupport {
            u_degree,
            v_degree,
            poles: poles.into_iter().map(|row| out.points(row)).collect(),
            weights,
            u_knots,
            v_knots,
            u_multiplicities,
            v_multiplicities,
            u_periodic,
            v_periodic,
        },
        construction_kind,
        // A parameter direction (`u` or `v`), not a direction in space.
        construction_axis,
        construction_vector: construction_vector.map(|d| out.direction(d)),
        continuity_group,
        continuity_links: continuity_links.into_iter().map(continuity_link).collect(),
        offset_partner,
        offset_distance,
        offset_basis,
    }
}

/// `base` and `top` are along the prism's axis.
fn polygonal_prism(out: &mut Out, r: PolygonalPrism) -> PolygonalPrism {
    let PolygonalPrism {
        axis,
        center,
        side_count,
        across_flats,
        base,
        top,
        flat_directions,
        flat_centres,
    } = r;
    let i = index_str(&axis);
    let (base, top) = out.placed_interval(i, (base, top), "base");
    if out.axes.is_none() {
        out.mark("top");
    }
    PolygonalPrism {
        axis: out.letter_string(axis, "axis"),
        center: out.placed_point(center),
        side_count,
        across_flats,
        base,
        top,
        flat_directions: flat_directions
            .into_iter()
            .map(|d| out.direction(d))
            .collect(),
        flat_centres: flat_centres
            .into_iter()
            .map(|c| out.placed_point(c))
            .collect(),
    }
}

/// The passage frame's origin is where its centre line crosses the plane through the origin
/// across `run`, and `run_interval` is measured along `run` from the origin; the section and the
/// ends' gradients are in the frame's `u` and `v`, which turn with it.
fn section_passage(out: &mut Out, r: SectionPassage) -> SectionPassage {
    let SectionPassage {
        frame: PassageFrame { origin, run, u, v },
        run_interval,
        section,
        ends,
    } = r;
    let run = out.direction(run);
    let origin = out.placed(origin, |out, o| {
        let q = out.point(o);
        let along = py::dot(&q, &run);
        [0, 1, 2].map(|i| q[i] - along * run[i])
    });
    let shift = py::dot(&out.motion.translation, &run);
    SectionPassage {
        frame: PassageFrame {
            origin,
            run,
            u: out.direction(u),
            v: out.direction(v),
        },
        run_interval: out.placed(run_interval, |_, (lo, hi)| (lo + shift, hi + shift)),
        section: passage_section(section),
        ends: passage_ends(ends),
    }
}

/// A section recess's frame as a section passage's ([`section_passage`]), its section, its ends'
/// gradients and a cylindrical end's axis across the section unchanged (they are in the frame's
/// `u` and `v`, which turn with it), and its run coordinates (the interval, an envelope's terms'
/// heights, a cylinder's axis height), measured along `run` from the origin, moved with it.
/// Published, the origin is the unrounded one's image on the plane through the file's origin
/// across `run`, and each run coordinate is the unrounded one moved by that projection's shift,
/// each published once on its grid. Carried as found (for fingerprints), the shift is taken on
/// the three-decimal grid and the frame's origin set off along `run` by the rest (at most
/// 0.0005 mm), so each coordinate moves exactly on its grid. A record whose checks still refuse
/// independently reanchored values (a rounding tie) instead keeps its validated relative geometry
/// and carries its complete measuring frame without a second rounding.
fn section_recess(out: &mut Out, r: SectionRecess) -> SectionRecess {
    match moved_section_recess(out, &r) {
        Some(moved) => moved,
        None => {
            let frame = r.geometry().frame();
            let mapped = PassageFrame {
                origin: frame.origin.carried(false, |p| out.point(p)),
                run: out.direction(frame.run),
                u: out.direction(frame.u),
                v: out.direction(frame.v),
            };
            r.with_frame(mapped)
        }
    }
}

/// [`section_recess`]'s moved record, or `None` where its checks refuse it.
fn moved_section_recess(out: &Out, r: &SectionRecess) -> Option<SectionRecess> {
    let geometry = r.geometry();
    let PassageFrame { origin, run, u, v } = geometry.frame().clone();
    let run = out.direction(run);
    let (origin, shift) = if out.publish {
        let q = out.point(*origin.raw());
        let along = py::dot(&q, &run);
        let projected = origin.carried(true, |_| [0, 1, 2].map(|i| q[i] - along * run[i]));
        (projected, along)
    } else {
        let shift = py::round_to(py::dot(&out.motion.translation, &run), 3);
        let moved = origin.carried(false, |o| {
            let q = out.point(o);
            [0, 1, 2].map(|i| q[i] - shift * run[i])
        });
        (moved, shift)
    };
    let frame = PassageFrame {
        origin,
        run,
        u: out.direction(u),
        v: out.direction(v),
    };
    // A run coordinate on its grid of *digits* decimals, moved by the shift.
    let publish = out.publish;
    let moved = |c: Placed<f64>, digits: usize| {
        c.carried(publish, |v| {
            if publish {
                v + shift
            } else {
                py::round_to(v + shift, digits)
            }
        })
    };
    let run_interval = geometry.placed_run_interval().ends().map(|c| moved(c, 3));
    let run_interval = Placed::from_parts(
        (*run_interval[0], *run_interval[1]),
        (*run_interval[0].raw(), *run_interval[1].raw()),
        run_interval[0].grid(),
    );
    let end = |end: &SectionEnd| -> Option<SectionEnd> {
        let surface = match end.surface() {
            EndSurface::Planar(p) => EndSurface::Planar(p.clone()),
            EndSurface::Cylindrical(c) => {
                let axis_point = c.placed_axis_point();
                let height = moved(axis_point.map(|[_, _, h]| h), 6);
                let axis_point = Placed::from_parts(
                    [axis_point[0], axis_point[1], *height],
                    [axis_point.raw()[0], axis_point.raw()[1], *height.raw()],
                    axis_point.grid(),
                );
                EndSurface::Cylindrical(
                    CylindricalEndSurface::new(
                        "cylinder",
                        axis_point,
                        c.axis_direction(),
                        c.radius(),
                        c.branch(),
                    )
                    .ok()?,
                )
            }
            EndSurface::PlanarEnvelope(e) => {
                let term = |t: &PlanarEndTerm| {
                    PlanarEndTerm::new(moved(t.placed_height(), 6), t.gradient()).ok()
                };
                let [t0, t1] = e.terms();
                EndSurface::PlanarEnvelope(
                    PlanarEnvelopeEndSurface::new(
                        "plane_envelope",
                        e.operator(),
                        [term(t0)?, term(t1)?],
                    )
                    .ok()?,
                )
            }
        };
        SectionEnd::new(end.condition(), surface).ok()
    };
    let ends = geometry.ends();
    let ends = SectionRecessEnds::new(end(ends.low())?, end(ends.high())?).ok()?;
    let geometry = SectionRecessGeometry::new(
        "section_recess",
        frame,
        run_interval,
        geometry.profile().clone(),
        ends,
    )
    .ok()?;
    SectionRecess::new(
        r.index(),
        r.body(),
        geometry,
        r.classification().clone(),
        r.evidence().clone(),
    )
    .ok()
}

/// `section` is the void's corners in the two axes across `axis`, walked canonically in the
/// frame (counter-clockwise from the least corner); that walk is kept, as an edge-open recess's
/// wall chain is.
fn prismatic_pocket(out: &mut Out, r: PrismaticPocket) -> PrismaticPocket {
    let PrismaticPocket {
        axis,
        sides,
        depth,
        open_sign,
        at,
        section,
    } = r;
    let i = index_str(&axis);
    PrismaticPocket {
        open_sign: out.sign(i, open_sign, "open_sign"),
        section: section
            .into_iter()
            .map(|p| out.placed_across(i, p, "section"))
            .collect(),
        axis: out.letter_string(axis, "axis"),
        sides,
        depth,
        at: out.placed_point(at),
    }
}

/// The slot's directions keep the frame's canonical sense, as other directions do.
fn oriented_slot(out: &mut Out, r: OrientedSlot) -> OrientedSlot {
    let OrientedSlot {
        source,
        width_direction,
        long_direction,
        width,
        length,
        center,
        body_key,
    } = r;
    let source = section_passage(out, source);
    // The centre is the source's mid-run point: published, measured on the published source as
    // the recogniser measured it.
    let center = if out.publish {
        let f = &source.frame;
        let run_midpoint = 0.5 * py::sum([source.run_interval.0, source.run_interval.1]);
        [0, 1, 2].map(|a| Grid::Decimals(3).publish(f.origin[a] + run_midpoint * f.run[a]))
    } else {
        out.point(center)
    };
    OrientedSlot {
        source,
        width_direction: out.direction(width_direction),
        long_direction: out.direction(long_direction),
        width,
        length,
        center,
        body_key: out.body_key(body_key, "body_key"),
    }
}

fn oriented_slot_pattern(out: &mut Out, r: OrientedSlotPattern) -> OrientedSlotPattern {
    let members = |out: &mut Out, slots: Vec<OrientedSlot>| {
        let mut each = Out::new(out.motion, out.publish, "slots[].");
        let slots: Vec<OrientedSlot> = slots
            .into_iter()
            .map(|s| oriented_slot(&mut each, s))
            .collect();
        out.relative.extend(each.done());
        slots
    };
    match r {
        OrientedSlotPattern::OrientedSlotGrid {
            slots,
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            center,
        } => {
            // The grid's plane basis is taken from the members' depth direction.
            let normal = slots
                .first()
                .expect("an oriented slot pattern has members")
                .depth_direction();
            let angle = out.pattern_angle(normal, angle, 180.0);
            let slots = members(out, slots);
            OrientedSlotPattern::OrientedSlotGrid {
                center: out.pattern_center(center, &slots),
                angle,
                slots,
                rows,
                cols,
                row_pitch,
                col_pitch,
            }
        }
        OrientedSlotPattern::OrientedSlotArray {
            slots,
            pitch,
            direction,
        } => OrientedSlotPattern::OrientedSlotArray {
            slots: members(out, slots),
            pitch,
            direction: out.direction(direction),
        },
    }
}

/// The pad's box is in the frame's axes, and `direction` the sign it rises in along `axis`.
fn raised_pad(out: &mut Out, r: RaisedPad) -> RaisedPad {
    let RaisedPad {
        x0,
        x1,
        y0,
        y1,
        z0,
        z1,
        axis,
        direction,
    } = r;
    let i = index_str(&axis);
    // The box, as [`Out::bounds`] carries it: each axis's interval to its target axis.
    let (mut low, mut high) = ([x0, y0, z0], [x1, y1, z1]);
    match out.axes {
        Some(axes) => {
            let (from_low, from_high) = (low, high);
            for (k, &(w, _)) in axes.iter().enumerate() {
                (low[w], high[w]) = out.placed_interval(k, (from_low[k], from_high[k]), "x0");
            }
        }
        None => {
            for path in ["x0", "x1", "y0", "y1", "z0", "z1"] {
                out.mark(path);
            }
        }
    }
    let ([x0, y0, z0], [x1, y1, z1]) = (low, high);
    RaisedPad {
        x0,
        x1,
        y0,
        y1,
        z0,
        z1,
        direction: out.sign(i, direction, "direction"),
        axis: out.letter_string(axis, "axis"),
    }
}

/// Whether the frame's z is the file's, in either sense: face levels and risers are read along
/// the frame's z by specification (`quiddity.levels`), so carried only then.
fn z_kept(out: &Out) -> Option<[(usize, f64); 3]> {
    out.axes.filter(|axes| axes[2].0 == 2)
}

/// A horizontal level: carried when [`z_kept`] (the spans exchanged where the frame's x is the
/// file's y), and left relative otherwise.
fn face_level(out: &mut Out, r: FaceLevel) -> FaceLevel {
    let FaceLevel {
        z,
        x_span,
        y_span,
        body_key,
    } = r;
    let body_key = out.body_key(body_key, "body_key");
    let Some(axes) = z_kept(out) else {
        out.mark("z");
        for (path, span) in [("x_span", x_span), ("y_span", y_span)] {
            if span.is_some() {
                out.mark(path);
            }
        }
        return FaceLevel {
            z,
            x_span,
            y_span,
            body_key,
        };
    };
    let z = out.coordinate(2, z, "z");
    let x = x_span.map(|s| out.interval(0, s, "x_span"));
    let y = y_span.map(|s| out.interval(1, s, "y_span"));
    let (x_span, y_span) = if axes[0].0 == 0 { (x, y) } else { (y, x) };
    FaceLevel {
        z,
        x_span,
        y_span,
        body_key,
    }
}

/// A riser: carried when [`z_kept`], its positions and body levels kept ascending and, along a
/// reversed z, its ends (and which of them is at the envelope) exchanged; left relative otherwise.
fn riser(out: &mut Out, r: RiserEvidence) -> RiserEvidence {
    let RiserEvidence {
        vertical,
        axis,
        positions,
        other_axis,
        other_positions,
        z_lo,
        z_hi,
        lo_at_envelope,
        hi_at_envelope,
        tol,
        body_levels,
        body_key,
    } = r;
    let body_key = out.body_key(body_key, "body_key");
    let mut levels = Out::new(out.motion, out.publish, "body_levels[].");
    let mut body_levels: Vec<FaceLevel> = body_levels
        .into_iter()
        .map(|l| face_level(&mut levels, l))
        .collect();
    out.relative.extend(levels.done());
    let Some(axes) = z_kept(out) else {
        for path in [
            "axis",
            "positions",
            "other_axis",
            "other_positions",
            "z_lo",
            "z_hi",
            "lo_at_envelope",
            "hi_at_envelope",
        ] {
            out.mark(path);
        }
        return RiserEvidence {
            vertical,
            axis,
            positions,
            other_axis,
            other_positions,
            z_lo,
            z_hi,
            lo_at_envelope,
            hi_at_envelope,
            tol,
            body_levels,
            body_key,
        };
    };
    let mut along = |letter: char, values: Vec<Placed<f64>>, path: &str| {
        let i = index(letter);
        let mut moved: Vec<Placed<f64>> = values
            .into_iter()
            .map(|v| out.placed(v, |out, v| out.coordinate(i, v, path)))
            .collect();
        if axes[i].1 < 0.0 {
            moved.reverse();
        }
        moved
    };
    let positions = along(axis, positions, "positions");
    let other_positions = along(other_axis, other_positions, "other_positions");
    let (z_lo, z_hi) = out.interval(2, (z_lo, z_hi), "z_lo");
    let (lo_at_envelope, hi_at_envelope) = if axes[2].1 > 0.0 {
        (lo_at_envelope, hi_at_envelope)
    } else {
        // The body's levels, distinct heights in ascending order, stay ascending.
        body_levels.reverse();
        (hi_at_envelope, lo_at_envelope)
    };
    RiserEvidence {
        vertical,
        axis: out.letter(axis, "axis"),
        positions,
        other_axis: out.letter(other_axis, "other_axis"),
        other_positions,
        z_lo,
        z_hi,
        lo_at_envelope,
        hi_at_envelope,
        tol,
        body_levels,
        body_key,
    }
}

// Nested records with nothing placed in them, destructured so that a placed field added later
// fails to compile until mapped.

fn counterbore(c: CounterBore) -> CounterBore {
    let CounterBore { diameter, depth } = c;
    CounterBore { diameter, depth }
}

fn wall_pair(p: WallFacePair) -> WallFacePair {
    let WallFacePair {
        first_face,
        second_face,
        thickness,
    } = p;
    WallFacePair {
        first_face,
        second_face,
        thickness,
    }
}

fn wall_pairs(pairs: Vec<WallFacePair>) -> Vec<WallFacePair> {
    pairs.into_iter().map(wall_pair).collect()
}

fn unpaired_wall_face(f: UnpairedWallFace) -> UnpairedWallFace {
    let UnpairedWallFace { face, kind } = f;
    UnpairedWallFace { face, kind }
}

fn shell_history_hint(h: ShellHistoryHint) -> ShellHistoryHint {
    let ShellHistoryHint {
        basis,
        direction,
        outer_faces,
        inner_faces,
        opening_rims,
        before_shell_collar_pairs,
        after_shell_cut_faces,
    } = h;
    ShellHistoryHint {
        basis,
        // `inward`: toward the shell's material, not a direction in space.
        direction,
        outer_faces,
        inner_faces,
        opening_rims,
        before_shell_collar_pairs: wall_pairs(before_shell_collar_pairs),
        after_shell_cut_faces,
    }
}

fn formed_feature(f: FormedSheetFeature) -> FormedSheetFeature {
    let FormedSheetFeature {
        faces,
        paired_faces,
    } = f;
    FormedSheetFeature {
        faces,
        paired_faces,
    }
}

fn edge_treatment(t: SheetEdgeTreatment) -> SheetEdgeTreatment {
    let SheetEdgeTreatment { face, kind, radius } = t;
    SheetEdgeTreatment { face, kind, radius }
}

fn overlap_witness(w: FlatOverlapWitness) -> FlatOverlapWitness {
    let FlatOverlapWitness {
        first_kind,
        first_index,
        second_kind,
        second_index,
        area,
    } = w;
    FlatOverlapWitness {
        first_kind,
        first_index,
        second_kind,
        second_index,
        area,
    }
}

fn continuity_link(l: SurfaceContinuityLink) -> SurfaceContinuityLink {
    let SurfaceContinuityLink { other_face, kind } = l;
    SurfaceContinuityLink { other_face, kind }
}

/// In the passage frame's `u` and `v`, about its origin.
fn passage_section(s: PassageSection) -> PassageSection {
    let PassageSection { boundary } = s;
    PassageSection {
        boundary: boundary.into_iter().map(passage_vertex).collect(),
    }
}

fn passage_vertex(v: PassageSectionVertex) -> PassageSectionVertex {
    let PassageSectionVertex { point, bulge } = v;
    PassageSectionVertex { point, bulge }
}

/// Gradients across the section in the passage frame's `u` and `v`.
fn passage_ends(e: PassageEnds) -> PassageEnds {
    let PassageEnds {
        low_capped,
        high_capped,
        low_gradient,
        high_gradient,
    } = e;
    PassageEnds {
        low_capped,
        high_capped,
        low_gradient,
        high_gradient,
    }
}

/// Patterns retain run-relative occurrence references; only their spatial fields move.
fn section_recess_pattern(out: &mut Out, r: SectionRecessPattern) -> SectionRecessPattern {
    let moved = match &r {
        SectionRecessPattern::Array(a) => SectionRecessArray::new(
            a.members().to_vec(),
            a.pitch(),
            out.direction(a.direction()),
        )
        .map(SectionRecessPattern::Array),
        SectionRecessPattern::Grid(g) => g
            .mapped(|d| out.direction(d), |p| out.point(p))
            .map(SectionRecessPattern::Grid),
    };
    moved.unwrap_or_else(|_| {
        out.mark("direction");
        out.mark("row_direction");
        out.mark("col_direction");
        out.mark("center");
        r
    })
}

/// Re-anchor a line at the file origin, then orient its dominant component positively.
/// Reversing the direction swaps and negates the axial endpoints, preserving physical
/// handedness and agreement with caller-space recognition.
fn thread_axis(
    out: &Out,
    axis: crate::features::modelled_threads::ThreadAxis,
) -> (crate::features::modelled_threads::ThreadAxis, f64, f64) {
    use crate::kernel::geom;
    let p = out.point(axis.point);
    let d = out.direction(axis.direction);
    let shift = geom::dot(p, d);
    let sign = if d[geom::dominant_axis_preferring_z(d)] < 0.0 {
        -1.0
    } else {
        1.0
    };
    (
        crate::features::modelled_threads::ThreadAxis {
            point: geom::sub(p, geom::scale(d, shift)),
            direction: geom::scale(d, sign),
        },
        shift,
        sign,
    )
}

fn modelled_thread(
    out: &mut Out,
    mut r: crate::features::modelled_threads::ModelledThread,
) -> crate::features::modelled_threads::ModelledThread {
    let (axis, shift, sign) = thread_axis(out, r.axis);
    r.axis = axis;
    let (start, end) = (r.start + shift, r.end + shift);
    (r.start, r.end) = if sign > 0.0 {
        (start, end)
    } else {
        (-end, -start)
    };
    r
}

fn modelled_thread_refusal(
    out: &mut Out,
    mut r: crate::features::modelled_threads::ModelledThreadRefusal,
) -> crate::features::modelled_threads::ModelledThreadRefusal {
    r.axis = thread_axis(out, r.axis).0;
    r
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use crate::features::fillets::Fillet;
    use crate::features::placed::Grid;

    fn fillet(at: V3) -> Features {
        Features {
            fillets: vec![Fillet {
                axis: FilletAxis::Principal('x'),
                radius: 2.0,
                at: Placed::new(at, Grid::Decimals(3)),
                turned: false,
                side: "convex",
            }],
            ..Features::default()
        }
    }

    fn quarter_turn(translation: V3) -> Rigid {
        Rigid {
            rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            translation,
        }
    }

    /// A rotation within the coordinate floor of a signed permutation over the part's reach is
    /// published as that permutation (#70); beyond it, or for a wider part, it is kept.
    #[test]
    fn a_near_permutation_is_snapped_only_within_the_coordinate_floor() {
        // sm-hanger's frame: 2.42e-9 rad from x = file z, y = −file y, z = file x.
        let e = 2.423932049116e-9;
        let near = Rigid {
            rotation: [[0.0, 0.0, 1.0], [e, -1.0, 0.0], [1.0, e, 0.0]],
            translation: [1.0, 2.0, 3.0],
        };
        let snapped = near.aligned_within(200.0);
        assert_eq!(
            snapped.rotation,
            [[0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]]
        );
        assert_eq!(snapped.translation, near.translation);
        assert_eq!(snapped.axes(), Some([(2, 1.0), (1, -1.0), (0, 1.0)]));
        // The same tilt across a part reaching 1 km moves points by more than the floor.
        assert_eq!(near.aligned_within(1e6), near);
        // A tilt of 1e-5 rad moves a 1 mm part's points by 1e-5 mm: kept.
        let (s, c) = 1e-5f64.sin_cos();
        let tilted = Rigid {
            rotation: [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0; 3],
        };
        assert_eq!(tilted.aligned_within(1.0), tilted);
        assert_eq!(tilted.aligned_within(1.0).axes(), None);
        // An exact permutation is itself at any reach.
        let exact = quarter_turn([0.5; 3]);
        assert_eq!(exact.aligned_within(f64::INFINITY), exact);
    }

    /// Rounded in the frame and moved by an origin off the grid, the hole of #69 came back as
    /// 39.999728; carried unrounded and published once, it is 40.
    #[test]
    fn positions_are_published_once_on_the_files_grid() {
        let origin = [-2.347589604795208, -1.3536804064754746, -32.2985296392566];
        let raw = [
            41.353_680_406_475_475 + 1e-10,
            2.347589604795208 + 42.0,
            72.2985296392566,
        ];
        let mapped = map_features(fillet(raw), &quarter_turn(origin));
        let published = *mapped.features.fillets[0].at;
        assert_eq!(published, [-46.695, 40.0, 40.0]);
        // Fingerprints read the frame's published value carried as it is.
        let found = *mapped.fingerprinted.fillets[0].at;
        assert_eq!(found, quarter_turn(origin).point([41.354, 44.348, 72.299]));
        assert_eq!(mapped.features.fillets[0].axis, FilletAxis::Principal('y'));
    }

    /// Where the motion keeps the grid (a signed permutation and a translation on it), the
    /// published values are those carried as found, cleared of the addition's binary noise.
    #[test]
    fn a_motion_on_the_grid_changes_nothing_but_noise() {
        let motion = quarter_turn([10.0, -3.5, 0.25]);
        let mapped = map_features(fillet([1.23456, -7.0004, 3.9994]), &motion);
        let (published, found) = (
            *mapped.features.fillets[0].at,
            *mapped.fingerprinted.fillets[0].at,
        );
        assert_eq!(published, [17.0, -2.265, 4.249]);
        assert!(
            (0..3).all(|i| (published[i] - found[i]).abs() < 1e-12),
            "{found:?}"
        );
        assert!(mapped.features.fillets[0].at.on_grid());
    }

    #[test]
    fn slot_grid_uses_measured_centres_and_preserves_file_axis_extents() {
        let mut slots = Vec::new();
        for row in 0..2 {
            for col in 0..3 {
                let x = 13.98076211353316 + col as f64 * 9.87654321;
                let y = -7.12345678 + row as f64 * 7.65432109;
                let placed = |v| Placed::new(v, Grid::Decimals(2));
                slots.push(Slot {
                    width_axis: 'y',
                    long_axis: 'x',
                    width: 2.,
                    length: 4.,
                    w_center: placed(y),
                    lo: placed(x - 2.),
                    hi: placed(x + 2.),
                    d_lo: placed(-5.),
                    d_hi: placed(5.),
                    body_key: None,
                    end_radius: None,
                    corner_radius: None,
                });
            }
        }
        let original = slots[0].clone();
        let motion = Rigid::from_placement(&crate::kernel::step::IDENTITY);
        let carried = slot(&mut Out::new(&motion, true, ""), original.clone());
        assert_eq!(carried, original);
        assert_eq!(*carried.lo, 11.98);
        let (s, c) = 0.37_f64.sin_cos();
        let motion = Rigid {
            rotation: [[c, -s, 0.], [s, c, 0.], [0., 0., 1.]],
            translation: [0.; 3],
        };
        let pattern = SlotPattern::SlotGrid {
            slots,
            rows: 2,
            cols: 3,
            row_pitch: 7.65,
            col_pitch: 9.88,
            angle: 0.,
            center: [0.; 3],
        };
        let SlotPattern::SlotGrid {
            slots,
            rows,
            cols,
            row_pitch,
            col_pitch,
            angle,
            center,
        } = slot_pattern(
            &mut Out::new(&motion, true, ""),
            pattern,
            &Features::default(),
        )
        else {
            panic!("grid retained")
        };
        assert_eq!((rows, cols, slots.len()), (2, 3, 6));
        assert!((row_pitch - 7.65432109).abs() < 1e-12);
        assert!((col_pitch - 9.87654321).abs() < 1e-12);
        assert!((angle - 0.37_f64.to_degrees()).abs() < 1e-12);
        let expected = motion.point([
            13.98076211353316 + 9.87654321,
            -7.12345678 + 7.65432109 / 2.,
            0.,
        ]);
        assert!((0..3).all(|i| (center[i] - expected[i]).abs() < 1e-12));
        assert_eq!(*slots[0].lo, *original.lo.raw());
    }
}
