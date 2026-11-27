//! Step 1 of `correspond`: the rigid motion taking the old revision onto the new one, from
//! fingerprints distinctive in both (`docs/correspondence.md`, "Align"). Candidate pairs are
//! anchors (features and faces) whose motion-free fingerprints agree; hypotheses are solved
//! from two or three pairs at a time (Horn's closed form, the quaternion version of Kabsch), and
//! the one most anchors agree with wins (RANSAC, over a deterministic sample).

// Matrix code reads clearest indexed.
#![allow(clippy::needless_range_loop)]

use crate::kernel::geom::{self, V3};

use super::fingerprint::{FaceFingerprint, FeatureFingerprint, Fingerprints};
use super::{Alignment, Thresholds, multiset_distance, same_value};

/// A proper rigid motion: `p ↦ R p + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub r: [[f64; 3]; 3],
    pub t: V3,
}

impl Motion {
    pub const IDENTITY: Motion = Motion {
        r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t: [0.0; 3],
    };

    pub fn point(&self, p: V3) -> V3 {
        geom::add(self.dir(p), self.t)
    }

    pub fn dir(&self, d: V3) -> V3 {
        [0, 1, 2].map(|i| geom::dot(self.r[i], d))
    }
}

/// An anchor: a point, and a direction where it has one (signed for a face's normal, a line for
/// a feature's axis).
#[derive(Clone, Copy, Debug)]
struct Anchor {
    point: V3,
    dir: Option<V3>,
    line: bool,
}

/// A candidate pair: indices into the old and new anchor lists.
type Pair = (usize, usize);

fn feature_anchor(f: &FeatureFingerprint) -> Option<Anchor> {
    Some(Anchor {
        point: f.position?,
        dir: f.axis,
        line: true,
    })
}

fn face_anchor(f: &FaceFingerprint) -> Anchor {
    Anchor {
        point: f.centroid,
        dir: f.normal,
        line: false,
    }
}

/// Whether two features could be the same feature unmoved and unchanged: everything but their
/// placement agrees.
fn same_feature(a: &FeatureFingerprint, b: &FeatureFingerprint, rel: f64) -> bool {
    // The source fillet faces already vote for alignment.
    !crate::features::is_fillet_aggregate(&a.family)
        && a.family == b.family
        && a.traits == b.traits
        && a.sizes.len() == b.sizes.len()
        && a.sizes
            .iter()
            .all(|(k, x)| b.sizes.get(k).is_some_and(|y| same_value(*x, *y, rel)))
        && a.neighbourhood == b.neighbourhood
}

fn same_face(a: &FaceFingerprint, b: &FaceFingerprint, rel: f64) -> bool {
    a.surface == b.surface
        && a.parameters.len() == b.parameters.len()
        && a.parameters
            .iter()
            .all(|(k, x)| b.parameters.get(k).is_some_and(|y| same_value(*x, *y, rel)))
        && match (a.area, b.area) {
            (Some(x), Some(y)) => same_value(x, y, rel),
            _ => false,
        }
        && multiset_distance(&a.adjacency, &b.adjacency) == 0.0
}

/// Candidate pairs among items: each old item paired with every new one *same* accepts, keeping
/// only items with at most `max` candidates either way (a repeat seen more often than that
/// cannot anchor anything).
fn candidates<T>(
    old: &[T],
    new: &[T],
    same: impl Fn(&T, &T) -> bool,
    max: usize,
) -> Vec<(usize, usize, usize)> {
    let mut pairs = Vec::new();
    let mut per_new = vec![0usize; new.len()];
    for (i, a) in old.iter().enumerate() {
        let mine: Vec<usize> = (0..new.len()).filter(|&j| same(a, &new[j])).collect();
        for &j in &mine {
            per_new[j] += 1;
        }
        if !mine.is_empty() && mine.len() <= max {
            for &j in &mine {
                pairs.push((i, j, mine.len()));
            }
        }
    }
    pairs.retain(|&(_, j, _)| per_new[j] <= max);
    pairs
        .into_iter()
        .map(|(i, j, n)| (i, j, n.max(per_new[j])))
        .collect()
}

/// Both revisions' anchors (features first, then faces) and the candidate pairs between them.
struct Candidates {
    old: Vec<Anchor>,
    new: Vec<Anchor>,
    pairs: Vec<Pair>,
}

impl Candidates {
    fn of(old: &Fingerprints, new: &Fingerprints, th: &Thresholds) -> Candidates {
        // Anchors: features first, then faces.
        let mut old_anchors = Vec::new();
        let mut new_anchors = Vec::new();
        let mut pairs: Vec<(Pair, usize, f64)> = Vec::new();
        let feature_pairs = candidates(
            &old.features,
            &new.features,
            |a, b| same_feature(a, b, th.same_rel),
            th.anchor_candidates,
        );
        let mut index_old = vec![usize::MAX; old.features.len()];
        let mut index_new = vec![usize::MAX; new.features.len()];
        for (i, j, n) in feature_pairs {
            let (Some(a), Some(b)) = (
                feature_anchor(&old.features[i]),
                feature_anchor(&new.features[j]),
            ) else {
                continue;
            };
            if index_old[i] == usize::MAX {
                index_old[i] = old_anchors.len();
                old_anchors.push(a);
            }
            if index_new[j] == usize::MAX {
                index_new[j] = new_anchors.len();
                new_anchors.push(b);
            }
            pairs.push(((index_old[i], index_new[j]), n, f64::INFINITY));
        }
        let face_pairs = candidates(
            &old.faces,
            &new.faces,
            |a, b| same_face(a, b, th.same_rel),
            th.anchor_candidates,
        );
        let mut index_old = vec![usize::MAX; old.faces.len()];
        let mut index_new = vec![usize::MAX; new.faces.len()];
        for (i, j, n) in face_pairs {
            let Some(area) = old.faces[i].area else {
                continue;
            };
            if index_old[i] == usize::MAX {
                index_old[i] = old_anchors.len();
                old_anchors.push(face_anchor(&old.faces[i]));
            }
            if index_new[j] == usize::MAX {
                index_new[j] = new_anchors.len();
                new_anchors.push(face_anchor(&new.faces[j]));
            }
            pairs.push(((index_old[i], index_new[j]), n, area));
        }
        // The most distinctive first (fewest candidates; then features, then the larger faces),
        // as many as the budget allows.
        pairs.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.total_cmp(&a.2)));
        pairs.truncate(th.anchor_pairs);
        Candidates {
            old: old_anchors,
            new: new_anchors,
            pairs: pairs.into_iter().map(|p| p.0).collect(),
        }
    }

    /// How many pairs *m* takes each anchor onto its partner for (each anchor counted once),
    /// the rms of their distances, and every pair it does.
    fn inliers(&self, m: &Motion, tol: f64, cos_tol: f64) -> (usize, f64, Vec<Pair>) {
        let mut hit_old = vec![false; self.old.len()];
        let mut hit_new = vec![false; self.new.len()];
        let (mut count, mut sq) = (0, 0.0);
        let mut kept = Vec::new();
        for &(i, j) in &self.pairs {
            let (a, b) = (&self.old[i], &self.new[j]);
            let d = geom::dist(m.point(a.point), b.point);
            if d > tol {
                continue;
            }
            if let (Some(da), Some(db)) = (a.dir, b.dir) {
                let c = geom::dot(m.dir(da), db);
                if (if a.line { c.abs() } else { c }) < cos_tol {
                    continue;
                }
            }
            kept.push((i, j));
            if !hit_old[i] && !hit_new[j] {
                hit_old[i] = true;
                hit_new[j] = true;
                count += 1;
                sq += d * d;
            }
        }
        (count, (sq / count.max(1) as f64).sqrt(), kept)
    }

    /// The points *pairs* fix a motion by, old and new: each anchor's point and, for a face
    /// with a normal, the point *scale* along it.
    fn points(&self, pairs: impl IntoIterator<Item = Pair>, scale: f64) -> (Vec<V3>, Vec<V3>) {
        let mut from = Vec::new();
        let mut to = Vec::new();
        for (i, j) in pairs {
            let (a, b) = (&self.old[i], &self.new[j]);
            from.push(a.point);
            to.push(b.point);
            if let (Some(da), Some(db), false) = (a.dir, b.dir, a.line) {
                from.push(geom::add(a.point, geom::scale(da, scale)));
                to.push(geom::add(b.point, geom::scale(db, scale)));
            }
        }
        (from, to)
    }
}

/// A hypothesis kept as the best so far: its motion, inlier count and rms.
type Best = Option<(Motion, usize, f64)>;

/// The alignment of *old* onto *new*.
pub fn align(old: &Fingerprints, new: &Fingerprints, th: &Thresholds) -> (Motion, Alignment) {
    let (motion, alignment, _) = align_all(old, new, th);
    (motion, alignment)
}

/// [`align`], and where the alignment is symmetric, every other distinct motion that fits as
/// well (`Alignment::symmetric`): another symmetric copy of the same anchors, another of the
/// turns about the axis of a turned part, or, for an axisymmetric part whose turn is free, the
/// end-for-end flip when the anchors agree with it as well.
pub fn align_all(
    old: &Fingerprints,
    new: &Fingerprints,
    th: &Thresholds,
) -> (Motion, Alignment, Vec<Motion>) {
    let scale = old.scale.max(new.scale);
    let tol = th.position_tol * scale;
    let cos_tol = th.angle_tol.cos();

    let c = Candidates::of(old, new, th);
    let anchors = {
        let mut seen: Vec<usize> = c.pairs.iter().map(|p| p.0).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    };

    let (best, rivals) = search(&c, th, scale, tol, cos_tol);
    let accepted = |b: &Best| {
        b.filter(|(_, count, _)| {
            *count >= th.min_inliers && *count as f64 >= th.min_inlier_fraction * anchors as f64
        })
    };
    let (m, count, axisymmetric) = match (accepted(&best[0]), accepted(&best[1])) {
        (Some((m, c, _)), _) => (m, c, false),
        (None, Some((m, c, _))) => (m, c, true),
        (None, None) => {
            let most = best.iter().flatten().map(|b| b.1).max().unwrap_or(0);
            let (m, alignment) = unaligned(anchors, most);
            return (m, alignment, Vec::new());
        }
    };
    // Refine on every inlier, then recount.
    let (_, _, kept) = c.inliers(&m, tol, cos_tol);
    let (from, to) = c.points(kept, scale);
    let refined = horn(&from, &to).unwrap_or(m);
    let (count2, rms2, _) = c.inliers(&refined, tol, cos_tol);
    let (m, count, rms) = if count2 >= count {
        (refined, count2, rms2)
    } else {
        (m, count, c.inliers(&m, tol, cos_tol).1)
    };
    // The line every anchor lies on, in the new revision: through its two furthest points.
    let axis = if axisymmetric {
        line_through(&to)
    } else {
        None
    };
    // On a turned part, fix the turn about the axis from the off-axis repeats.
    if let Some([o, d]) = axis
        && let Some((turned, support, fold, others)) = resolve_turn(old, new, th, &m, o, d, scale)
    {
        return (
            turned,
            Alignment {
                found: true,
                rotation: turned.r,
                translation: turned.t,
                anchors,
                inliers: count + support,
                symmetric: fold > 1,
                axisymmetric: false,
                axis: None,
                fold: Some(fold),
                rms,
            },
            others,
        );
    }
    // Symmetric: another motion, distinctly different, that as many anchors agree with.
    let mut others: Vec<Motion> = Vec::new();
    let distinct = |others: &mut Vec<Motion>, r: Motion| {
        if !same_motion(&r, &m, tol, th.angle_tol, scale)
            && !others
                .iter()
                .any(|o| same_motion(o, &r, tol, th.angle_tol, scale))
        {
            others.push(r);
        }
    };
    // Only rivals distinct from the chosen motion and from each other (as `symmetric` always
    // asked) are refined, and at most a bounded number are tried.
    let mut tried = 0;
    let mut raw: Vec<Motion> = Vec::new();
    for (r, n) in rivals.iter().filter(|(_, n)| *n >= count) {
        if same_motion(r, &m, tol, th.angle_tol, scale)
            || raw
                .iter()
                .any(|o| same_motion(o, r, tol, th.angle_tol, scale))
        {
            continue;
        }
        raw.push(*r);
        if tried >= super::MAX_ALTERNATIVES * 4 {
            continue;
        }
        tried += 1;
        // The rival as the best fit to its own inliers, like the chosen motion.
        let (_, _, kept) = c.inliers(r, tol, cos_tol);
        let (from, to) = c.points(kept, scale);
        let refined = horn(&from, &to).filter(|h| c.inliers(h, tol, cos_tol).0 >= *n);
        distinct(&mut others, refined.unwrap_or(*r));
    }
    // Symmetric as before: another distinct motion fits as well, or the turn is free.
    let symmetric = axisymmetric || !raw.is_empty();
    if let (true, Some([o, d])) = (axisymmetric, axis) {
        // The turn about the axis is free, and the flip end for end about its middle is another
        // alignment when the anchors agree with it as well.
        let along: Vec<f64> = to.iter().map(|p| geom::dot(geom::sub(*p, o), d)).collect();
        let (lo, hi) = along
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), x| {
                (l.min(*x), h.max(*x))
            });
        let middle = geom::add(o, geom::scale(d, 0.5 * (lo + hi)));
        let side = geom::unit(geom::cross(
            d,
            if d[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            },
        ));
        if let Some(side) = side {
            let flip = rotation_about(side, std::f64::consts::PI);
            let turn = Motion {
                r: flip,
                t: [0.0; 3],
            };
            let mut r = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    r[i][j] = (0..3).map(|k| flip[i][k] * m.r[k][j]).sum();
                }
            }
            let flipped = Motion {
                r,
                t: geom::add(middle, turn.dir(geom::sub(m.t, middle))),
            };
            if c.inliers(&flipped, tol, cos_tol).0 >= count {
                distinct(&mut others, flipped);
            }
        }
    }
    (
        m,
        Alignment {
            found: true,
            rotation: m.r,
            translation: m.t,
            anchors,
            inliers: count,
            symmetric,
            axisymmetric,
            axis,
            fold: None,
            rms,
        },
        others,
    )
}

/// No alignment: the identity, with how many *inliers* the best hypothesis had.
fn unaligned(anchors: usize, inliers: usize) -> (Motion, Alignment) {
    (
        Motion::IDENTITY,
        Alignment {
            found: false,
            rotation: Motion::IDENTITY.r,
            translation: [0.0; 3],
            anchors,
            inliers,
            symmetric: false,
            axisymmetric: false,
            axis: None,
            fold: None,
            rms: 0.0,
        },
    )
}

/// The hypotheses tried: the best motion fixed by points spanning a plane, and the best fixed
/// only by points on a line (an axisymmetric part: its rotation about the line is not
/// determined); and every motion tried with its inlier count.
fn search(
    c: &Candidates,
    th: &Thresholds,
    scale: f64,
    tol: f64,
    cos_tol: f64,
) -> ([Best; 2], Vec<(Motion, usize)>) {
    let mut best: [Best; 2] = [None, None];
    let mut rivals: Vec<(Motion, usize)> = Vec::new();
    for_each_sample(c.pairs.len(), th.hypotheses, |sample| {
        let Some((m, line)) = hypothesis(c, sample, scale, tol) else {
            return;
        };
        let best = &mut best[usize::from(line)];
        let (count, rms, _) = c.inliers(&m, tol, cos_tol);
        rivals.push((m, count));
        if better(&m, count, rms, best, tol, scale) {
            *best = Some((m, count, rms));
        }
    });
    (best, rivals)
}

/// Each sample of *n* pairs to solve from: pairs of pairs, then triples, each all of them or
/// a deterministic sample of *hypotheses*.
fn for_each_sample(n: usize, hypotheses: usize, mut consider: impl FnMut(&[usize])) {
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |bound: usize| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % bound as u64) as usize
    };
    if n >= 2 {
        if n * (n - 1) / 2 <= hypotheses {
            for a in 0..n {
                for b in a + 1..n {
                    consider(&[a, b]);
                }
            }
        } else {
            for _ in 0..hypotheses {
                let (a, b) = (next(n), next(n));
                if a != b {
                    consider(&[a, b]);
                }
            }
        }
    }
    if n >= 3 {
        if n * (n - 1) * (n - 2) / 6 <= hypotheses {
            for a in 0..n {
                for b in a + 1..n {
                    for c in b + 1..n {
                        consider(&[a, b, c]);
                    }
                }
            }
        } else {
            for _ in 0..hypotheses {
                let (a, b, c) = (next(n), next(n), next(n));
                if a != b && b != c && a != c {
                    consider(&[a, b, c]);
                }
            }
        }
    }
}

/// The motion the pairs of *sample* fix, and whether their points lie on one line; `None` when
/// they are too close together to fix one or it leaves any of them out of place.
fn hypothesis(c: &Candidates, sample: &[usize], scale: f64, tol: f64) -> Option<(Motion, bool)> {
    let (from, to) = c.points(sample.iter().map(|&k| c.pairs[k]), scale);
    let line = !spread(&from, tol);
    if line && !far_apart(&from, tol) {
        return None;
    }
    let m = horn(&from, &to)?;
    if from
        .iter()
        .zip(&to)
        .any(|(p, q)| geom::dist(m.point(*p), *q) > tol)
    {
        return None;
    }
    Some((m, line))
}

/// Whether *m*, with *count* inliers at *rms*, beats *best*: more inliers, then a clearly
/// smaller rms, then nearer the identity.
fn better(m: &Motion, count: usize, rms: f64, best: &Best, tol: f64, scale: f64) -> bool {
    match best {
        None => true,
        Some((bm, bc, br)) => {
            count > *bc
                || (count == *bc
                    && (rms < br - 0.1 * tol
                        || ((rms - br).abs() <= 0.1 * tol
                            && distance_from_identity(m, scale)
                                < distance_from_identity(bm, scale) - 1e-9)))
        }
    }
}

/// The line through the two furthest apart of *points* (the first is a point on it, the second
/// its direction), from the first point's furthest.
fn line_through(points: &[V3]) -> Option<[V3; 2]> {
    let a = points[0];
    let far = points
        .iter()
        .copied()
        .max_by(|p, q| geom::dist(*p, a).total_cmp(&geom::dist(*q, a)))
        .unwrap_or(a);
    let b = points
        .iter()
        .copied()
        .max_by(|p, q| geom::dist(*p, far).total_cmp(&geom::dist(*q, far)))
        .unwrap_or(a);
    geom::unit(geom::sub(b, far)).map(|d| [far, d])
}

/// The rotation by *angle* about unit direction *d*.
fn rotation_about(d: V3, angle: f64) -> [[f64; 3]; 3] {
    let (s, c) = angle.sin_cos();
    let k = [[0.0, -d[2], d[1]], [d[2], 0.0, -d[0]], [-d[1], d[0], 0.0]];
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (1.0 - c) * d[i] * d[j] + s * k[i][j] + if i == j { c } else { 0.0 };
        }
    }
    r
}

/// The turn about an axisymmetric alignment's line (through *o* along *d*, in the new frame)
/// that the off-axis repeats agree on. Every pair of alike features (or faces) off the line votes
/// for the turn taking the old one, moved, onto the new one; the turns most old items vote for
/// win. A part with k-fold symmetry about the line has k winning turns, as alike as each other:
/// the one giving the motion nearest the identity is taken, as for any symmetric part, and k is
/// reported (`fold`). Returns the turned motion, how many old items agree with it, and k; `None`
/// when fewer than two off-axis items agree on any turn.
fn resolve_turn(
    old: &Fingerprints,
    new: &Fingerprints,
    th: &Thresholds,
    m: &Motion,
    o: V3,
    d: V3,
    scale: f64,
) -> Option<(Motion, usize, usize, Vec<Motion>)> {
    let tol = th.position_tol * scale;
    let cos_tol = th.angle_tol.cos();
    // (old key, old anchor, new anchor); old keys: features, then faces offset past them.
    let mut items: Vec<(usize, Anchor, Anchor)> = Vec::new();
    let feature_pairs = candidates(
        &old.features,
        &new.features,
        |a, b| same_feature(a, b, th.same_rel),
        th.turn_candidates,
    );
    for (i, j, _) in feature_pairs {
        if let (Some(a), Some(b)) = (
            feature_anchor(&old.features[i]),
            feature_anchor(&new.features[j]),
        ) {
            items.push((i, a, b));
        }
    }
    let face_pairs = candidates(
        &old.faces,
        &new.faces,
        |a, b| same_face(a, b, th.same_rel),
        th.turn_candidates,
    );
    for (i, j, _) in face_pairs {
        items.push((
            old.features.len() + i,
            face_anchor(&old.faces[i]),
            face_anchor(&new.faces[j]),
        ));
    }
    let split = |x: V3| {
        let v = geom::sub(x, o);
        let along = geom::dot(v, d);
        (along, geom::sub(v, geom::scale(d, along)))
    };
    // Votes: (turn, its tolerance, item).
    let mut votes: Vec<(f64, f64, usize)> = Vec::new();
    for (k, (_, a, b)) in items.iter().enumerate() {
        let (a1, ra) = split(m.point(a.point));
        let (a2, rb) = split(b.point);
        let r = geom::norm(rb);
        if r <= 10.0 * tol || (a1 - a2).abs() > tol || (geom::norm(ra) - r).abs() > tol {
            continue;
        }
        let angle = geom::dot(d, geom::cross(ra, rb)).atan2(geom::dot(ra, rb));
        votes.push((angle, tol / r + th.angle_tol, k));
    }
    votes.sort_by(|x, y| x.0.total_cmp(&y.0));
    // Clusters of agreeing turns, the one across ±π joined.
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for v in 0..votes.len() {
        match clusters.last_mut() {
            Some(c) if votes[v].0 - votes[v - 1].0 <= votes[v].1 + votes[v - 1].1 => c.push(v),
            _ => clusters.push(vec![v]),
        }
    }
    if clusters.len() > 1 {
        let (first, last) = (votes[0], votes[votes.len() - 1]);
        if first.0 + 2.0 * std::f64::consts::PI - last.0 <= first.1 + last.1 {
            let mut joined = clusters.pop().unwrap();
            joined.append(&mut clusters[0]);
            clusters[0] = joined;
        }
    }
    // Each cluster's turn (the vote furthest from the line, the most exact) and how many old
    // items agree with it, directions included.
    let turned = |angle: f64| {
        let rt = rotation_about(d, angle);
        let turn = Motion { r: rt, t: [0.0; 3] };
        let mut r = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                r[i][j] = (0..3).map(|k| rt[i][k] * m.r[k][j]).sum();
            }
        }
        Motion {
            r,
            t: geom::add(o, turn.dir(geom::sub(m.t, o))),
        }
    };
    let mut scored: Vec<(Motion, usize)> = Vec::new();
    for c in &clusters {
        let Some(&best) = c
            .iter()
            .min_by(|x, y| votes[**x].1.total_cmp(&votes[**y].1))
        else {
            continue;
        };
        let motion = turned(votes[best].0);
        let mut keys: Vec<usize> = c
            .iter()
            .map(|&v| votes[v].2)
            .filter(|&k| {
                let (_, a, b) = &items[k];
                geom::dist(motion.point(a.point), b.point) <= tol
                    && match (a.dir, b.dir) {
                        (Some(da), Some(db)) => {
                            let c = geom::dot(motion.dir(da), db);
                            (if a.line { c.abs() } else { c }) >= cos_tol
                        }
                        _ => true,
                    }
            })
            .map(|k| items[k].0)
            .collect();
        keys.sort_unstable();
        keys.dedup();
        scored.push((motion, keys.len()));
    }
    let most = scored.iter().map(|s| s.1).max()?;
    if most < 2 {
        return None;
    }
    let tied: Vec<&(Motion, usize)> = scored.iter().filter(|s| s.1 == most).collect();
    let chosen = tied.iter().min_by(|x, y| {
        distance_from_identity(&x.0, scale).total_cmp(&distance_from_identity(&y.0, scale))
    })?;
    let others = tied
        .iter()
        .filter(|t| !std::ptr::eq(**t, *chosen))
        .map(|t| t.0)
        .collect();
    Some((chosen.0, most, tied.len(), others))
}

fn distance_from_identity(m: &Motion, scale: f64) -> f64 {
    let trace = m.r[0][0] + m.r[1][1] + m.r[2][2];
    (3.0 - trace) + geom::norm(m.t) / scale
}

fn same_motion(a: &Motion, b: &Motion, tol: f64, angle_tol: f64, scale: f64) -> bool {
    let rotation = (0..3).all(|i| geom::dot(a.dir(b_axis(i)), b.dir(b_axis(i))) >= angle_tol.cos());
    // Compare where the two motions put points across the part, not their translations alone.
    rotation && geom::dist(a.t, b.t) <= tol + scale * angle_tol
}

fn b_axis(i: usize) -> V3 {
    let mut v = [0.0; 3];
    v[i] = 1.0;
    v
}

/// Whether two of the points are far enough apart to fix a line.
fn far_apart(points: &[V3], tol: f64) -> bool {
    points
        .iter()
        .any(|p| geom::dist(*p, points[0]) > 10.0 * tol)
}

/// Whether the points span a plane (not all on one line): a rotation is determined by them.
fn spread(points: &[V3], tol: f64) -> bool {
    if points.len() < 3 {
        return false;
    }
    let a = points[0];
    let Some(far) = points
        .iter()
        .max_by(|p, q| geom::dist(**p, a).total_cmp(&geom::dist(**q, a)))
    else {
        return false;
    };
    let Some(u) = geom::unit(geom::sub(*far, a)) else {
        return false;
    };
    if geom::dist(*far, a) <= 10.0 * tol {
        return false;
    }
    points.iter().any(|p| {
        let d = geom::sub(*p, a);
        geom::norm(geom::sub(d, geom::scale(u, geom::dot(d, u)))) > 10.0 * tol
    })
}

/// The proper rotation and translation that best take *from* onto *to* in least squares
/// (Horn 1987: the largest eigenvector of a 4×4 symmetric matrix is the rotation's quaternion).
pub fn horn(from: &[V3], to: &[V3]) -> Option<Motion> {
    if from.len() != to.len() || from.is_empty() {
        return None;
    }
    let n = from.len() as f64;
    let mean = |ps: &[V3]| geom::scale(ps.iter().fold([0.0; 3], |s, p| geom::add(s, *p)), 1.0 / n);
    let (ca, cb) = (mean(from), mean(to));
    let mut s = [[0.0; 3]; 3];
    for (p, q) in from.iter().zip(to) {
        let (a, b) = (geom::sub(*p, ca), geom::sub(*q, cb));
        for i in 0..3 {
            for j in 0..3 {
                s[i][j] += a[i] * b[j];
            }
        }
    }
    let [[sxx, sxy, sxz], [syx, syy, syz], [szx, szy, szz]] = s;
    let mut nm = [
        [sxx + syy + szz, syz - szy, szx - sxz, sxy - syx],
        [syz - szy, sxx - syy - szz, sxy + syx, szx + sxz],
        [szx - sxz, sxy + syx, -sxx + syy - szz, syz + szy],
        [sxy - syx, szx + sxz, syz + szy, -sxx - syy + szz],
    ];
    let (values, vectors) = jacobi4(&mut nm);
    let k = (0..4).max_by(|&a, &b| values[a].total_cmp(&values[b]))?;
    let q = [0, 1, 2, 3].map(|i| vectors[i][k]);
    let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm == 0.0 || norm.is_nan() {
        return None;
    }
    let [w, x, y, z] = q.map(|c| c / norm);
    let r = [
        [
            w * w + x * x - y * y - z * z,
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            w * w - x * x + y * y - z * z,
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            w * w - x * x - y * y + z * z,
        ],
    ];
    let m = Motion { r, t: [0.0; 3] };
    Some(Motion {
        r,
        t: geom::sub(cb, m.dir(ca)),
    })
}

/// Eigenvalues and eigenvectors (columns) of a symmetric 4×4 matrix, by cyclic Jacobi rotations.
fn jacobi4(a: &mut [[f64; 4]; 4]) -> ([f64; 4], [[f64; 4]; 4]) {
    let mut v = [[0.0; 4]; 4];
    for (i, row) in v.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for _ in 0..100 {
        let off: f64 = (0..4)
            .flat_map(|i| (0..4).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * a[i][j])
            .sum();
        let diag: f64 = (0..4).map(|i| a[i][i] * a[i][i]).sum();
        if off <= 1e-30 * diag.max(1e-300) {
            break;
        }
        for p in 0..4 {
            for q in p + 1..4 {
                if a[p][q].abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..4 {
                    let (akp, akq) = (a[k][p], a[k][q]);
                    a[k][p] = c * akp - s * akq;
                    a[k][q] = s * akp + c * akq;
                }
                for k in 0..4 {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
                for row in v.iter_mut() {
                    let (vp, vq) = (row[p], row[q]);
                    row[p] = c * vp - s * vq;
                    row[q] = s * vp + c * vq;
                }
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2], a[3][3]], v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horn_recovers_a_rotation_and_translation() {
        let r = [[0.0, -1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]];
        let m = Motion {
            r,
            t: [5.0, -2.0, 7.5],
        };
        let from = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, 4.0, 0.0],
            [1.0, 2.0, 3.0],
        ];
        let to: Vec<V3> = from.iter().map(|p| m.point(*p)).collect();
        let got = horn(&from, &to).unwrap();
        for i in 0..3 {
            for j in 0..3 {
                assert!((got.r[i][j] - r[i][j]).abs() < 1e-12, "{got:?}");
            }
            assert!((got.t[i] - m.t[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn collinear_points_do_not_fix_a_rotation() {
        assert!(!spread(&[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], 1e-6));
        assert!(spread(&[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], 1e-6));
    }
}
