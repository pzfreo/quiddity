use super::*;
use crate::features::{hole_patterns::HolePattern, holes::Bottom};
use crate::kernel::geom::Surface;

pub(super) fn face_id(role: &str, faces: &[usize]) -> String {
    let mut faces = faces.to_vec();
    faces.sort_unstable();
    faces.dedup();
    format!(
        "{role}:{}",
        faces
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".")
    )
}

impl Input {
    /// Recognise once in the inferred part frame, preserving caller face numbers.
    /// Refuses absent/invalid frames instead of silently interpreting world axes as part axes.
    /// Datum faces use caller face indices. Their planes are measured in the inferred frame.
    pub fn from_recognition(
        part: &crate::Part,
        datums: Vec<Datum>,
        policy: Policy,
    ) -> Result<Self, String> {
        let inferred = crate::frames::infer_part_frame(part)
            .map_err(|r| format!("dimensioning frame: {}", r.as_str()))?;
        let frame = super::levels::planning_frame(inferred);
        let local = part
            .transformed(&frame.normalisation())
            .map_err(|e| format!("dimensioning frame transform: {e:?}"))?;
        // Keep the accepted run's constituent faces: pocket floors are consulted evidence,
        // not defining walls, so the public record projection alone cannot assign ownership.
        let (found, constituents, cylinders) =
            crate::features::take_inventory(&local, |ctx, inventory| {
                let constituents: std::collections::BTreeMap<_, _> = inventory
                    .physical
                    .defining
                    .iter()
                    .flat_map(|(family, records)| {
                        records
                            .iter()
                            .zip(&inventory.constituent[family])
                            .map(|(faces, all)| (face_id(family, faces), all.clone()))
                    })
                    .collect();
                (
                    crate::features::recognise_in(ctx, inventory),
                    constituents,
                    ctx.cylinders().to_vec(),
                )
            })
            .map_err(|e| format!("dimensioning recognition: {e}"))?;
        let bounds = local.bounds();
        let bounds = [bounds.min, bounds.max];
        let holes: Vec<_> = found
            .holes
            .iter()
            .zip(found.defining.get("holes").map_or(&[][..], Vec::as_slice))
            .map(|(h, faces)| {
                let depths = super::depth::Depths::new(&local, h, faces, &cylinders);
                let mut details = Vec::new();
                for (role, r) in [
                    ("counterbore".to_owned(), h.cbore.as_ref()),
                    ("spotface".to_owned(), h.spotface.as_ref()),
                ]
                .into_iter()
                .chain(
                    h.outer_steps
                        .iter()
                        .enumerate()
                        .map(|(i, r)| (format!("outer_steps.{i}"), Some(r))),
                ) {
                    if let Some(r) = r {
                        details.push(Size {
                            role: format!("{role}.diameter"),
                            nominal: r.diameter,
                            kind: SizeKind::Diameter,
                        });
                        details.push(Size {
                            role: format!("{role}.depth"),
                            nominal: depths.enlargement(r.diameter, r.depth),
                            kind: SizeKind::Length,
                        });
                    }
                }
                if let Some(c) = &h.csink {
                    details.push(Size {
                        role: "countersink.diameter".into(),
                        nominal: c.major_diameter,
                        kind: SizeKind::Diameter,
                    });
                    details.push(Size {
                        role: "countersink.angle".into(),
                        nominal: c.included_angle,
                        kind: SizeKind::Angle,
                    });
                }
                Hole {
                    support: Support {
                        feature: face_id("hole", faces),
                        faces: faces.clone(),
                        body: faces.first().and_then(|&f| local.faces[f].solid),
                        at: *h.location,
                        plane: false,
                    },
                    direction: h.axis,
                    diameter: h.diameter,
                    depth: (h.bottom != Bottom::Through).then(|| depths.bore()),
                    traversal: (h.bottom == Bottom::Through).then_some(h.depth),
                    bottom: h.bottom,
                    details,
                }
            })
            .collect();
        let patterns = found
            .hole_patterns
            .iter()
            .filter_map(|p| {
                let (members, arrangement) = match p {
                    HolePattern::RectGrid { holes, .. }
                    | HolePattern::RectangularHoleSet { holes, .. } => (holes, Arrangement::Grid),
                    HolePattern::LinearArray { holes, .. } => (holes, Arrangement::Line),
                    HolePattern::BoltCircle {
                        holes,
                        center,
                        diameter,
                    } => (
                        holes,
                        Arrangement::BoltCircle {
                            centre: *center,
                            diameter: *diameter,
                        },
                    ),
                };
                let members = members
                    .iter()
                    .map(|m| {
                        found
                            .holes
                            .iter()
                            .position(|h| h == m)
                            .map(|i| holes[i].support.feature.clone())
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Pattern {
                    members,
                    arrangement,
                })
            })
            .collect();
        let mut planes: Vec<Plane> = Vec::new();
        for (i, face) in local.faces.iter().enumerate() {
            let Surface::Plane { frame: plane } = &face.surface else {
                continue;
            };
            let Some(axis) = super::rules::principal(plane.z) else {
                continue;
            };
            let at = plane.origin[axis];
            let body = face.solid.unwrap_or(usize::MAX);
            if let Some(p) = planes.iter_mut().find(|p| {
                p.body == body && p.axis == axis && (p.support.at[axis] - at).abs() < 1e-6
            }) {
                p.support.faces.push(i);
            } else {
                let mut point = [0.; 3];
                point[axis] = at;
                planes.push(Plane {
                    support: Support {
                        feature: String::new(),
                        faces: vec![i],
                        body: face.solid,
                        at: point,
                        plane: true,
                    },
                    axis,
                    body,
                });
            }
        }
        for p in &mut planes {
            p.support.feature = face_id("plane", &p.support.faces);
        }
        let (steps, cartesian_envelope, deferred) =
            super::levels::recognise(&local, &found, &planes, &constituents);
        let round_axes = found.bosses.iter().map(|b| (b.location, b.axis)).collect();
        Ok(Self {
            frame,
            bounds,
            holes,
            patterns,
            planes,
            steps,
            cartesian_envelope,
            deferred,
            datums,
            round_axes,
            specified: vec![],
            policy,
        })
    }
}
