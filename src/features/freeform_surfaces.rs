//! Native B-spline supports and their bounded relationships (`quiddity.freeform_surfaces`).
//!
//! The support is the untrimmed surface the B-rep carries, as OpenCascade holds it; construction
//! history is not recovered from a generic B-spline. Each record claims its one source face.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::Context;
use super::evidence::{self, EvidenceError, Occurrence, common_valid_solid};
use super::policy::length_tol;
use super::thin_walls::{self, ThinWallBody};
use crate::kernel::brep::{Arc, Part};
use crate::kernel::geom::{Surface, V3};
use crate::kernel::nurbs::NurbsSurface;
use crate::kernel::py;

/// `Precision::Confusion()`: how close OpenCascade's reader needs a B-spline's first and last
/// pole rows to be before it makes the surface periodic (probed: 1e-7 mm closes it, 1.02e-7
/// does not, whatever the file's uncertainty; weights are not compared).
const CONFUSION: f64 = 1e-7;

/// `BSplineSurfaceSupport`: the full native tensor-product support, before trimming.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct BSplineSurfaceSupport {
    pub u_degree: usize,
    pub v_degree: usize,
    pub poles: Vec<Vec<V3>>,
    pub weights: Vec<Vec<f64>>,
    pub u_knots: Vec<f64>,
    pub v_knots: Vec<f64>,
    pub u_multiplicities: Vec<usize>,
    pub v_multiplicities: Vec<usize>,
    pub u_periodic: bool,
    pub v_periodic: bool,
}

/// `SurfaceContinuityLink`: a direct shared-edge relation to another native freeform face.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct SurfaceContinuityLink {
    pub other_face: usize,
    /// `same_support` or `G1`.
    pub kind: &'static str,
}

/// `FreeformSurface`: one native B-spline face and only the relations its geometry establishes.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FreeformSurface {
    pub face: usize,
    pub support_kind: &'static str,
    pub support: BSplineSurfaceSupport,
    pub construction_kind: Option<&'static str>,
    pub construction_axis: Option<&'static str>,
    pub construction_vector: Option<V3>,
    pub continuity_group: Vec<usize>,
    pub continuity_links: Vec<SurfaceContinuityLink>,
    pub offset_partner: Option<usize>,
    pub offset_distance: Option<f64>,
    pub offset_basis: Option<&'static str>,
}

/// The face's native B-spline surface (`Geom_BSplineSurface`): Bézier, extrusion and revolution
/// supports are other OpenCascade types.
fn native(part: &Part, face: usize) -> Option<&NurbsSurface> {
    match &part.faces[face].surface {
        Surface::Freeform {
            kind: "BSPLINE",
            surface,
        } => Some(surface),
        _ => None,
    }
}

/// Distinct knots and their multiplicities, from a flat knot vector.
fn distinct(flat: &[f64]) -> (Vec<f64>, Vec<usize>) {
    let (mut knots, mut mults) = (Vec::new(), Vec::<usize>::new());
    for &k in flat {
        if knots.last() == Some(&k) {
            *mults.last_mut().expect("a knot has a multiplicity") += 1;
        } else {
            knots.push(k);
            mults.push(1);
        }
    }
    (knots, mults)
}

/// One parameter direction's data, its poles and weights as rows along it.
struct Direction {
    degree: usize,
    knots: Vec<f64>,
    mults: Vec<usize>,
    poles: Vec<Vec<V3>>,
    weights: Vec<Vec<f64>>,
    periodic: bool,
}

impl Direction {
    /// OpenCascade's reader makes a direction periodic when its first and last pole rows
    /// coincide, by `Geom_BSplineSurface::SetUPeriodic`: the knots between
    /// `BSplCLib::FirstUKnotIndex` and `LastUKnotIndex`, end multiplicities
    /// min(degree, max of the two), and the leading `BSplCLib::NbPoles` rows.
    fn read_periodic(mut self) -> Self {
        let closed = match (self.poles.first(), self.poles.last()) {
            (Some(first), Some(last)) if self.poles.len() > 1 => first
                .iter()
                .zip(last)
                .all(|(a, b)| py::dist(a, b) <= CONFUSION),
            _ => false,
        };
        if !closed {
            return self;
        }
        let n = self.mults.len();
        let (mut first, mut sigma) = (0, self.mults[0]);
        while sigma <= self.degree && first + 1 < n {
            first += 1;
            sigma += self.mults[first];
        }
        let (mut last, mut sigma) = (n - 1, self.mults[n - 1]);
        while sigma <= self.degree && last > 0 {
            last -= 1;
            sigma += self.mults[last];
        }
        if last <= first {
            return self;
        }
        let mut mults = self.mults[first..=last].to_vec();
        let end = self.degree.min(mults[0].max(mults[mults.len() - 1]));
        let k = mults.len();
        (mults[0], mults[k - 1]) = (end, end);
        let count: usize = mults[..k - 1].iter().sum();
        if count > self.poles.len() {
            return self;
        }
        self.knots = self.knots[first..=last].to_vec();
        self.mults = mults;
        self.poles.truncate(count);
        self.weights.truncate(count);
        self.periodic = true;
        self
    }
}

fn transpose<T: Copy>(rows: &[Vec<T>]) -> Vec<Vec<T>> {
    let width = rows.first().map_or(0, Vec::len);
    (0..width)
        .map(|j| rows.iter().map(|row| row[j]).collect())
        .collect()
}

/// `_support`, as OpenCascade's reader holds the surface.
fn support(surface: &NurbsSurface) -> BSplineSurfaceSupport {
    let (u_knots, u_mults) = distinct(&surface.knots_u);
    let u = Direction {
        degree: surface.degree_u,
        knots: u_knots,
        mults: u_mults,
        poles: surface.control_points.clone(),
        weights: surface.weights.clone(),
        periodic: false,
    }
    .read_periodic();
    let (v_knots, v_mults) = distinct(&surface.knots_v);
    let v = Direction {
        degree: surface.degree_v,
        knots: v_knots,
        mults: v_mults,
        poles: transpose(&u.poles),
        weights: transpose(&u.weights),
        periodic: false,
    }
    .read_periodic();
    BSplineSurfaceSupport {
        u_degree: u.degree,
        v_degree: v.degree,
        poles: transpose(&v.poles),
        weights: transpose(&v.weights),
        u_knots: u.knots,
        v_knots: v.knots,
        u_multiplicities: u.mults,
        v_multiplicities: v.mults,
        u_periodic: u.periodic,
        v_periodic: v.periodic,
    }
}

type Construction = (Option<&'static str>, Option<&'static str>, Option<V3>);

/// `_construction`: prove only a two-section ruled sweep or its constant-vector extrusion.
fn construction(support: &BSplineSurfaceSupport) -> Construction {
    for axis in ["u", "v"] {
        let (first, last, first_weights, last_weights): (Vec<V3>, Vec<V3>, Vec<f64>, Vec<f64>) =
            if axis == "u" {
                if support.u_periodic || support.u_degree != 1 || support.poles.len() != 2 {
                    continue;
                }
                (
                    support.poles[0].clone(),
                    support.poles[1].clone(),
                    support.weights[0].clone(),
                    support.weights[1].clone(),
                )
            } else {
                if support.v_periodic || support.v_degree != 1 || support.poles[0].len() != 2 {
                    continue;
                }
                let column = |rows: &[Vec<V3>], j: usize| rows.iter().map(|r| r[j]).collect();
                let weights = |j: usize| support.weights.iter().map(|r| r[j]).collect();
                (
                    column(&support.poles, 0),
                    column(&support.poles, 1),
                    weights(0),
                    weights(1),
                )
            };
        let offset = |start: &V3, end: &V3| -> V3 { std::array::from_fn(|i| end[i] - start[i]) };
        let vector = offset(&first[0], &last[0]);
        let tolerance = length_tol(py::dist(&first[0], &last[0]), 1e-9);
        if py::dist(&[0.0; 3], &vector) > tolerance
            && first
                .iter()
                .zip(&last)
                .all(|(start, end)| py::dist(&vector, &offset(start, end)) <= tolerance)
            && first_weights == last_weights
        {
            return (Some("linear_extrusion"), Some(axis), Some(vector));
        }
        return (Some("ruled"), Some(axis), None);
    }
    (None, None, None)
}

/// `_records`: every native B-spline face of a valid solid, with its continuity links and
/// group and its thin-wall offset partner.
fn records(part: &Part, walls: &[ThinWallBody]) -> Vec<FreeformSurface> {
    let surfaces: BTreeMap<usize, BSplineSurfaceSupport> = (0..part.faces.len())
        .filter(|&f| common_valid_solid(part, &[f]).is_some())
        .filter_map(|f| native(part, f).map(|s| (f, support(s))))
        .collect();
    let mut links: BTreeMap<usize, Vec<SurfaceContinuityLink>> =
        surfaces.keys().map(|&f| (f, Vec::new())).collect();
    for (&index, own) in &surfaces {
        for other in part.neighbours(index) {
            if other <= index {
                continue;
            }
            let Some(theirs) = surfaces.get(&other) else {
                continue;
            };
            // Python compares OpenCascade's surface handles; equal supports are the same
            // surface however the file shares it. Verdict rust-correct, with no corpus or
            // captured link to list it against (neither has a `same_support` link): through
            // STEP, Python never reports one, since OpenCascade's reader gives every face its
            // own handle even where faces share one surface entity. Probed on a NURBS box
            // whose top face is split by an edge (OCP, OpenCascade 7.9.3): the two halves'
            // supports are equal value for value, and Python links them `G1`, whether the
            // file writes the surface twice or once; the port links them `same_support`.
            let kind = if own == theirs {
                "same_support"
            } else if part.arc(index, other) == Some(Arc::Smooth) {
                "G1"
            } else {
                continue;
            };
            for (from, to) in [(index, other), (other, index)] {
                links
                    .get_mut(&from)
                    .expect("a native face")
                    .push(SurfaceContinuityLink {
                        other_face: to,
                        kind,
                    });
            }
        }
    }

    let mut remaining: BTreeSet<usize> = surfaces.keys().copied().collect();
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    while let Some(seed) = remaining.pop_first() {
        let mut component = BTreeSet::from([seed]);
        let mut pending = vec![seed];
        while let Some(face) = pending.pop() {
            for link in &links[&face] {
                if remaining.remove(&link.other_face) {
                    component.insert(link.other_face);
                    pending.push(link.other_face);
                }
            }
        }
        let group: Vec<usize> = component.iter().copied().collect();
        groups.extend(component.into_iter().map(|f| (f, group.clone())));
    }

    let mut offsets: BTreeMap<usize, (usize, f64)> = BTreeMap::new();
    for pair in walls.iter().flat_map(|w| &w.face_pairs) {
        if let Some(thickness) = pair.thickness.filter(|&t| t != 0.0)
            && surfaces.contains_key(&pair.first_face)
            && surfaces.contains_key(&pair.second_face)
        {
            offsets.insert(pair.first_face, (pair.second_face, thickness));
            offsets.insert(pair.second_face, (pair.first_face, thickness));
        }
    }

    surfaces
        .into_iter()
        .map(|(index, support)| {
            let (construction_kind, construction_axis, construction_vector) =
                construction(&support);
            let mut own_links = links.remove(&index).expect("a native face");
            own_links.sort_by_key(|link| link.other_face);
            let offset = offsets.get(&index);
            FreeformSurface {
                face: index,
                support_kind: "bspline",
                support,
                construction_kind,
                construction_axis,
                construction_vector,
                continuity_group: groups[&index].clone(),
                continuity_links: own_links,
                offset_partner: offset.map(|o| o.0),
                offset_distance: offset.map(|o| o.1),
                offset_basis: offset.map(|_| "reciprocal_material_rays"),
            }
        })
        .collect()
}

internal_entry! {
/// `recognise_freeform_surfaces`: the native B-spline supports on faces of valid solids.
pub fn recognise_freeform_surfaces(part: &Part) -> Vec<FreeformSurface> {
    if !(0..part.faces.len()).any(|f| native(part, f).is_some()) {
        return Vec::new();
    }
    let ctx = Context::new(part);
    let walls = super::records(thin_walls::discover(&ctx));
    super::records(discover(&ctx, &walls))
}
}

internal_entry! {
/// `_discover`: each record defined by its own face, offsets read from the run's thin walls.
pub fn discover(ctx: &Context<'_>, walls: &[ThinWallBody]) -> Vec<Occurrence<FreeformSurface>> {
    records(ctx.part, walls)
        .into_iter()
        .map(|record| Occurrence {
            defining: vec![record.face],
            context: Vec::new(),
            record,
        })
        .collect()
}
}

internal_entry! {
/// The evidence path: every record's face on one valid solid, or the run refused.
pub fn discover_verified(
    ctx: &Context<'_>,
    walls: &[ThinWallBody],
) -> Result<Vec<Occurrence<FreeformSurface>>, EvidenceError> {
    evidence::verified(ctx.part, discover(ctx, walls))
}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seven rows of two poles, the last repeating the first unless *wrap* is false.
    fn direction(knots: &[f64], mults: &[usize], wrap: bool) -> Direction {
        let mut poles: Vec<Vec<V3>> = (0..7)
            .map(|i| vec![[i as f64, 0.0, 0.0], [i as f64, 0.0, 12.0]])
            .collect();
        poles[6] = if wrap {
            poles[0].clone()
        } else {
            vec![[9.0, 0.5, 0.0], [9.0, 0.5, 12.0]]
        };
        Direction {
            degree: 2,
            knots: knots.to_vec(),
            mults: mults.to_vec(),
            poles,
            weights: vec![vec![1.0; 2]; 7],
            periodic: false,
        }
    }

    /// What OpenCascade 7.9.3's STEP reader made of each form of a degree-2 surface (probed
    /// through OCP on edited copies of a captured NURBS cylinder).
    #[test]
    fn closed_rows_are_read_periodic_as_opencascade_reads_them() {
        let k = std::f64::consts::TAU / 3.0;
        let read = |knots: &[f64], mults: &[usize], wrap: bool| {
            let d = direction(knots, mults, wrap).read_periodic();
            (d.periodic, d.knots, d.mults, d.poles.len())
        };
        let periodic = vec![0.0, k, 2.0 * k, 3.0 * k];
        // Clamped, and the unclamped form OpenCascade writes: both periodic, six rows.
        assert_eq!(
            read(&periodic, &[3, 2, 2, 3], true),
            (true, periodic.clone(), vec![2, 2, 2, 2], 6)
        );
        let unclamped = [-k, 0.0, k, 2.0 * k, 3.0 * k, 4.0 * k];
        assert_eq!(
            read(&unclamped, &[1, 2, 2, 2, 2, 1], true),
            (true, periodic, vec![2, 2, 2, 2], 6)
        );
        // Uneven end multiplicities: the knots from the first to the last that spans.
        assert_eq!(
            read(&unclamped, &[2, 2, 2, 2, 1, 1], true),
            (true, vec![0.0, k, 2.0 * k], vec![2, 2, 2], 4)
        );
        // Rows that do not close stay as written.
        assert_eq!(
            read(&unclamped, &[1, 2, 2, 2, 2, 1], false),
            (false, unclamped.to_vec(), vec![1, 2, 2, 2, 2, 1], 7)
        );
    }

    /// The reader applies the same rule in v: each form above, written through STEPControl
    /// once closed along u and once along v, read back with the same periodicity, knots,
    /// multiplicities and pole count in the closed direction (OpenCascade 7.9.3 through OCP).
    #[test]
    fn closed_columns_are_read_periodic_in_v_as_rows_are_in_u() {
        let k = std::f64::consts::TAU / 3.0;
        // The unclamped form above, as a flat knot vector.
        let knots = [-k, 0.0, k, 2.0 * k, 3.0 * k, 4.0 * k];
        let flat: Vec<f64> = knots
            .iter()
            .zip([1, 2, 2, 2, 2, 1])
            .flat_map(|(&knot, m)| std::iter::repeat_n(knot, m))
            .collect();
        let rows = direction(&[], &[], true).poles;
        let along_u = NurbsSurface::new(
            2,
            1,
            rows.clone(),
            vec![vec![1.0; 2]; 7],
            flat.clone(),
            vec![0.0, 0.0, 12.0, 12.0],
        )
        .expect("a well-formed surface");
        let along_v = NurbsSurface::new(
            1,
            2,
            transpose(&rows),
            vec![vec![1.0; 7]; 2],
            vec![0.0, 0.0, 12.0, 12.0],
            flat,
        )
        .expect("a well-formed surface");
        let (u, v) = (support(&along_u), support(&along_v));
        assert!(u.u_periodic && !u.v_periodic);
        assert!(v.v_periodic && !v.u_periodic);
        assert_eq!(
            (u.u_knots, u.u_multiplicities, u.poles.len()),
            (v.v_knots, v.v_multiplicities, v.poles[0].len())
        );
        assert_eq!(u.poles, transpose(&v.poles));
    }
}
