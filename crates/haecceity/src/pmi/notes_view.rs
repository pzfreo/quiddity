//! CAx-IF PMI 4.1 §9.4: the notes plane, viewed square-on in the part's units.
use super::*;

#[derive(Default)]
pub(super) struct Frame {
    basis: Option<[[f64; 3]; 3]>,
    points: Vec<[f64; 3]>,
    several_planes: bool,
    several_orientations: bool,
    kept_planes: Vec<u64>,
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
impl Frame {
    pub(super) fn has_notes(&self) -> bool {
        self.basis.is_some()
    }
    pub(super) fn one_plane(&self) -> bool {
        !self.several_planes && !self.several_orientations
    }
    pub(super) fn new() -> Self {
        Self::default()
    }
    pub(super) fn include(&mut self, basis: [[f64; 3]; 3], lines: &[Vec<[f64; 3]>]) {
        if let Some([origin, normal, along]) = self.basis {
            if dot(normal, basis[1]).abs() < 1. - 1e-8
                || dot(sub(basis[0], origin), normal).abs() > 1e-6
            {
                self.several_planes = true;
            } else if dot(along, basis[2]) < 1. - 1e-8
                || dot(cross(normal, along), cross(basis[1], basis[2])) < 1. - 1e-8
            {
                self.several_orientations = true;
            }
        } else {
            self.basis = Some(basis);
        }
        self.points.extend(lines.iter().flatten().copied());
    }
    pub(super) fn include_point(&mut self, p: [f64; 3]) {
        self.points.push(p);
    }
    pub(super) fn include_bounds(&mut self, min: [f64; 3], max: [f64; 3]) {
        for mask in 0..8 {
            self.points.push(std::array::from_fn(|i| {
                if mask & (1 << i) == 0 { min[i] } else { max[i] }
            }));
        }
    }
    pub(super) fn include_kept(
        &mut self,
        doc: &Document,
        removed: &BTreeSet<u64>,
        context: u64,
        shape: u64,
        product: u64,
    ) {
        // Add never moves or rewrites foreign presentation. Its notes participate
        // in the one-plane decision, even though they are outside the new view.
        for plane in doc.ids().filter(|id| !removed.contains(id)) {
            if !is_a(doc, plane, "annotation_plane") {
                continue;
            }
            if !doc
                .referrers(plane)
                .iter()
                .any(|id| attr_ref(doc, *id, "representation", "context_of_items") == Some(context))
            {
                continue;
            }
            let callouts = attr_refs(doc, plane, "annotation_plane", "elements");
            let members: Vec<_> = callouts
                .iter()
                .copied()
                .chain(
                    callouts
                        .iter()
                        .flat_map(|id| attr_refs(doc, *id, "draughting_callout", "contents")),
                )
                .collect();
            let definitions: Vec<_> = members
                .iter()
                .flat_map(|id| doc.referrers(*id))
                .filter_map(|id| {
                    attr_ref(
                        doc,
                        *id,
                        "item_identified_representation_usage",
                        "definition",
                    )
                })
                .collect();
            if !definitions.is_empty()
                && !definitions.iter().any(|id| {
                    attr_ref(doc, *id, "property_definition", "definition")
                        .is_some_and(|owner| owner == shape || owner == product)
                })
            {
                continue;
            }
            let named = attr_str(doc, plane, "representation_item", "name").as_deref()
                == Some("General notes");
            let note_ink = members.iter().any(|id| {
                attr_ref(doc, *id, "styled_item", "item").is_some_and(|item| {
                    attr_str(doc, item, "representation_item", "name").as_deref() == Some("note")
                })
            });
            if !named && !note_ink {
                continue;
            }
            self.kept_planes.push(plane);
            for &member in &members {
                let Some(set) = attr_ref(doc, member, "styled_item", "item") else {
                    continue;
                };
                for child in attr_refs(doc, set, "tessellated_geometric_set", "children") {
                    let Some(coords) = attr_ref(doc, child, "tessellated_item", "coordinates")
                        .or_else(|| attr_ref(doc, child, "tessellated_curve_set", "coordinates"))
                    else {
                        continue;
                    };
                    if let Some(A::List(points)) =
                        attr(doc, coords, "coordinates_list", "position_coords")
                    {
                        for point in points {
                            if let A::List(v) = point {
                                let p: Option<Vec<_>> = v
                                    .iter()
                                    .map(|a| match a {
                                        A::Real(x) => Some(*x),
                                        A::Integer(x) => Some(*x as f64),
                                        _ => None,
                                    })
                                    .collect();
                                if let Some(p) = p.and_then(|v| v.try_into().ok()) {
                                    self.points.push(p);
                                }
                            }
                        }
                    }
                }
            }
            let Some(item) = attr_ref(doc, plane, "styled_item", "item") else {
                continue;
            };
            let Some(pos) = attr_ref(doc, item, "elementary_surface", "position") else {
                continue;
            };
            let vector = |entity, field, default| {
                attr_ref(doc, pos, entity, field)
                    .and_then(|id| {
                        let (ty, slot) = if field == "location" {
                            ("cartesian_point", "coordinates")
                        } else {
                            ("direction", "direction_ratios")
                        };
                        let A::List(v) = attr(doc, id, ty, slot)? else {
                            return None;
                        };
                        let v: Option<Vec<_>> = v
                            .iter()
                            .map(|a| match a {
                                A::Real(x) => Some(*x),
                                A::Integer(x) => Some(*x as f64),
                                _ => None,
                            })
                            .collect();
                        let value: [f64; 3] = v?.try_into().ok()?;
                        if field == "location" {
                            Some(value)
                        } else {
                            crate::geom::unit(value)
                        }
                    })
                    .unwrap_or(default)
            };
            self.include(
                [
                    vector("placement", "location", [0.; 3]),
                    vector("axis2_placement_3d", "axis", [0., 0., 1.]),
                    vector("axis2_placement_3d", "ref_direction", [1., 0., 0.]),
                ],
                &[],
            );
        }
    }
}

impl PartPlan<'_> {
    pub(super) fn notes_view(
        &self,
        em: &mut Emitter<'_>,
        global: R,
        planes: &[R],
        frame: Frame,
        report: &mut WriteReport,
    ) -> Result<(), WriteError> {
        if em.edition.entity("model_geometric_view").is_none() {
            report.saved_view_findings.push(Finding {kind:FindingKind::Fact,part:Some(self.id),ids:vec![],entity:"draughting_model".into(),detail:"Notes saved view was not written: edition 1 retained; the optional view requires edition 2 or later".into()});
            return Ok(());
        }
        let own = if self.mode == Mode::Add {
            own_view(self.doc, self.def.shape)
        } else {
            None
        };
        if own.is_none() && em.model_names.contains("Notes") {
            report.saved_view_findings.push(Finding {
                kind: FindingKind::Fact,
                part: Some(self.id),
                ids: vec![],
                entity: "draughting_model".into(),
                detail:
                    "existing Notes draughting model retained; no second Notes model was written"
                        .into(),
            });
            return Ok(());
        }
        if frame.several_orientations && !frame.several_planes {
            report.saved_view_findings.push(Finding {
                kind: FindingKind::Fact,
                part: Some(self.id),
                ids: vec![],
                entity: "annotation_plane".into(),
                detail: "general notes have several orientations; no Notes saved view was written"
                    .into(),
            });
            return Ok(());
        }
        if frame.several_planes {
            report.saved_view_findings.push(Finding {
                kind: FindingKind::Fact,
                part: Some(self.id),
                ids: vec![],
                entity: "draughting_model".into(),
                detail: "general notes span several planes; no Notes saved view was written".into(),
            });
            return Ok(());
        }
        let Some([_, normal, along]) = frame.basis else {
            return Ok(());
        };
        let z = normal.map(|x| -x);
        let x = along.map(|v| -v);
        let y = cross(z, x);
        let axes = [x, y, z];
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for p in frame.points {
            for i in 0..3 {
                let v = dot(p, axes[i]);
                min[i] = min[i].min(v);
                max[i] = max[i].max(v);
            }
        }
        let size: [f64; 3] = std::array::from_fn(|i| (max[i] - min[i]).max(1e-6));
        let d = 2. * dot(size, size).sqrt();
        let centre: [f64; 3] = std::array::from_fn(|i| (min[i] + max[i]) / 2.);
        // z points camera -> target, hence the eye is centre - D*z.
        let eye: [f64; 3] =
            std::array::from_fn(|i| (0..3).map(|j| axes[j][i] * centre[j]).sum::<f64>() - d * z[i]);
        let w = size[0] * 1.1;
        let h = size[1] * 1.1;
        let point = |em: &mut Emitter<'_>, p: &[f64]| {
            em.simple(
                "cartesian_point",
                &[
                    ("name", s("")),
                    (
                        "coordinates",
                        A::List(p.iter().copied().map(A::Real).collect()),
                    ),
                ],
            )
        };
        let direction = |em: &mut Emitter<'_>, p: [f64; 3]| {
            em.simple(
                "direction",
                &[
                    ("name", s("")),
                    ("direction_ratios", A::List(p.map(A::Real).to_vec())),
                ],
            )
        };
        let eye = point(em, &eye)?;
        let z = direction(em, z)?;
        let x = direction(em, x)?;
        let vrs = em.simple(
            "axis2_placement_3d",
            &[
                ("name", s("")),
                ("location", eye.a()),
                ("axis", z.a()),
                ("ref_direction", x.a()),
            ],
        )?;
        let corner = point(em, &[-w / 2., -h / 2.])?;
        let placement = em.simple(
            "axis2_placement_2d",
            &[("name", s("")), ("location", corner.a())],
        )?;
        let win = em.simple(
            "planar_box",
            &[
                ("name", s("")),
                ("size_in_x", A::Real(w)),
                ("size_in_y", A::Real(h)),
                ("placement", placement.a()),
            ],
        )?;
        let origin = point(em, &[0.; 3])?;
        let volume = em.simple(
            "view_volume",
            &[
                ("projection_type", en("parallel")),
                ("projection_point", origin.a()),
                ("view_plane_distance", A::Real(d)),
                ("front_plane_distance", A::Real(0.)),
                ("front_plane_clipping", en("f")),
                ("back_plane_distance", A::Real(2. * d)),
                ("back_plane_clipping", en("f")),
                ("view_volume_sides_clipping", en("t")),
                ("view_window", win.a()),
            ],
        )?;
        let cam = em.simple(
            "camera_model_d3",
            &[
                ("name", s("Notes")),
                ("view_reference_system", vrs.a()),
                ("perspective_of_volume", volume.a()),
            ],
        )?;
        if let Some(own) = own {
            let items = refs(
                [cam]
                    .into_iter()
                    .chain(frame.kept_planes.iter().copied().map(R::Old))
                    .chain(planes.iter().copied())
                    .chain([R::Old(own.part)]),
            );
            em.kept_rewrites.insert(
                own.model,
                p21::simple(
                    "DRAUGHTING_MODEL",
                    vec![s("Notes"), items, em.reference(self.context)?.a()],
                ),
            );
            let mut view = self
                .doc
                .get(own.view)
                .ok_or_else(|| WriteError::Internal("missing owned view".into()))?
                .clone();
            crate::p21::visit_attributes_mut(&mut view, &mut |a| {
                if *a == A::EntityRef(own.camera) {
                    *a = cam.a();
                }
            });
            em.kept_rewrites.insert(own.view, view);
            em.view_edits.extend([own.view, own.model]);
            // Retire only private camera support. Shared origins/placements and
            // cameras referenced by another view remain untouched.
            let mut private = BTreeSet::new();
            let mut pending = vec![own.camera];
            while let Some(id) = pending.pop() {
                if private.insert(id) {
                    pending.extend(references(self.doc, id));
                }
            }
            loop {
                let shared: Vec<_> = private
                    .iter()
                    .copied()
                    .filter(|id| {
                        self.doc
                            .referrers(*id)
                            .iter()
                            .any(|r| !private.contains(r) && ![own.view, own.model].contains(r))
                    })
                    .collect();
                if shared.is_empty() {
                    break;
                }
                for id in shared {
                    private.remove(&id);
                }
            }
            em.view_edits.extend(&private);
            em.retired.extend(private);
            report.saved_view_findings.push(Finding {
                kind: FindingKind::Fact,
                part: Some(self.id),
                ids: vec![own.view],
                entity: "draughting_model".into(),
                detail: "reused the writer-owned Notes saved view for the added general notes"
                    .into(),
            });
            return Ok(());
        }
        let identity = em.simple(
            "axis2_placement_3d",
            &[("name", s("")), ("location", origin.a())],
        )?;
        let source = em.simple(
            "representation_map",
            &[
                ("mapping_origin", identity.a()),
                ("mapped_representation", em.reference(self.shape_rep)?.a()),
            ],
        )?;
        let part = em.simple(
            "mapped_item",
            &[
                ("name", s("")),
                ("mapping_source", source.a()),
                ("mapping_target", identity.a()),
            ],
        )?;
        let style = em.simple(
            "presentation_style_assignment",
            &[("styles", A::List(vec![typed("null_style", en("null"))]))],
        )?;
        let styled = em.simple(
            "styled_item",
            &[
                ("name", s("")),
                ("styles", refs([style])),
                ("item", part.a()),
            ],
        )?;
        // DRAUGHTING_MODEL.WR2 rejects a directly contained NULL-only styled
        // mapped item. Keep that style in an ordinary representation instead.
        em.simple(
            "representation",
            &[
                ("name", s("Notes part styling")),
                ("items", refs([styled])),
                ("context_of_items", em.reference(self.context)?.a()),
            ],
        )?;
        em.model_names.insert("Notes".into());
        let model = em.simple(
            "draughting_model",
            &[
                ("name", s("Notes")),
                (
                    "items",
                    refs(
                        [cam]
                            .into_iter()
                            .chain(planes.iter().copied())
                            .chain([part]),
                    ),
                ),
                ("context_of_items", em.reference(self.context)?.a()),
            ],
        )?;
        em.simple(
            "mechanical_design_and_draughting_relationship",
            &[
                ("name", s("")),
                ("description", s("")),
                ("rep_1", model.a()),
                ("rep_2", global.a()),
            ],
        )?;
        let existing = self.doc.ids().find(|id| {
            !em.removed.contains(id)
                && is_a(self.doc, *id, "default_model_geometric_view")
                && attr_ref(self.doc, *id, "shape_aspect", "of_shape") == Some(self.def.shape)
        });
        let mut vals = vec![
            (("characterized_object", "name"), s("Notes")),
            (
                ("characterized_item_within_representation", "item"),
                cam.a(),
            ),
            (
                ("characterized_item_within_representation", "rep"),
                model.a(),
            ),
        ];
        let ty = if let Some(id) = existing {
            report.saved_view_findings.push(Finding {
                kind: FindingKind::Fact,
                part: Some(self.id),
                ids: vec![id],
                entity: "default_model_geometric_view".into(),
                detail: format!(
                    "default view #{id} retained; Notes written as an ordinary saved view"
                ),
            });
            "model_geometric_view"
        } else {
            vals.extend([
                (("shape_aspect", "name"), s("Notes")),
                (
                    ("shape_aspect", "of_shape"),
                    em.reference(self.def.shape)?.a(),
                ),
            ]);
            "default_model_geometric_view"
        };
        em.instance(&[ty], &vals, &[])?;
        Ok(())
    }
}

pub(super) struct OwnView {
    view: u64,
    model: u64,
    camera: u64,
    part: u64,
}
pub(super) fn own_view(doc: &Document, shape: u64) -> Option<OwnView> {
    for view in doc
        .ids()
        .filter(|id| is_a(doc, *id, "model_geometric_view"))
    {
        let Some(model) = attr_ref(doc, view, "characterized_item_within_representation", "rep")
        else {
            continue;
        };
        if attr_str(doc, model, "representation", "name").as_deref() != Some("Notes") {
            continue;
        }
        let related = doc.referrers(model).iter().any(|id| {
            is_a(doc, *id, "mechanical_design_and_draughting_relationship")
                && attr_ref(doc, *id, "representation_relationship", "rep_1") == Some(model)
                && attr_ref(doc, *id, "representation_relationship", "rep_2").is_some_and(|g| {
                    attr_str(doc, g, "representation", "name")
                        .is_some_and(|n| n == "PMI callouts" || n.starts_with("PMI callouts ("))
                })
        });
        if !related {
            continue;
        }
        for part in attr_refs(doc, model, "representation", "items") {
            let matches = attr_ref(doc, part, "mapped_item", "mapping_source")
                .and_then(|m| attr_ref(doc, m, "representation_map", "mapped_representation"))
                .is_some_and(|rep| {
                    doc.referrers(rep).iter().any(|r| {
                        attr_ref(doc, *r, "property_definition_representation", "definition")
                            == Some(shape)
                    })
                });
            if matches {
                return Some(OwnView {
                    view,
                    model,
                    camera: attr_ref(
                        doc,
                        view,
                        "characterized_item_within_representation",
                        "item",
                    )?,
                    part,
                });
            }
        }
    }
    None
}

/// Only our precise Notes/global-model pairing is eligible for owned cleanup.
/// Foreign saved views are never enrolled merely because they contain a camera.
pub(super) fn owned_roots(
    doc: &Document,
    shape: u64,
    views: &[super::super::SavedView],
) -> BTreeSet<u64> {
    let mut roots = BTreeSet::new();
    for view in views {
        let view = view.id;
        let Some(model) = attr_ref(doc, view, "characterized_item_within_representation", "rep")
        else {
            continue;
        };
        if attr_str(doc, model, "representation", "name").as_deref() != Some("Notes") {
            continue;
        }
        let items = attr_refs(doc, model, "representation", "items");
        let mapped = items.iter().any(|id| {
            attr_ref(doc, *id, "mapped_item", "mapping_source")
                .and_then(|map| attr_ref(doc, map, "representation_map", "mapped_representation"))
                .is_some_and(|rep| {
                    doc.referrers(rep).iter().any(|r| {
                        attr_ref(doc, *r, "property_definition_representation", "definition")
                            == Some(shape)
                    })
                })
        });
        let relations: Vec<_> = doc
            .referrers(model)
            .iter()
            .copied()
            .filter(|id| {
                is_a(doc, *id, "mechanical_design_and_draughting_relationship")
                    && attr_ref(doc, *id, "representation_relationship", "rep_1") == Some(model)
                    && attr_ref(doc, *id, "representation_relationship", "rep_2").is_some_and(|g| {
                        attr_str(doc, g, "representation", "name")
                            .is_some_and(|n| n == "PMI callouts" || n.starts_with("PMI callouts ("))
                    })
            })
            .collect();
        if !mapped || relations.is_empty() {
            continue;
        }
        roots.extend([view, model]);
        roots.extend(relations);
        for item in items {
            if is_a(doc, item, "annotation_plane") {
                continue;
            }
            roots.insert(item);
            for &styled in doc
                .referrers(item)
                .iter()
                .filter(|id| is_a(doc, **id, "styled_item"))
            {
                roots.insert(styled);
                roots.extend(doc.referrers(styled).iter().copied().filter(|id| {
                    attr_str(doc, *id, "representation", "name").as_deref()
                        == Some("Notes part styling")
                }));
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opposite_normals_and_flipped_along_are_distinct_text_orientations() {
        for (normal, along) in [
            ([0., 0., -1.], [1., 0., 0.]),
            ([0., 0., -1.], [-1., 0., 0.]),
            ([0., 0., 1.], [-1., 0., 0.]),
        ] {
            let mut f = Frame::new();
            f.include([[0.; 3], [0., 0., 1.], [1., 0., 0.]], &[]);
            f.include([[0.; 3], normal, along], &[]);
            assert!(!f.one_plane());
            assert!(f.several_orientations);
            assert!(!f.several_planes);
        }
        let mut f = Frame::new();
        f.include([[0.; 3], [0., 0., 1.], [1., 0., 0.]], &[]);
        f.include([[2., 3., 0.], [0., 0., 1.], [1., 0., 0.]], &[]);
        assert!(f.one_plane());
    }
}
