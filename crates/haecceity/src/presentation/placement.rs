//! Feature-local model annotation placement and a deterministic stagger; no sheet layout.
use super::Callout;
use crate::geom::{self, Bounds, V3};
pub use crate::pmi::sites::{face_site, feature_site, surface_sites, text_height};

#[derive(Clone, Debug)]
pub struct Placed {
    pub anchor: V3,
    pub origin: V3,
    pub normal: V3,
    pub along: V3,
    pub lines: Vec<Vec<V3>>,
    /// Actual transform used to stroke local callout coordinates into the model.
    pub text_origin: V3,
    pub text_scale: f64,
}

#[derive(Clone, Copy, Default)]
struct Packing<'a> {
    columns: Option<&'a [f64]>,
    align_start: bool,
    ink_bounds: bool,
    feature_origin: bool,
    scale: Option<f64>,
}

#[derive(Clone)]
pub struct Layout {
    bounds: Bounds,
    axes: [usize; 3],
    height: f64,
    occupied: Vec<Bounds>,
    occupied_normals: Vec<Option<V3>>,
    view_local: bool,
    callouts: Vec<Vec<V3>>,
    notes_bottom: Option<f64>,
    kept_notes: Vec<Bounds>,
}
impl Layout {
    pub fn new(bounds: Bounds) -> Self {
        Self::with_unit(bounds, 1.)
    }
    /// Generated saved views contain their own annotation planes. Pack parallel
    /// planes together, without moving text to clear unrelated camera planes.
    #[cfg(feature = "presentation")]
    pub(crate) fn for_views(bounds: Bounds) -> Self {
        Self {
            view_local: true,
            ..Self::new(bounds)
        }
    }
    pub fn with_unit(bounds: Bounds, mm_per_unit: f64) -> Self {
        let mut axes = [0, 1, 2];
        axes.sort_by(|&a, &b| {
            (bounds.max[b] - bounds.min[b])
                .total_cmp(&(bounds.max[a] - bounds.min[a]))
                .then(a.cmp(&b))
        });
        let largest = if bounds.max_extent().is_finite() {
            bounds.max_extent().max(1e-6)
        } else {
            1.
        };
        Self {
            bounds,
            axes,
            height: text_height(largest * mm_per_unit) / mm_per_unit,
            occupied: Vec::new(),
            occupied_normals: Vec::new(),
            view_local: false,
            callouts: Vec::new(),
            notes_bottom: None,
            kept_notes: Vec::new(),
        }
    }
    pub fn text_height(&self) -> f64 {
        self.height
    }
    /// Reserve completed graphics (including leaders) before placing the notes block.
    pub fn reserve(&mut self, lines: &[Vec<V3>]) {
        let mut b = Bounds::empty();
        for &p in lines.iter().flatten() {
            b.add(p);
        }
        if b.max_extent().is_finite() {
            self.occupied.push(b);
            self.occupied_normals.push(None);
        }
    }
    /// Reserve completed annotation ink in both model space and projected views.
    /// Used when rebuilding only the feature stacks affected by new thread ink.
    #[cfg(feature = "presentation")]
    pub(crate) fn reserve_ink(&mut self, lines: &[Vec<V3>], normal: V3, along: V3) {
        let Some(&first) = lines.iter().flatten().next() else {
            return;
        };
        self.reserve(lines);
        let up = geom::cross(normal, along);
        let mut b = Bounds::empty();
        for &p in lines.iter().flatten() {
            b.add([geom::dot(p, along), geom::dot(p, up), 0.]);
        }
        let depth = geom::dot(first, normal);
        self.callouts.push(
            [
                (b.min[0], b.min[1]),
                (b.max[0], b.min[1]),
                (b.max[0], b.max[1]),
                (b.min[0], b.max[1]),
            ]
            .map(|(x, y)| {
                geom::add(
                    geom::scale(normal, depth),
                    geom::add(geom::scale(along, x), geom::scale(up, y)),
                )
            })
            .to_vec(),
        );
    }
    /// Keep appended notes clear of existing notes in the front plane.
    #[cfg(feature = "presentation")]
    pub(crate) fn reserve_notes(&mut self, lines: &[Vec<V3>]) {
        let mut bounds = Bounds::empty();
        for &p in lines.iter().flatten() {
            bounds.add(p);
        }
        if bounds.max_extent().is_finite() {
            self.kept_notes.push(bounds);
        }
    }
    /// ISO 1302's actual tip touches an interior face site; its legs rise normally.
    /// A small fixed list of interior stations avoids existing annotation ink.
    pub fn surface(&mut self, sites: &[(V3, V3)], c: &Callout) -> Option<Placed> {
        let tip = c.strokes.first()?[1];
        let scale = (self.height / 3.)
            .min(0.32 * self.bounds.max_extent().max(self.height) / c.size[0].max(1.));
        let local = c.polylines();
        let mut best = None;
        for &(anchor, outward) in sites {
            let outward = geom::unit(outward)?;
            let mut along = self.along(outward);
            let mut normal = geom::unit(geom::cross(along, outward))?;
            let k = geom::dominant_axis(normal);
            if (k == 1 && normal[k] > 0.) || (k != 1 && normal[k] < 0.) {
                normal = geom::scale(normal, -1.);
                along = geom::scale(along, -1.);
            }
            let at = |p: [f64; 2]| {
                geom::add(
                    anchor,
                    geom::add(
                        geom::scale(along, (p[0] - tip[0]) * scale),
                        geom::scale(outward, (p[1] - tip[1]) * scale),
                    ),
                )
                .map(|x| (x * 1e6).round() / 1e6 + 0.)
            };
            let lines: Vec<Vec<V3>> = local
                .iter()
                .map(|line| line.iter().copied().map(at).collect())
                .collect();
            let mut bounds = Bounds::empty();
            for &p in lines.iter().flatten() {
                bounds.add(p);
            }
            let collisions = self
                .occupied
                .iter()
                .zip(&self.occupied_normals)
                .filter(|(b, n)| {
                    (!self.view_local || n.is_none_or(|n| geom::dot(n, normal).abs() > 1. - 1e-8))
                        && boxes_overlap(bounds, **b, 0.2 * self.height)
                })
                .count();
            if best
                .as_ref()
                .is_none_or(|(score, _, _)| collisions < *score)
            {
                best = Some((
                    collisions,
                    Placed {
                        anchor: at(tip),
                        origin: at(tip),
                        normal,
                        along,
                        lines,
                        text_origin: at([0., 0.]),
                        text_scale: scale,
                    },
                    bounds,
                ));
            }
            if collisions == 0 {
                break;
            }
        }
        let (_, placed, bounds) = best?;
        self.occupied.push(bounds);
        self.occupied_normals.push(Some(placed.normal));
        Some(placed)
    }
    /// Front-plane notes column beside the part's lower-right X/Z corner.
    /// Ink starts two capital heights right of max X and below min Z.
    /// Subsequent blocks stack down with the same gap. Only part bounds and
    /// preceding notes determine this column; remote leaders cannot move it.
    pub fn notes(&mut self, c: &Callout) -> Placed {
        let scale = self.height / 3.;
        let local = c.polylines();
        let mut ink = Bounds::empty();
        for p in local.iter().flatten() {
            ink.add([p[0] * scale, 0., p[1] * scale]);
        }
        let gap = 2. * self.height;
        let mut top = self.notes_bottom.unwrap_or(self.bounds.min[2]) - gap;
        let left = self.bounds.max[0] + gap;
        let right = left + ink.max[0] - ink.min[0];
        let height = ink.max[2] - ink.min[2];
        loop {
            let next = self
                .kept_notes
                .iter()
                .filter(|b| {
                    left < b.max[0] + gap
                        && right + gap > b.min[0]
                        && top - height < b.max[2] + gap
                        && top + gap > b.min[2]
                })
                .map(|b| b.min[2] - gap)
                .fold(top, f64::min);
            if next == top {
                break;
            }
            top = next;
        }
        let origin = [
            self.bounds.max[0] + gap - ink.min[0],
            self.bounds.centre()[1],
            top - ink.max[2],
        ];
        if ink.max_extent().is_finite() {
            self.notes_bottom = Some(origin[2] + ink.min[2]);
        }
        let lines = local
            .iter()
            .map(|line| {
                line.iter()
                    .map(|p| {
                        [
                            origin[0] + p[0] * scale,
                            origin[1],
                            origin[2] + p[1] * scale,
                        ]
                    })
                    .collect()
            })
            .collect::<Vec<Vec<V3>>>();
        self.reserve(&lines);
        Placed {
            anchor: origin,
            origin,
            normal: [0., -1., 0.],
            along: [1., 0., 0.],
            lines,
            text_origin: origin,
            text_scale: scale,
        }
    }
    fn along(&self, normal: V3) -> V3 {
        self.axes
            .iter()
            .find_map(|&k| {
                let mut a = [0.; 3];
                a[k] = 1.;
                let p = geom::sub(a, geom::scale(normal, geom::dot(a, normal)));
                (geom::norm(p) > 0.5).then(|| geom::unit(p)).flatten()
            })
            .unwrap_or([1., 0., 0.])
    }
    /// A diameter/control frame seen end-on to its feature axis, beside its feature.
    pub fn place(&mut self, anchor: V3, axis: Option<V3>, c: &Callout) -> Placed {
        let mut view = axis.and_then(geom::unit).unwrap_or_else(|| {
            let mut n = [0.; 3];
            n[self.axes[2]] = 1.;
            n
        });
        let along = self.along(view);
        let mut outward = geom::unit(geom::cross(view, along)).unwrap_or([0., 1., 0.]);
        if geom::dot(geom::sub(anchor, self.bounds.centre()), outward) < 0. {
            outward = geom::scale(outward, -1.);
        }
        if geom::dominant_axis(view) == 1 {
            view = geom::scale(view, -1.);
        }
        self.draw(anchor, [along, outward, view], c, false, None)
    }
    /// The datum's plane contains its face normal, and the leader leaves the face normally.
    pub fn on_face(&mut self, anchor: V3, outward: V3, c: &Callout) -> Placed {
        self.face_callout(anchor, outward, c, None)
    }
    pub fn on_feature(&mut self, anchor: V3, outward: V3, c: &Callout, extent: f64) -> Placed {
        self.feature_callout(anchor, outward, c, extent, Packing::default())
    }
    /// A text label reserves its actual ink, without the notation cell's padding.
    #[cfg(feature = "presentation")]
    pub(crate) fn on_label(&mut self, anchor: V3, outward: V3, c: &Callout, extent: f64) -> Placed {
        self.feature_callout(
            anchor,
            outward,
            c,
            extent,
            Packing {
                ink_bounds: true,
                ..Packing::default()
            },
        )
    }
    /// Attach a control frame near its first cell rather than centring its whole
    /// potentially long datum-reference row on the feature.
    #[cfg(feature = "presentation")]
    pub(crate) fn on_control(
        &mut self,
        anchor: V3,
        outward: V3,
        c: &Callout,
        extent: f64,
    ) -> Placed {
        self.feature_callout(
            anchor,
            outward,
            c,
            extent,
            Packing {
                align_start: true,
                ..Packing::default()
            },
        )
    }
    /// Pack related ink together in one plane, with the size above its control
    /// frames. The feature lies in the first inter-callout gap, so neither row
    /// has to drift a full stack height away from a small bore.
    #[cfg(feature = "presentation")]
    pub(crate) fn stack(
        &mut self,
        anchor: V3,
        outward: V3,
        callouts: &[&Callout],
        extent: f64,
    ) -> Vec<Placed> {
        let gap = 0.2 * self.height;
        let mut combined = Callout::default();
        let mut transforms = vec![];
        let mut top = -gap / 2.;
        for (i, c) in callouts.iter().enumerate() {
            let mut ink = Bounds::empty();
            for p in c.polylines().iter().flatten() {
                ink.add([p[0], p[1], 0.]);
            }
            let scale = (self.height / 3.)
                .min(0.32 * self.bounds.max_extent().max(self.height) / c.size[0].max(1.));
            let y = if i == 0 {
                gap / 2. - ink.min[1] * scale
            } else {
                top - ink.max[1] * scale
            };
            let offset = [-c.runs[0].at[0] * scale, y];
            transforms.push((offset, scale, ink));
            for line in c.polylines() {
                combined
                    .strokes
                    .extend(
                        line.windows(2)
                            .map(|w| [w[0], w[1]])
                            .map(|edge: [[f64; 2]; 2]| {
                                edge.map(|p| [p[0] * scale + offset[0], p[1] * scale + offset[1]])
                            }),
                    );
            }
            if i > 0 {
                top = y + ink.min[1] * scale - gap;
            }
        }
        combined.runs.push(super::Text {
            text: String::new(),
            at: [0., 0.],
            size: 3.,
        });
        combined.cells.push(String::new());
        let p = self.feature_callout(
            anchor,
            outward,
            &combined,
            extent,
            Packing {
                ink_bounds: true,
                align_start: true,
                feature_origin: true,
                scale: Some(1.),
                ..Packing::default()
            },
        );
        let up = geom::cross(p.normal, p.along);
        callouts
            .iter()
            .zip(transforms)
            .map(|(c, (offset, scale, ink))| {
                let centre = geom::add(
                    p.text_origin,
                    geom::add(
                        geom::scale(p.along, offset[0] + ink.centre()[0] * scale),
                        geom::scale(up, offset[1] + ink.centre()[1] * scale),
                    ),
                );
                let size = [ink.max[0] - ink.min[0], ink.max[1] - ink.min[1]].map(|v| v * scale);
                stroke(
                    anchor,
                    [
                        p.along,
                        geom::scale(
                            up,
                            if geom::dot(geom::sub(centre, anchor), up) < 0. {
                                -1.
                            } else {
                                1.
                            },
                        ),
                        p.normal,
                    ],
                    c,
                    (scale, self.height),
                    centre,
                    size,
                    true,
                )
            })
            .collect()
    }
    fn feature_callout(
        &mut self,
        anchor: V3,
        outward: V3,
        c: &Callout,
        extent: f64,
        packing: Packing<'_>,
    ) -> Placed {
        let outward = geom::unit(outward).unwrap_or([0., 0., 1.]);
        let mut best = None;
        // Try both readable tangents around the feature normal. A small bore need
        // not inherit a view in which its frame collides with a nearby datum.
        for k in self.axes {
            let mut a = [0.; 3];
            a[k] = 1.;
            let Some(along) = geom::unit(geom::sub(a, geom::scale(outward, geom::dot(a, outward))))
            else {
                continue;
            };
            let Some(mut normal) = geom::unit(geom::cross(along, outward)) else {
                continue;
            };
            let axis = geom::dominant_axis(normal);
            if (axis == 1 && normal[axis] > 0.) || (axis != 1 && normal[axis] < 0.) {
                normal = geom::scale(normal, -1.);
            }
            let mut candidate = self.clone();
            let p = candidate.draw_columns(
                anchor,
                [along, outward, normal],
                c,
                (true, Some(extent)),
                packing,
            );
            let reach = if packing.align_start {
                let run = &c.runs[0];
                geom::dist(
                    anchor,
                    geom::add(
                        p.text_origin,
                        geom::add(
                            geom::scale(p.along, run.at[0] * p.text_scale),
                            geom::scale(geom::cross(p.normal, p.along), run.at[1] * p.text_scale),
                        ),
                    ),
                )
            } else {
                p.lines
                    .last()
                    .into_iter()
                    .flatten()
                    .map(|q| geom::dist(anchor, *q))
                    .fold(0., f64::max)
            };
            if best.as_ref().is_none_or(|(r, _, _)| reach < *r) {
                best = Some((reach, p, candidate));
            }
        }
        if let Some((_, p, layout)) = best {
            *self = layout;
            p
        } else {
            self.face_callout(anchor, outward, c, Some(extent))
        }
    }
    /// End-on frames on axial features use either radial side and a corner
    /// attachment, keeping their leaders short and their projected ink separated.
    #[cfg(feature = "presentation")]
    pub(crate) fn on_axis(&mut self, anchor: V3, axis: V3, c: &Callout, extent: f64) -> Placed {
        let mut best = None;
        // End-on is also a valid plane for an axial tolerance. It lets a frame
        // share the diameter's plane instead of crossing it in isometric views.
        let along = self.along(axis);
        let radial = geom::unit(geom::cross(axis, along)).unwrap_or([0., 1., 0.]);
        for outward in [radial, geom::scale(radial, -1.)] {
            let mut candidate = self.clone();
            let placed = candidate.draw_columns(
                anchor,
                [along, outward, axis],
                c,
                (true, Some(extent)),
                Packing {
                    columns: Some(&[-1., 1., -2., 2., -3., 3.]),
                    ..Packing::default()
                },
            );
            let reach = placed
                .lines
                .last()
                .into_iter()
                .flatten()
                .map(|p| geom::dist(anchor, *p))
                .fold(0., f64::max);
            if best.as_ref().is_none_or(|(r, _, _)| reach < *r) {
                best = Some((reach, placed, candidate));
            }
        }
        if let Some((_, placed, layout)) = best {
            *self = layout;
            placed
        } else {
            self.on_feature(anchor, axis, c, extent)
        }
    }
    fn face_callout(
        &mut self,
        anchor: V3,
        outward: V3,
        c: &Callout,
        extent: Option<f64>,
    ) -> Placed {
        let outward = geom::unit(outward).unwrap_or([0., 0., 1.]);
        let along = self.along(outward);
        let mut view = geom::unit(geom::cross(along, outward)).unwrap_or([0., -1., 0.]);
        let k = geom::dominant_axis(view);
        if (k == 1 && view[k] > 0.) || (k != 1 && view[k] < 0.) {
            view = geom::scale(view, -1.);
        }
        self.draw(anchor, [along, outward, view], c, true, extent)
    }
    fn draw(
        &mut self,
        anchor: V3,
        basis: [V3; 3],
        c: &Callout,
        face: bool,
        feature_extent: Option<f64>,
    ) -> Placed {
        self.draw_columns(anchor, basis, c, (face, feature_extent), Packing::default())
    }
    fn draw_columns(
        &mut self,
        anchor: V3,
        basis: [V3; 3],
        c: &Callout,
        placement: (bool, Option<f64>),
        packing: Packing<'_>,
    ) -> Placed {
        let (face, feature_extent) = placement;
        let [along, outward, normal] = basis;
        let h = self.height;
        let scale = packing.scale.unwrap_or_else(|| {
            (h / 3.).min(0.32 * self.bounds.max_extent().max(h) / c.size[0].max(1.))
        });
        let local = c.polylines();
        let mut ink = Bounds::empty();
        for p in local.iter().flatten() {
            ink.add([p[0], p[1], 0.]);
        }
        let size = if packing.ink_bounds && ink.max_extent().is_finite() {
            [ink.max[0] - ink.min[0], ink.max[1] - ink.min[1]]
        } else {
            c.size
        };
        let alignment = if packing.align_start {
            (ink.centre()[0] - c.runs[0].at[0]) * scale
        } else {
            0.
        };
        let width = size[0] * scale;
        let height = size[1] * scale;
        let up = geom::cross(normal, along);
        let mut chosen = anchor;
        let mut chosen_box = Bounds::empty();
        // Bare labels must participate too: their parallel planes may have
        // different model depths while sharing the same readable projection.
        let projected = self.view_local || !face || c.cells.len() > 1;
        let rectangle = |centre| {
            [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
                .map(|(x, y)| {
                    geom::add(
                        centre,
                        geom::add(
                            geom::scale(along, x * width / 2.),
                            geom::scale(up, y * height / 2.),
                        ),
                    )
                })
                .to_vec()
        };
        let columns: &[f64] = packing.columns.unwrap_or(if face && c.cells.len() == 1 {
            &[0.]
        } else {
            &[0., -1., 1., -2., 2., -3., 3.]
        });
        let gap = feature_extent.map_or(0.6 * h, |e| (0.6 * h).min(0.12 * e))
            + 2.4 * f64::from(c.leader_rings > 0) * h;
        let mut candidates: Vec<_> = (0..8)
            .flat_map(|row| {
                columns.iter().map(move |&column| {
                    let a = column * (width / 2. + feature_extent.map_or(2. * h, |_| gap));
                    let b = gap + row as f64 * (height + h);
                    (row, column, a.hypot(b))
                })
            })
            .collect();
        candidates.sort_by(|a, b| a.2.total_cmp(&b.2));
        // Saved-plane packing can keep growing outward: every row advances in
        // its readable projection. Preserve the bounded seven-view search,
        // where an outward step can be invisible in another camera.
        let extra_rows = if self.view_local { 8..usize::MAX } else { 8..8 };
        for (row, column, _) in candidates
            .into_iter()
            .chain(extra_rows.flat_map(|row| columns.iter().map(move |&column| (row, column, 0.))))
        {
            let centre = geom::add(
                anchor,
                geom::add(
                    if packing.feature_origin {
                        geom::add(
                            geom::scale(up, ink.centre()[1] * scale),
                            geom::scale(outward, row as f64 * (height + h)),
                        )
                    } else {
                        geom::scale(outward, gap + height / 2. + row as f64 * (height + h))
                    },
                    geom::scale(
                        along,
                        alignment + column * (width / 2. + feature_extent.map_or(2. * h, |_| gap)),
                    ),
                ),
            );
            let mut b = Bounds::empty();
            for x in [-width / 2., width / 2.] {
                for y in [-height / 2., height / 2.] {
                    b.add(geom::add(
                        centre,
                        geom::add(geom::scale(along, x), geom::scale(up, y)),
                    ));
                }
            }
            chosen = centre;
            chosen_box = b;
            if !self
                .occupied
                .iter()
                .zip(&self.occupied_normals)
                .any(|(r, n)| {
                    (!self.view_local || n.is_none_or(|n| geom::dot(n, normal).abs() > 1. - 1e-8))
                        && boxes_overlap(b, *r, 0.2 * h)
                })
                && (!projected
                    || self.callouts.iter().all(|other| {
                        let other_normal = geom::cross(
                            geom::sub(other[1], other[0]),
                            geom::sub(other[3], other[0]),
                        );
                        if self.view_local {
                            geom::unit(other_normal)
                                .is_none_or(|n| geom::dot(n, normal).abs() < 1. - 1e-8)
                                || polygon_clearance(&rectangle(centre), other, normal) >= 0.2 * h
                        } else {
                            super::linear::VIEWS.into_iter().all(|view| {
                                (geom::dot(normal, view).abs() < 1e-8
                                    && geom::dot(other_normal, view).abs() < 1e-8)
                                    || polygon_clearance(&rectangle(centre), other, view) >= 0.2 * h
                            })
                        }
                    }))
            {
                break;
            }
        }
        self.occupied.push(chosen_box);
        self.occupied_normals.push(Some(normal));
        if projected {
            self.callouts.push(rectangle(chosen));
        }
        stroke(
            anchor,
            basis,
            c,
            (scale, h),
            chosen,
            [width, height],
            feature_extent.is_some(),
        )
    }
}

/// Stroke a packed rectangle without changing the allocation. Shared by single
/// callouts and feature stacks, including scope rings and leader attachments.
fn stroke(
    anchor: V3,
    [along, outward, normal]: [V3; 3],
    c: &Callout,
    (scale, h): (f64, f64),
    chosen: V3,
    [width, height]: [f64; 2],
    feature: bool,
) -> Placed {
    let up = geom::cross(normal, along);
    let local = c.polylines();
    let mut ink = Bounds::empty();
    for q in local.iter().flatten() {
        ink.add([q[0], q[1], 0.]);
    }
    let mid = if ink.max_extent().is_finite() {
        ink.centre()
    } else {
        [0.; 3]
    };
    let round = |p: V3| p.map(|x| (x * 1e6).round() / 1e6 + 0.);
    let at = |p: [f64; 2]| {
        round(geom::add(
            chosen,
            geom::add(
                geom::scale(along, (p[0] - mid[0]) * scale),
                geom::scale(up, (p[1] - mid[1]) * scale),
            ),
        ))
    };
    let mut lines: Vec<_> = local
        .iter()
        .map(|l| l.iter().map(|&p| at(p)).collect())
        .collect();
    let mut end = geom::sub(chosen, geom::scale(outward, height / 2.));
    if feature {
        // Attach to the nearest point on the frame edge, not its midpoint.
        let a = geom::dot(geom::sub(anchor, end), along).clamp(-width / 2., width / 2.);
        end = geom::add(end, geom::scale(along, a));
    }
    lines.extend(leader_lines(
        anchor,
        end,
        [along, outward, up],
        c.leader_rings,
        (scale, h),
        round,
    ));
    Placed {
        anchor: round(anchor),
        origin: at([ink.min[0], ink.min[1]]),
        text_origin: at([0., 0.]),
        text_scale: scale,
        normal,
        along,
        lines,
    }
}

/// Include coordinate axes for edge-on projections as well as polygon edge normals.
fn polygon_clearance(a: &[V3], b: &[V3], view: V3) -> f64 {
    let n = geom::unit(view).unwrap_or(view);
    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
        .into_iter()
        .filter_map(|axis| geom::unit(geom::sub(axis, geom::scale(n, geom::dot(axis, n)))))
        .map(|axis| {
            let interval = |p: &[V3]| {
                p.iter()
                    .map(|p| geom::dot(*p, axis))
                    .fold([f64::INFINITY, f64::NEG_INFINITY], |r, t| {
                        [r[0].min(t), r[1].max(t)]
                    })
            };
            let x = interval(a);
            let y = interval(b);
            (x[0] - y[1]).max(y[0] - x[1])
        })
        .fold(super::linear::projected_clearance(a, b, view), f64::max)
}

/// Compare annotations in parallel planes by their projected boxes too.
pub(crate) fn boxes_overlap(a: Bounds, b: Bounds, padding: f64) -> bool {
    (0..3)
        .filter(|&k| (a.max[k] - a.min[k]).abs() > 1e-9 || (b.max[k] - b.min[k]).abs() > 1e-9)
        .all(|k| a.min[k] < b.max[k] + padding && a.max[k] + padding > b.min[k])
}

/// Draw supplied placement geometry without packing or coordinate rounding.
#[cfg(feature = "json-import")]
pub(crate) fn source(
    c: &Callout,
    [origin, normal, along]: [V3; 3],
    height: f64,
    leader: Option<V3>,
) -> Vec<Vec<V3>> {
    let up = geom::cross(normal, along);
    let scale = height / 3.;
    // TextPlacement origins are readable-run centres in generated companions,
    // rather than the padded callout rectangle's lower-left corner.
    let anchor = c.runs.first().map_or([0.; 2], |run| run.at);
    let at = |p: [f64; 2]| {
        geom::add(
            origin,
            geom::add(
                geom::scale(along, (p[0] - anchor[0]) * scale),
                geom::scale(up, (p[1] - anchor[1]) * scale),
            ),
        )
    };
    let mut lines: Vec<_> = c
        .polylines()
        .into_iter()
        .map(|l| l.into_iter().map(at).collect())
        .collect();
    if let Some(anchor) = leader {
        lines.extend(leader_lines(
            anchor,
            at([0., 0.]),
            [along, up, up],
            c.leader_rings,
            (scale, height),
            |p| p,
        ));
    }
    lines
}

fn leader_lines(
    anchor: V3,
    end: V3,
    [along, outward, up]: [V3; 3],
    rings: u8,
    (scale, h): (f64, f64),
    round: impl Fn(V3) -> V3,
) -> Vec<Vec<V3>> {
    let mut lines = Vec::new();
    let elbow = geom::add(
        anchor,
        geom::scale(outward, geom::dot(geom::sub(end, anchor), outward)),
    );
    for ring in 0..rings {
        let r = 0.7 * 2.7 * scale * if ring == 0 { 1. } else { 1.7 };
        let centre = geom::sub(elbow, geom::scale(outward, 2.4 * h));
        lines.push(
            (0..=32)
                .map(|i| {
                    let t = (i as f64 * 360. / 32.).to_radians();
                    round(geom::add(
                        centre,
                        geom::add(
                            geom::scale(along, r * t.cos()),
                            geom::scale(up, r * t.sin()),
                        ),
                    ))
                })
                .collect(),
        );
    }
    if rings > 0 {
        let ring_centre = geom::sub(elbow, geom::scale(outward, 2.4 * h));
        lines.push(vec![round(anchor), round(ring_centre), round(end)]);
    } else if geom::dist(elbow, end) > 1e-8 {
        lines.push(vec![round(anchor), round(elbow), round(end)]);
    } else {
        lines.push(vec![round(anchor), round(end)]);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bare_label_ink_clears_parallel_depths_and_more_than_eight_slots() {
        let mut layout = Layout::for_views(Bounds {
            min: [0., 0., 0.],
            max: [40., 4., 4.],
        });
        let c = super::super::label("SIZE");
        let mut prior = vec![];
        for depth in 0..10 {
            let p = layout.on_face([20., f64::from(depth) * 10., 0.], [0., 0., 1.], &c);
            let mut ink = Bounds::empty();
            for &q in p.lines[..p.lines.len() - 1].iter().flatten() {
                ink.add([q[0], 0., q[2]]);
            }
            for &other in &prior {
                assert!(
                    !boxes_overlap(ink, other, -2e-6),
                    "{ink:?} overlaps {other:?}"
                );
            }
            prior.push(ink);
        }
    }

    #[test]
    fn notes_column_stays_at_part_bounds_despite_remote_ink_and_stacks_without_overlap() {
        for mm_per_unit in [1., 25.4] {
            let bounds = Bounds {
                min: [0., -4., -4.],
                max: [37.6, 4., 4.],
            };
            let mut layout = Layout::with_unit(bounds, mm_per_unit);
            let c = super::super::label("GENERAL NOTES\nMATERIAL: Steel C45");
            let first = layout.clone().notes(&c);
            // Existing feature ink must not recursively expand the notes area.
            layout.reserve(&[vec![[-2000., -3000., -4000.], [5000., 6000., 7000.]]]);
            let placed = layout.notes(&c);
            assert_eq!(
                first.lines, placed.lines,
                "deterministic, part-relative placement"
            );
            assert_eq!(placed.normal, [0., -1., 0.]);
            assert_eq!(placed.along, [1., 0., 0.]);
            let first_box = *layout.occupied.last().unwrap();
            let gap = 2. * layout.text_height();
            assert!((first_box.min[0] - bounds.max[0] - gap).abs() < 1e-12);
            assert!((first_box.max[2] - bounds.min[2] + gap).abs() < 1e-12);
            assert_eq!(first_box.min[1], bounds.centre()[1]);
            layout.notes(&c);
            let second_box = *layout.occupied.last().unwrap();
            assert!((second_box.min[0] - first_box.min[0]).abs() < 1e-12);
            assert!((first_box.min[2] - second_box.max[2] - gap).abs() < 1e-12);
        }
    }
}
