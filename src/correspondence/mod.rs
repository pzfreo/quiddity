//! Correspondence between two revisions of a part, from their recognition results alone
//! (`docs/correspondence.md`): revision-stable [`fingerprint`]s, and [`correspond`], a pure
//! function of two sets of them.
//!
//! 1. **Align** ([`align`]): a rigid motion from anchors distinctive in both revisions, or the
//!    identity, marked unaligned.
//! 2. **Match the features**: per family, a cost matrix of semantics, neighbourhood and placement,
//!    solved one to one with an "unmatched" option ([`assign`]); each old feature is then carried,
//!    adapted, ambiguous or orphaned, and unmatched new ones are new.
//! 3. **Match the faces**: seeded from coincident faces and matched features' faces, then
//!    propagated over the attributed adjacency graph.
//!
//! Every threshold is in [`Thresholds`], with how it was chosen.

pub mod align;
pub mod assign;
mod congruent;
pub mod fingerprint;
mod revisions;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::features::{self, Features};
use crate::framed_records::{self, Mapped};
use crate::frames::{self, PartFrame, RecordFrame};
use crate::kernel::brep::Part;
use crate::kernel::geom::{self, V3};
use crate::published_records::PublishedFeatures;

pub use align::Motion;
pub use congruent::{congruent, congruent_with};
pub use fingerprint::{
    FINGERPRINT_REVISION, FINGERPRINT_VERSION, FaceFingerprint, FeatureFingerprint, Fingerprints,
    fingerprint,
};
pub use revisions::{ExcludedKey, FingerprintRevisions};

/// A recognition result with its fingerprints: what `quiddity part.step` writes. The families'
/// records are at the top level beside `fingerprints`. The recognition document reports the
/// provenance frame; every record is self-contained in file coordinates.
#[derive(Clone, Debug, Serialize)]
pub struct Recognition {
    #[serde(flatten)]
    pub features: PublishedFeatures,
    pub fingerprints: Fingerprints,
    #[serde(skip)]
    pub frame: RecordFrame,
    /// SHA256 of the exact input file bytes; absent for caller-supplied in-memory geometry.
    #[serde(skip)]
    pub source_sha256: Option<String>,
    /// Reported kernel input repairs, carried into the recognition document.
    #[serde(skip)]
    pub input_repairs: Vec<String>,
    /// Positive body evidence in the inferred frame; never a feature claim.
    #[serde(skip)]
    pub(crate) body_evidence: BTreeMap<usize, crate::recognition::BodyReason>,
}

/// Recognise a loaded part in its inferred frame and map records back to caller space.
/// Nonprincipal directions use vectors and explicit origins; `frame` is provenance only.
/// If frame inference is refused, records are caller-space and `frame` carries the reason.
/// This performs no STEP reads and preserves face and edge indices.
pub fn recognise(part: &Part) -> Recognition {
    let frame = match frames::infer_part_frame(part) {
        Ok(frame) => frame,
        Err(reason) => {
            return features::recognise_with(part, |ctx, features| {
                let features: PublishedFeatures = features.into();
                let fingerprints = fingerprint::published_fingerprint_in(part, &features, ctx);
                Recognition {
                    features,
                    fingerprints,
                    frame: RecordFrame::Refused { reason },
                    source_sha256: None,
                    input_repairs: part.input_repairs().to_vec(),
                    body_evidence: BTreeMap::new(),
                }
            });
        }
    };
    recognise_in_frame(part, frame)
}

fn recognise_in_frame(part: &Part, frame: PartFrame) -> Recognition {
    let working = match part.transformed(&frame.normalisation()) {
        Ok(working) => working,
        Err(_) => {
            // A failed normalisation must never silently publish caller-space features.
            let features = Features::default();
            return Recognition {
                fingerprints: fingerprint(part, &features),
                features: features.into(),
                frame: RecordFrame::Refused {
                    reason: frames::FrameRefusal::NonrigidFrame,
                },
                source_sha256: None,
                input_repairs: part.input_repairs().to_vec(),
                body_evidence: BTreeMap::new(),
            };
        }
    };
    features::recognise_with(&working, |ctx, features| {
        let body_evidence = crate::body::evidence(&working, &features);
        let Mapped {
            features,
            fingerprinted,
            motion,
        } = framed_records::to_file(features, &frame, &working);
        let _ = motion;
        // Fingerprints read the published values as found in the frame, carried through the exact
        // motion, so they stay intrinsic to the part: publishing on the file's grid would make a
        // derived size (hi − lo) depend on where the part is placed, and a near-aligned frame's
        // publication in the file's axes (#70) would change its axes by the round-off it drops.
        let fingerprints = fingerprint::published_fingerprint_in(part, &fingerprinted, ctx);
        Recognition {
            features,
            fingerprints,
            frame: RecordFrame::Inferred(frame),
            source_sha256: None,
            input_repairs: part.input_repairs().to_vec(),
            body_evidence,
        }
    })
}

/// Caller-space recognition for internal regression and geometry tests.
#[cfg(test)]
pub(crate) fn caller_space_recognise(part: &Part) -> Recognition {
    features::recognise_with(part, |ctx, features| {
        let features: PublishedFeatures = features.into();
        let fingerprints = fingerprint::published_fingerprint_in(part, &features, ctx);
        Recognition {
            features,
            fingerprints,
            frame: RecordFrame::CallerSpace,
            source_sha256: None,
            input_repairs: part.input_repairs().to_vec(),
            body_evidence: BTreeMap::new(),
        }
    })
}

/// The thresholds and weights `correspond` uses. Lengths are relative to the part's scale (the
/// square root of its area, `Fingerprints::scale`); costs are sums of terms each in [0, 1].
///
/// How they were chosen (`docs/correspondence.md` asks for them to be tuned on revision pairs,
/// since no published values transfer; its "Implementation" section has the evidence):
///
/// - The *tolerances* (`same_rel`, `position_tol`, `angle_tol`) only have to absorb round-off:
///   a corpus part read moved and turned gives analytic fingerprints equal to ~1e-9 relative,
///   and so do a build123d revision's unchanged faces. The exception is a kernel defect, not
///   round-off: a sphere face whose boundary runs through the sphere's pole (cgb202's corner
///   patches) gets an area from the kernel that changes several-fold with the placement, listed
///   in `references/correspondence.json`; no tolerance would absorb it. They sit three or four orders above that
///   and well below any design change (a micron on a 100 mm part). `freeform_rel` is looser
///   because the kernel's B-spline quadrature moves with the placement (up to 2e-4 on most
///   faces, more on a few listed in `references/correspondence.json`).
/// - `anchor_candidates`, `anchor_pairs` and `hypotheses` bound the work. Four candidates keep
///   the faces of two-fold and four-fold symmetric parts as anchors; RANSAC needs only one clean
///   hypothesis among them.
/// - `min_inliers` and `min_inlier_fraction`: at least three anchors, and half of those with a
///   candidate, must agree. The aligned revision pairs keep at least 70% of their 8 to 15 anchors; the thickened
///   plate (every face but one changed) keeps none and is unaligned, as it should be.
/// - `cutoff` and `margin`, on the revision pairs: matched features cost 0.06 to 0.5 (a moved
///   hole 0.24, a grown pattern 0.5), so the cut-off of 1 leaves room above the worst. Unaligned,
///   swapping two identical holes 20 mm apart on the 100 mm plate costs about 0.064 and 5 to
///   10 mm apart 0.016 to 0.032: the margin of 0.05 separates the two, so a far repeat is still
///   placed by the identity and a near one is reported ambiguous. Aligned, displacements cost
///   by their logarithm in tolerances, so repeats a few millimetres apart differ by ~0.5.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// Sizes, areas and surface parameters agree when within this relative difference.
    pub same_rel: f64,
    /// A freeform face's area agrees within this relative difference, and its mean normal within
    /// this angle (radians): the kernel's quadrature on B-spline faces changes with the part's
    /// placement by up to about 2e-4 relative (more on a few faces, listed in
    /// `references/correspondence.json`).
    pub freeform_rel: f64,
    /// Points agree after alignment within this fraction of the scale.
    pub position_tol: f64,
    /// Directions agree after alignment within this angle, radians.
    pub angle_tol: f64,
    /// An anchor with more candidates than this in the other revision is not used to align.
    pub anchor_candidates: usize,
    /// On a turned part, items off the axis with up to this many candidates vote for the turn
    /// about it (repeats round a bolt circle have as many candidates as the circle has holes,
    /// or twice that with two alike flanges).
    pub turn_candidates: usize,
    /// At most this many candidate anchor pairs, the most distinctive first.
    pub anchor_pairs: usize,
    /// Hypotheses tried, of each size (two pairs, three pairs), when there are more.
    pub hypotheses: usize,
    pub min_inliers: usize,
    pub min_inlier_fraction: f64,
    pub semantics_weight: f64,
    pub neighbourhood_weight: f64,
    pub placement_weight: f64,
    /// The placement weight's factor when no alignment was found.
    pub unaligned_placement: f64,
    /// A displacement of this fraction of the scale costs the whole position term.
    pub placement_reach: f64,
    /// A match costing more than this is no match (each side left unmatched costs half).
    pub cutoff: f64,
    /// A match whose best alternative costs less than this more is ambiguous.
    pub margin: f64,
    /// Re-solves allowed per family to settle margins the lower bound does not; beyond it the
    /// unsettled entries are reported ambiguous.
    pub resolves: usize,
    /// Two faces on one surface are taken as the same face when their areas agree within this
    /// ratio and their centroids within this fraction of the smaller face's size: the overlap
    /// test, estimated from the fingerprints.
    pub overlap: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            same_rel: 1e-6,
            freeform_rel: 1e-3,
            position_tol: 1e-5,
            angle_tol: 1e-5,
            anchor_candidates: 4,
            turn_candidates: 64,
            anchor_pairs: 400,
            hypotheses: 3000,
            min_inliers: 3,
            min_inlier_fraction: 0.5,
            semantics_weight: 1.0,
            neighbourhood_weight: 1.0,
            placement_weight: 1.0,
            unaligned_placement: 0.1,
            placement_reach: 0.25,
            cutoff: 1.0,
            margin: 0.05,
            resolves: 200,
            overlap: 0.8,
        }
    }
}

/// How an old feature or face fares in the new revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// The same, unchanged. In an unaligned result (`Alignment::found` false) placement is not
    /// compared, since there is no frame to compare it in: carried then means unchanged in
    /// semantics and neighbourhood (a face: parameters, area and adjacency), wherever it is.
    Carried,
    /// The same, with changed parameters (in `changes`; placement only when aligned).
    Adapted,
    /// Two or more candidates within the margin: none is guessed.
    Ambiguous,
    /// No match in the new revision.
    Orphaned,
    /// In the new revision only.
    New,
}

/// The alignment found (or not) between the revisions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    pub found: bool,
    /// Rows of the rotation taking the old revision's frame to the new one's.
    pub rotation: [[f64; 3]; 3],
    pub translation: V3,
    /// Old anchors with a candidate in the new revision, and how many agree with the motion.
    pub anchors: usize,
    pub inliers: usize,
    /// Another, distinct motion fits as many anchors: the part is symmetric, and which of the
    /// symmetric copies is which was decided by preferring the motion nearest the identity. Also
    /// set when every anchor lies on one line (an axisymmetric part), whose turn about that line
    /// the anchors leave free (`axisymmetric`).
    pub symmetric: bool,
    /// Every anchor lies on one line and no off-axis repeats fix the turn about it (see `fold`),
    /// so that turn is arbitrary: positions are compared by their distance along and from the
    /// line, and directions by their angle to it.
    pub axisymmetric: bool,
    /// That line in the new revision: a point on it and its direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<[V3; 2]>,
    /// When the anchors lay on one line but repeats off it then fixed the turn about it: how many
    /// turns fit them equally, the part's rotational symmetry about the line (1 when the turn is
    /// unique). The turn giving the motion nearest the identity is taken, and `symmetric` is set
    /// when there are several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fold: Option<usize>,
    /// Root mean square distance of the inlier anchors after alignment, mm.
    pub rms: f64,
}

/// A field that changed: its old and new values (positions and axes in the new frame).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub old: Value,
    pub new: Value,
}

/// One old item's fate. `T` is a feature id or a face index.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry<T> {
    pub old: T,
    pub class: Class,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<T>,
    /// For an ambiguous entry, every new item within the margin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<T>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub changes: BTreeMap<String, Change>,
    /// The match's cost, where matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

/// What `correspond` finds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Correspondence {
    pub fingerprint_version: String,
    /// Value vintages compared, always emitted, including when equal or unknown (0).
    #[serde(default)]
    pub revisions: FingerprintRevisions,
    /// Family/key pairs not comparable across these vintages, sorted and deduplicated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_keys: Vec<ExcludedKey>,
    pub alignment: Alignment,
    /// One entry per old feature, in the old result's order.
    pub features: Vec<Entry<String>>,
    /// New features nothing was matched to.
    pub new_features: Vec<String>,
    /// One entry per old face.
    pub faces: Vec<Entry<usize>>,
    pub new_faces: Vec<usize>,
    /// One question per ambiguous entry, for whoever owns the decisions to answer.
    pub questions: Vec<String>,
}

/// Matches the *old* revision's features and faces to the *new* one's.
pub fn correspond(old: &Fingerprints, new: &Fingerprints) -> Result<Correspondence, String> {
    correspond_with(old, new, &Thresholds::default())
}

/// [`correspond`] with explicit thresholds.
pub fn correspond_with(
    old: &Fingerprints,
    new: &Fingerprints,
    th: &Thresholds,
) -> Result<Correspondence, String> {
    let merged = |f: &Fingerprints| {
        (f.fingerprint_version == FINGERPRINT_VERSION
            && f.features.iter().any(|feature| {
                matches!(
                    feature.family.as_str(),
                    "oriented_slots" | "oriented_chamfers" | "oriented_slot_patterns"
                )
            }))
        .then(|| {
            let mut f = f.clone();
            fingerprint::merge_legacy_families(&mut f);
            f
        })
    };
    let old_merged = merged(old);
    let new_merged = merged(new);
    let old = old_merged.as_ref().unwrap_or(old);
    let new = new_merged.as_ref().unwrap_or(new);
    for (which, f) in [("old", old), ("new", new)] {
        validate(f).map_err(|e| format!("the {which} fingerprints {e}"))?;
    }
    let revisions = FingerprintRevisions {
        old: old.fingerprint_revision,
        new: new.fingerprint_revision,
    };
    let excluded_keys = revisions::excluded_keys(old, new);
    // Work on comparison views only. Excluded semantics must not reject alignment anchors,
    // and neither costs nor change reports may observe their original values.
    let anchors_old = revisions::comparison_view(old, &excluded_keys, true);
    let anchors_new = revisions::comparison_view(new, &excluded_keys, true);
    let (motion, alignment) = align::align(
        anchors_old.as_ref().unwrap_or(old),
        anchors_new.as_ref().unwrap_or(new),
        th,
    );
    let comparable_old = revisions::comparison_view(old, &excluded_keys, false);
    let comparable_new = revisions::comparison_view(new, &excluded_keys, false);
    let old = comparable_old.as_ref().unwrap_or(old);
    let new = comparable_new.as_ref().unwrap_or(new);
    let ctx = Ctx {
        old,
        new,
        th,
        motion,
        aligned: alignment.found,
        axis: alignment.axis.map(|[p, d]| (p, d)),
        scale: old.scale.max(new.scale),
        excluded_keys: &excluded_keys,
    };
    let mut questions = Vec::new();
    let (features, new_features) = ctx.match_features(&mut questions);
    let (faces, new_faces) = ctx.match_faces(&features, &mut questions);
    Ok(Correspondence {
        fingerprint_version: FINGERPRINT_VERSION.into(),
        revisions,
        excluded_keys,
        alignment,
        features,
        new_features,
        faces,
        new_faces,
        questions,
    })
}

/// Whether fingerprints read from a file can be compared: of this version, with a finite
/// positive scale, each face at its own index, and every face a feature or a face names among
/// them. A hand-edited or truncated file is refused here rather than indexed out of range.
fn validate(f: &Fingerprints) -> Result<(), String> {
    if f.fingerprint_version != FINGERPRINT_VERSION {
        return Err(format!(
            "are version {:?}, not {FINGERPRINT_VERSION:?}; recognise the file again",
            f.fingerprint_version
        ));
    }
    if !(f.scale.is_finite() && f.scale > 0.0) {
        return Err(format!(
            "have scale {}, not a finite positive size",
            f.scale
        ));
    }
    let n = f.faces.len();
    for (i, face) in f.faces.iter().enumerate() {
        if face.index != i {
            return Err(format!("list face {} at position {i}", face.index));
        }
        if let Some((g, _)) = face.neighbours.iter().find(|(g, _)| *g >= n) {
            return Err(format!("give face {i} neighbour {g}, beyond the {n} faces"));
        }
    }
    for feature in &f.features {
        if let Some(g) = feature.faces.iter().find(|&&g| g >= n) {
            return Err(format!(
                "give feature {} face {g}, beyond the {n} faces",
                feature.id
            ));
        }
    }
    Ok(())
}

/// Whether two values agree within *rel* of the larger (or absolutely, near zero).
pub(crate) fn same_value(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * a.abs().max(b.abs()).max(1e-3)
}

/// How far apart two sorted multisets of strings are: 0 when equal, 1 when disjoint.
pub(crate) fn multiset_distance(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut common) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                common += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    1.0 - 2.0 * common as f64 / (a.len() + b.len()) as f64
}

/// The entries one multiset has and the other lacks: (removed, added).
fn multiset_changes(a: &[String], b: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) if x == y => {
                i += 1;
                j += 1;
            }
            (Some(x), Some(y)) if x < y => {
                removed.push(x.clone());
                i += 1;
            }
            (Some(x), None) => {
                removed.push(x.clone());
                i += 1;
            }
            (_, Some(y)) => {
                added.push(y.clone());
                j += 1;
            }
            (None, None) => break,
        }
    }
    (removed, added)
}

/// The mean relative difference of two named value sets, a missing name counting 1.
fn sizes_distance(a: &BTreeMap<String, f64>, b: &BTreeMap<String, f64>) -> (f64, usize) {
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let mut sum = 0.0;
    for k in &keys {
        sum += match (a.get(*k), b.get(*k)) {
            (Some(x), Some(y)) => {
                let d = (x - y).abs() / x.abs().max(y.abs()).max(1e-9);
                d.min(1.0)
            }
            _ => 1.0,
        };
    }
    (sum, keys.len())
}

fn round(x: f64) -> f64 {
    let r = (x * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}

fn rounded(p: V3) -> Value {
    json!(p.map(round))
}

struct Ctx<'a> {
    old: &'a Fingerprints,
    new: &'a Fingerprints,
    th: &'a Thresholds,
    motion: Motion,
    aligned: bool,
    /// The line about which an axisymmetric alignment's turn is free.
    axis: Option<(V3, V3)>,
    scale: f64,
    excluded_keys: &'a [ExcludedKey],
}

impl Ctx<'_> {
    fn tol(&self) -> f64 {
        self.th.position_tol * self.scale
    }

    /// How far old point *p*, moved, is from new point *q*: about an axisymmetric alignment's
    /// line, in distance along it and from it only.
    fn gap(&self, p: V3, q: V3) -> f64 {
        self.gap_along(p, q, None)
    }

    /// [`Self::gap`] with the component along unit direction *along* (in the new frame) removed.
    fn gap_along(&self, p: V3, q: V3, along: Option<V3>) -> f64 {
        let mut p = self.motion.point(p);
        if let Some(d) = along {
            p = geom::sub(p, geom::scale(d, geom::dot(geom::sub(p, q), d)));
        }
        match self.axis {
            Some((o, d)) => {
                let split = |x: V3| {
                    let v = geom::sub(x, o);
                    let along = geom::dot(v, d);
                    (along, geom::norm(geom::sub(v, geom::scale(d, along))))
                };
                let ((a1, r1), (a2, r2)) = (split(p), split(q));
                (a1 - a2).hypot(r1 - r2)
            }
            None => geom::dist(p, q),
        }
    }

    /// The cosine between old direction *x*, moved, and new direction *y* (as lines when *line*);
    /// about an axisymmetric alignment's line, how alike their angles to the line are.
    fn agreement(&self, x: V3, y: V3, line: bool) -> f64 {
        let x = self.motion.dir(x);
        let fold = |c: f64| if line { c.abs() } else { c };
        match self.axis {
            Some((_, d)) => {
                let (a, b) = (fold(geom::dot(x, d)), fold(geom::dot(y, d)));
                // The same angle to the line gives 1; the cosine of the angle between them else.
                (a * b + ((1.0 - a * a).max(0.0) * (1.0 - b * b).max(0.0)).sqrt()).min(1.0)
            }
            None => fold(geom::dot(x, y)).min(1.0),
        }
    }

    fn placement_weight(&self) -> f64 {
        self.th.placement_weight
            * if self.aligned {
                1.0
            } else {
                self.th.unaligned_placement
            }
    }

    /// What a displacement of *d* costs, in [0, 1]. Aligned, positions are good to the
    /// tolerance, so the cost rises with the logarithm of the displacement in tolerances: a
    /// shift of a few millimetres already tells two features apart, and the whole reach costs 1.
    /// Unaligned, positions are only a rough guide, and the cost is linear up to the reach.
    fn displacement(&self, d: f64) -> f64 {
        let reach = self.th.placement_reach * self.scale;
        if self.aligned {
            let tol = self.tol();
            ((1.0 + d / tol).ln() / (1.0 + reach / tol).ln()).min(1.0)
        } else {
            (d / reach).min(1.0)
        }
    }

    /// Whether *key* is excluded for features of *family*.
    fn excludes(&self, family: &str, key: &str) -> bool {
        self.excluded_keys
            .iter()
            .any(|e| e.family == family && e.key == key)
    }

    fn placement_parts(
        &self,
        a: (Option<V3>, Option<V3>),
        b: (Option<V3>, Option<V3>),
        compare_axis: bool,
        compare_position: bool,
        axial_excluded: bool,
    ) -> f64 {
        let axis = match (a.0, b.0) {
            (Some(x), Some(y)) => 1.0 - self.agreement(x, y, true),
            (None, None) => 0.0,
            _ => 1.0,
        };
        let position = match (a.1, b.1) {
            (Some(p), Some(q)) => {
                let along = if axial_excluded {
                    b.0.filter(|_| a.0.is_some())
                } else {
                    None
                };
                self.displacement(self.gap_along(p, q, along))
            }
            _ => 0.5,
        };
        0.5 * if compare_axis { axis } else { 0.0 }
            + 0.5 * if compare_position { position } else { 0.0 }
    }

    fn feature_cost(&self, a: &FeatureFingerprint, b: &FeatureFingerprint) -> f64 {
        let trait_keys: BTreeSet<&String> = a.traits.keys().chain(b.traits.keys()).collect();
        let differing = trait_keys
            .iter()
            .filter(|k| a.traits.get(**k) != b.traits.get(**k))
            .count();
        let (size_sum, size_n) = sizes_distance(&a.sizes, &b.sizes);
        let traits = if trait_keys.is_empty() {
            0.0
        } else {
            differing as f64 / trait_keys.len() as f64
        };
        let semantics = (traits + size_sum) / (1.0 + size_n as f64);
        let neighbourhood = multiset_distance(&a.neighbourhood, &b.neighbourhood);
        let placement = self.placement_parts(
            (a.axis, a.position),
            (b.axis, b.position),
            !self.excludes(&a.family, "axis"),
            !self.excludes(&a.family, "position"),
            self.excludes(&a.family, "position.axial"),
        );
        self.th.semantics_weight * semantics
            + self.th.neighbourhood_weight * neighbourhood
            + self.placement_weight() * placement
    }

    fn feature_changes(
        &self,
        a: &FeatureFingerprint,
        b: &FeatureFingerprint,
    ) -> BTreeMap<String, Change> {
        let mut out = BTreeMap::new();
        let keys: BTreeSet<&String> = a.traits.keys().chain(b.traits.keys()).collect();
        for k in keys {
            if a.traits.get(k) != b.traits.get(k) {
                out.insert(
                    format!("traits.{k}"),
                    Change {
                        old: json!(a.traits.get(k)),
                        new: json!(b.traits.get(k)),
                    },
                );
            }
        }
        let keys: BTreeSet<&String> = a.sizes.keys().chain(b.sizes.keys()).collect();
        for k in keys {
            let (x, y) = (a.sizes.get(k), b.sizes.get(k));
            let same = matches!((x, y), (Some(x), Some(y)) if same_value(*x, *y, self.th.same_rel));
            if !same {
                out.insert(
                    format!("sizes.{k}"),
                    Change {
                        old: json!(x),
                        new: json!(y),
                    },
                );
            }
        }
        if a.neighbourhood != b.neighbourhood {
            let (removed, added) = multiset_changes(&a.neighbourhood, &b.neighbourhood);
            out.insert(
                "neighbourhood".into(),
                Change {
                    old: json!(removed),
                    new: json!(added),
                },
            );
        }
        if self.aligned {
            self.placement_changes(
                &mut out,
                (a.axis, a.position),
                (b.axis, b.position),
                true,
                self.th.angle_tol,
                self.excludes(&a.family, "position.axial"),
            );
        }
        out
    }

    /// Axis (or normal) and position changes, the old ones carried into the new frame.
    fn placement_changes(
        &self,
        out: &mut BTreeMap<String, Change>,
        a: (Option<V3>, Option<V3>),
        b: (Option<V3>, Option<V3>),
        line: bool,
        angle_tol: f64,
        axial_excluded: bool,
    ) {
        if let (Some(x), Some(y)) = (a.0, b.0)
            && self.agreement(x, y, line) < angle_tol.cos()
        {
            out.insert(
                if line { "axis" } else { "normal" }.into(),
                Change {
                    old: rounded(self.motion.dir(x)),
                    new: rounded(y),
                },
            );
        }
        if let (Some(p), Some(q)) = (a.1, b.1)
            && self.gap_along(
                p,
                q,
                if axial_excluded {
                    b.0.filter(|_| a.0.is_some())
                } else {
                    None
                },
            ) > self.tol()
        {
            out.insert(
                "position".into(),
                Change {
                    old: rounded(self.motion.point(p)),
                    new: rounded(q),
                },
            );
        }
    }

    fn match_features(&self, questions: &mut Vec<String>) -> (Vec<Entry<String>>, Vec<String>) {
        let families: BTreeSet<&String> = self
            .old
            .features
            .iter()
            .chain(&self.new.features)
            .map(|f| &f.family)
            .collect();
        let mut entries: Vec<Option<Entry<String>>> = vec![None; self.old.features.len()];
        let mut taken = vec![false; self.new.features.len()];
        for family in families {
            let olds: Vec<usize> = (0..self.old.features.len())
                .filter(|&i| &self.old.features[i].family == family)
                .collect();
            let news: Vec<usize> = (0..self.new.features.len())
                .filter(|&j| &self.new.features[j].family == family)
                .collect();
            let costs: Vec<Vec<f64>> = olds
                .iter()
                .map(|&i| {
                    news.iter()
                        .map(|&j| self.feature_cost(&self.old.features[i], &self.new.features[j]))
                        .collect()
                })
                .collect();
            let ids = |js: &[usize]| -> Vec<String> {
                js.iter()
                    .map(|&j| self.new.features[news[j]].id.clone())
                    .collect()
            };
            for (r, outcome) in self.resolve(&costs, news.len()).into_iter().enumerate() {
                let a = &self.old.features[olds[r]];
                let entry = match outcome {
                    Outcome::Matched(j, cost) => {
                        taken[news[j]] = true;
                        let b = &self.new.features[news[j]];
                        let changes = self.feature_changes(a, b);
                        Entry {
                            old: a.id.clone(),
                            class: if changes.is_empty() {
                                Class::Carried
                            } else {
                                Class::Adapted
                            },
                            new: Some(b.id.clone()),
                            candidates: Vec::new(),
                            changes,
                            cost: Some(round(cost)),
                        }
                    }
                    Outcome::Ambiguous(js) => {
                        for &j in &js {
                            taken[news[j]] = true;
                        }
                        let candidates = ids(&js);
                        questions.push(format!(
                            "Which of {} is the old {}? They are alike within the margin (a pattern's instances or symmetric features).",
                            candidates.join(", "),
                            a.id
                        ));
                        Entry {
                            old: a.id.clone(),
                            class: Class::Ambiguous,
                            new: None,
                            candidates,
                            changes: BTreeMap::new(),
                            cost: None,
                        }
                    }
                    Outcome::Unmatched => Entry {
                        old: a.id.clone(),
                        class: Class::Orphaned,
                        new: None,
                        candidates: Vec::new(),
                        changes: BTreeMap::new(),
                        cost: None,
                    },
                };
                entries[olds[r]] = Some(entry);
            }
        }
        let new_features = (0..self.new.features.len())
            .filter(|&j| !taken[j])
            .map(|j| self.new.features[j].id.clone())
            .collect();
        (
            entries.into_iter().map(Option::unwrap).collect(),
            new_features,
        )
    }

    /// The assignment over *costs*, each row's outcome settled against the margin: a match whose
    /// best alternative is within the margin, or an unmatched row that could be matched within
    /// it, is ambiguous with every candidate that comes within it.
    fn resolve(&self, costs: &[Vec<f64>], m: usize) -> Vec<Outcome> {
        let unmatched = 0.5 * self.th.cutoff;
        let margin = self.th.margin;
        let solution = assign::solve(costs, m, unmatched);
        let mut budget = self.th.resolves;
        let mut spend = || {
            if budget == 0 {
                return false;
            }
            budget -= 1;
            true
        };
        let mut out = Vec::new();
        for (i, row) in costs.iter().enumerate() {
            let current = solution.matched[i];
            // Candidates: columns whose forcing costs less than the margin. The reduced cost is
            // a lower bound on that; only those under it are solved again.
            let mut candidates = Vec::new();
            let mut unsettled = false;
            for j in 0..m {
                if Some(j) == current || solution.reduced(i, Some(j)) >= margin {
                    continue;
                }
                if !spend() {
                    unsettled = true;
                    candidates.push(j);
                    continue;
                }
                if assign::force(costs, m, unmatched, i, j) < margin {
                    candidates.push(j);
                }
            }
            // Staying unmatched instead of the current match.
            let mut weak = false;
            if current.is_some() && solution.reduced(i, None) < margin {
                if !spend() {
                    unsettled = true;
                } else if assign::force_unmatched(costs, m, unmatched, i) < margin {
                    weak = true;
                }
            }
            out.push(match current {
                Some(j) if candidates.is_empty() && !weak && !unsettled => {
                    Outcome::Matched(j, row[j])
                }
                None if candidates.is_empty() && !unsettled => Outcome::Unmatched,
                // Several candidates within the margin, or one that is barely better than none.
                _ => {
                    let mut all: Vec<usize> = current.into_iter().chain(candidates).collect();
                    all.sort_unstable();
                    Outcome::Ambiguous(all)
                }
            });
        }
        out
    }
}

enum Outcome {
    Matched(usize, f64),
    Ambiguous(Vec<usize>),
    Unmatched,
}

mod faces;

#[cfg(test)]
mod tests {
    use super::*;

    /// Review finding L2: a fingerprints file naming a face that is not there (a feature's face
    /// 999) was indexed out of range; such files are refused, either side.
    #[test]
    fn correspond_refuses_fingerprints_that_do_not_hold_together() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prismatic.step");
        let good = recognise(&crate::read_step_file(&path).unwrap()).fingerprints;
        assert!(!good.features.is_empty());
        assert!(correspond(&good, &good).is_ok());
        type Mutation = fn(&mut Fingerprints);
        let bad: [(&str, Mutation); 7] = [
            ("face 999, beyond", |f| f.features[0].faces = vec![999]),
            ("neighbour 999", |f| {
                f.faces[0].neighbours.push((999, "convex".into()))
            }),
            ("at position 1", |f| f.faces[1].index = 0),
            ("scale 0", |f| f.scale = 0.0),
            ("scale NaN", |f| f.scale = f64::NAN),
            ("version", |f| f.fingerprint_version = "other".into()),
            ("fingerprint/2", |f| {
                f.fingerprint_version = "quiddity-rust/fingerprint/2".into()
            }),
        ];
        for (expected, mutate) in bad {
            let mut f = good.clone();
            mutate(&mut f);
            // Read back as `quiddity correspond` reads a file, where JSON can carry it (it has
            // no NaN).
            let f = serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap_or(f);
            for (old, new) in [(&f, &good), (&good, &f)] {
                let error = correspond(old, new).unwrap_err();
                assert!(error.contains(expected), "{expected}: {error}");
            }
        }
    }
}

#[cfg(test)]
mod normalisation_tests {
    use super::*;
    #[test]
    fn invalid_normalisation_refuses_without_caller_space_records() {
        let part = crate::read_step(include_bytes!(
            "../../tests/fixtures/large_block_small_fillets.step"
        ))
        .unwrap();
        let frame = PartFrame {
            origin: [0.; 3],
            x: [1., 0., 0.],
            y: [2e-12, 1., 0.],
            z: [0., 0., 1.],
            gauge: frames::FrameGauge::Full,
        };
        let result = recognise_in_frame(&part, frame);
        assert_eq!(
            result.frame,
            RecordFrame::Refused {
                reason: frames::FrameRefusal::NonrigidFrame
            }
        );
        let mut expected = serde_json::to_value(Features::default()).unwrap();
        for family in [
            "oriented_slots",
            "oriented_chamfers",
            "oriented_slot_patterns",
        ] {
            expected.as_object_mut().unwrap().remove(family);
        }
        assert_eq!(serde_json::to_value(&result.features).unwrap(), expected);
        assert!(result.fingerprints.features.is_empty());
    }
}
