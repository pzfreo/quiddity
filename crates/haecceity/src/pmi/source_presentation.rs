//! Placement-only input. Source tessellation and saved views are deliberately ignored.
use super::*;
use crate::{
    geom,
    json::{
        self,
        presentation::{Presentation, TextPlacement},
    },
    presentation::{self, Callout, notes},
};

type Key = (String, usize);
pub(super) type Placements = BTreeMap<Key, Placement>;
type Graphic = (
    Vec<R>,
    &'static str,
    String,
    [[f64; 3]; 3],
    Vec<Vec<[f64; 3]>>,
);

pub(super) struct Placement {
    id: String,
    plane: [[f64; 3]; 3],
    text: TextPlacement,
    text_anchor: [f64; 2],
    leader: Option<[f64; 3]>,
    callout: Callout,
    gps_data: bool,
}

fn point(em: &mut Emitter<'_>, p: [f64; 3]) -> Result<R, WriteError> {
    em.simple(
        "cartesian_point",
        &[
            ("name", s("")),
            ("coordinates", A::List(p.map(A::Real).to_vec())),
        ],
    )
}
fn direction(em: &mut Emitter<'_>, p: [f64; 3]) -> Result<R, WriteError> {
    em.simple(
        "direction",
        &[
            ("name", s("")),
            ("direction_ratios", A::List(p.map(A::Real).to_vec())),
        ],
    )
}
fn invalid(id: &str, why: &str) -> WriteError {
    WriteError::Parts(format!("presentation item {id}: {why}"))
}
fn frame(
    id: &str,
    origin: [f64; 3],
    x: [f64; 3],
    normal: [f64; 3],
) -> Result<[[f64; 3]; 3], WriteError> {
    if origin
        .into_iter()
        .chain(x)
        .chain(normal)
        .any(|v| !v.is_finite())
    {
        return Err(invalid(id, "non-finite placement"));
    }
    let normal = geom::unit(normal).ok_or_else(|| invalid(id, "degenerate plane normal"))?;
    let x = geom::unit(x).ok_or_else(|| invalid(id, "degenerate plane x axis"))?;
    if geom::norm(geom::cross(x, normal)) < 1e-12 {
        return Err(invalid(id, "degenerate plane x axis"));
    }
    let x = geom::unit(geom::sub(x, geom::scale(normal, geom::dot(x, normal))))
        .ok_or_else(|| invalid(id, "degenerate plane x axis"))?;
    Ok([origin, normal, x])
}

pub(super) fn validate_parts(
    input: &[(PartId, Presentation)],
    pmi: &[(PartId, PartPmi)],
    mode: Mode,
) -> Result<(), WriteError> {
    let mut seen = BTreeSet::new();
    for (part, _) in input {
        if mode == Mode::Remove || !pmi.iter().any(|(p, _)| p == part) || !seen.insert(part) {
            return Err(WriteError::Parts(format!(
                "source presentation part {} is absent, repeated or being removed",
                part.0
            )));
        }
    }
    Ok(())
}

/// Bounds of the first readable cell's actual strokes, excluding its frame.
/// Native GD&T symbols can be asymmetric about their nominal run centre.
fn text_bounds(callout: &Callout) -> Option<[[f64; 2]; 2]> {
    let run = callout.runs.first()?;
    let is_lettering = callout
        .letters
        .iter()
        .any(|letter| letter.text == run.text && letter.at == run.at && letter.size == run.size);
    if !is_lettering && let Some(cell) = callout.cell_boxes.first() {
        let mut bounds = geom::Bounds::empty();
        for line in callout.polylines().iter().skip(callout.frame_strokes) {
            if line
                .iter()
                .all(|p| (0..2).all(|k| p[k] >= cell[0][k] - 1e-9 && p[k] <= cell[1][k] + 1e-9))
            {
                for p in line {
                    bounds.add([p[0], p[1], 0.]);
                }
            }
        }
        return bounds.max_extent().is_finite().then_some([
            [bounds.min[0], bounds.min[1]],
            [bounds.max[0], bounds.max[1]],
        ]);
    }
    let ink = presentation::font().text(&run.text, run.size);
    if ink.lines.is_empty() {
        return None;
    }
    Some([
        std::array::from_fn(|k| run.at[k] + ink.min[k]),
        std::array::from_fn(|k| run.at[k] + ink.max[k]),
    ])
}

pub(super) fn prepare(
    pmi: &PartPmi,
    ids: Option<&BTreeMap<String, Vec<String>>>,
    input: Option<&Presentation>,
) -> Result<Placements, WriteError> {
    let Some(input) = input else {
        return Ok(BTreeMap::new());
    };
    if input.schema != "haecceity.presentation/1" || input.length_unit != "millimetre" {
        return Err(invalid(
            "document",
            "expected haecceity.presentation/1 in millimetres",
        ));
    }
    let mut semantic =
        json::identify(pmi.clone(), None).map_err(|e| invalid("document", &e.to_string()))?;
    if let Some(ids) = ids {
        semantic.ids = ids
            .iter()
            .filter(|(k, _)| !k.starts_with("__"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some(units) = ids.get("__units").and_then(|v| v.first()) {
            semantic.unit_table =
                Some(serde_json::from_str(units).map_err(|e| invalid("document", &e.to_string()))?);
        }
    }
    let known: BTreeMap<_, _> = semantic
        .ids
        .iter()
        .filter(|(c, _)| !c.starts_with("__"))
        .flat_map(|(c, ids)| {
            ids.iter()
                .enumerate()
                .map(move |(i, id)| (id.as_str(), (c.clone(), i)))
        })
        .collect();
    // semantic_ids can also contain unit ids, which are not callout owners.
    let all =
        json::presentation::id_set(&semantic).map_err(|e| invalid("document", &e.to_string()))?;
    for id in &input.semantic_ids {
        if !all.contains(id) {
            return Err(invalid(id, "unknown semantic id"));
        }
    }
    let mut planes = BTreeMap::new();
    for p in &input.planes {
        if planes.insert(p.id.as_str(), p).is_some() {
            return Err(invalid(&p.id, "duplicate plane id"));
        }
    }
    let rows = notes::general_notes(pmi);
    let mut out = BTreeMap::new();
    for (id, graphics) in &input.items {
        let key = known
            .get(id.as_str())
            .ok_or_else(|| invalid(id, "unknown semantic id"))?;
        // A plane alone also accompanies regenerated graphic-only callouts. It
        // supplies no text insertion point and requests no source override.
        // Multiple view variants use the first main text placement.
        let Some(g) = graphics
            .iter()
            .find(|g| !g.text_placements.is_empty() || g.leader.is_some())
        else {
            continue;
        };
        let text = g
            .text_placements
            .first()
            .ok_or_else(|| invalid(id, "placed item is missing a text position"))?;
        let text_frame = frame(id, text.origin, text.x_axis, text.normal)?;
        let alignment = text.alignment.as_deref().unwrap_or("centre");
        let lower = json::presentation::alignment_lower(alignment)
            .ok_or_else(|| invalid(id, &format!("unsupported text alignment {alignment:?}")))?;
        if text.height.is_some_and(|h| !h.is_finite() || h <= 0.) {
            return Err(invalid(id, "text height must be positive"));
        }
        let plane = if let Some(plane_id) = &g.plane_id {
            let p = planes
                .get(plane_id.as_str())
                .ok_or_else(|| invalid(id, "unknown annotation plane"))?;
            frame(id, p.origin, p.x_axis, p.normal)?
        } else {
            text_frame
        };
        if g.leader.is_some_and(|p| p.iter().any(|v| !v.is_finite())) {
            return Err(invalid(id, "non-finite leader tip"));
        }
        let (collection, i) = key;
        let source = match collection.as_str() {
            "notes" => Some(notes::Source::Note(*i)),
            "attributes" => Some(notes::Source::Attribute(*i)),
            "general" => Some(notes::Source::General(*i)),
            "surface_textures" => Some(notes::Source::Texture(*i)),
            _ => None,
        };
        let callout = match collection.as_str() {
            "datums" => presentation::datum(pmi.datums[*i].label().as_str()),
            "dimensions" => presentation::dimension(&pmi.dimensions[*i]),
            "tolerances" => presentation::tolerance(&pmi.tolerances[*i], pmi),
            "threads" => presentation::thread(&pmi.threads[*i], pmi),
            "surface_textures" if pmi.surface_textures[*i].on.is_some() => {
                notes::surface_texture(&pmi.surface_textures[*i])
            }
            "notes" => presentation::label(&pmi.notes[*i].text),
            _ if source.is_some() => {
                let mut text = rows
                    .iter()
                    .filter(|r| Some(r.source) == source)
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                // The typed Rust writer migrates legacy notes to semantic-text
                // attributes. Feature-owned notes have no general-notes row.
                if text.is_empty()
                    && collection == "attributes"
                    && pmi.attributes[*i].name == "semantic text"
                {
                    text = pmi.attributes[*i]
                        .items
                        .iter()
                        .filter_map(|(_, value)| match value {
                            AttributeValue::Text(text) => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                }
                if text.is_empty() {
                    return Err(invalid(id, "item has no drawable callout"));
                }
                presentation::label(&text)
            }
            _ => return Err(invalid(id, "item has no drawable callout")),
        };
        let bounds = text_bounds(&callout)
            .ok_or_else(|| invalid(id, "no readable text ink for alignment"))?;
        let text_anchor =
            std::array::from_fn(|k| bounds[0][k] - lower[k] * (bounds[1][k] - bounds[0][k]));
        let mut text = text.clone();
        text.x_axis = text_frame[2];
        text.normal = text_frame[1];
        out.insert(
            key.clone(),
            Placement {
                id: id.clone(),
                plane,
                text,
                text_anchor,
                leader: g.leader,
                callout,
                // RP 7.2.2 requires GPS_DATA for semantic PMI, including
                // property_definition notes (7.4). Applied feature labels use
                // shape_aspect carriers, outside AP242 APO.WR2's GPS correlation.
                gps_data: !matches!(collection.as_str(), "datums" | "threads"),
            },
        );
    }
    // Validate unused planes too, so an invalid companion is never partly accepted.
    for p in &input.planes {
        frame(&p.id, p.origin, p.x_axis, p.normal)?;
    }
    Ok(out)
}

impl Placement {
    pub(super) fn leader_start(&self) -> Option<usize> {
        self.leader.map(|_| self.callout.polylines().len())
    }
    fn scale(&self, default_height: f64) -> f64 {
        self.text.height.map_or(default_height / 3., |height| {
            height / self.callout.runs.first().map_or(3., |run| run.size)
        })
    }
    fn draw(&self, factor: f64, default_height: f64) -> Vec<Vec<[f64; 3]>> {
        presentation::placement::source(
            &self.callout,
            [
                self.text.origin.map(|v| v / factor),
                self.text.normal,
                self.text.x_axis,
            ],
            self.scale(default_height) * 3. / factor,
            self.text_anchor,
            self.leader.map(|p| p.map(|v| v / factor)),
        )
    }
    pub(super) fn emit_metadata(
        &self,
        em: &mut Emitter<'_>,
        style: R,
        factor: f64,
        default_height: f64,
        name: &str,
        kind: &str,
        alignment_role: &mut Option<R>,
    ) -> Result<R, WriteError> {
        let o = point(em, self.text.origin.map(|v| v / factor))?;
        let n = direction(em, self.text.normal)?;
        let x = direction(em, self.text.x_axis)?;
        let placement = em.simple(
            "axis2_placement_3d",
            &[
                ("name", s("")),
                ("location", o.a()),
                ("axis", n.a()),
                ("ref_direction", x.a()),
            ],
        )?;
        // A placeholder records the source insertion point without drawing another
        // font over the shared tessellated strokes.
        let scale = self.scale(default_height) / factor;
        let height = self.callout.runs.first().map_or(3., |run| run.size) * scale;
        let b = em.simple(
            "planar_box",
            &[
                ("name", s("")),
                (
                    "size_in_x",
                    A::Real((self.callout.size[0] * scale).max(height)),
                ),
                ("size_in_y", A::Real(height)),
                ("placement", placement.a()),
            ],
        )?;
        // RP 7.2.2: the axis placement must be a direct set member; the
        // optional box alone does not supply the placeholder's insertion frame.
        let set = em.simple(
            "geometric_set",
            &[("name", s(kind)), ("elements", refs([placement, b]))],
        )?;
        let placeholder = em.simple(
            "annotation_placeholder_occurrence",
            &[
                ("name", s(name)),
                ("styles", refs([style])),
                ("item", set.a()),
                (
                    "role",
                    en(if self.gps_data {
                        "gps_data"
                    } else {
                        "annotation_text"
                    }),
                ),
                ("line_spacing", A::Real(height)),
            ],
        )?;
        // APO has no alignment attribute. Keep the application vocabulary key in
        // an AP242 identification assignment, outside the displayed names (RP
        // 7.2.2 / 8.4). No extra text occurrence is rendered over the strokes.
        if let Some(alignment) = &self.text.alignment {
            let role = if let Some(role) = *alignment_role {
                role
            } else {
                em.simple(
                    "identification_role",
                    &[
                        ("name", s("haecceity.presentation/1 text alignment")),
                        (
                            "description",
                            s("Application text alignment key; not annotation text"),
                        ),
                    ],
                )?
            };
            *alignment_role = Some(role);
            em.simple(
                "applied_identification_assignment",
                &[
                    ("assigned_id", s(alignment)),
                    ("role", role.a()),
                    ("items", refs([placeholder])),
                ],
            )?;
        }
        Ok(placeholder)
    }
}

impl PartPlan<'_> {
    fn source_definition(
        &self,
        key: &Key,
        symbols: &[(R, Vec<String>, usize)],
        st: &Emitted,
    ) -> Option<R> {
        let (collection, i) = key;
        match collection.as_str() {
            "datums" => symbols
                .iter()
                .find(|(_, labels, _)| {
                    labels.contains(&self.pmi.datums[*i].label().as_str().to_string())
                })
                .map(|s| s.0),
            "dimensions" => st.dimensions.get(i).copied(),
            "tolerances" => st.tolerances.get(i).copied(),
            "threads" => st.threads.get(i).copied(),
            "notes" => st.properties.get(&notes::Source::Note(*i)).copied(),
            "attributes" => st.properties.get(&notes::Source::Attribute(*i)).copied(),
            "general" => st.properties.get(&notes::Source::General(*i)).copied(),
            "surface_textures" => st.properties.get(&notes::Source::Texture(*i)).copied(),
            _ => None,
        }
    }
    pub(super) fn source_for_definitions(
        &self,
        symbols: &[(R, Vec<String>, usize)],
        st: &Emitted,
        definitions: &[R],
    ) -> Option<&Placement> {
        self.source
            .iter()
            .find(|(key, _)| {
                self.source_definition(key, symbols, st)
                    .is_some_and(|d| definitions.contains(&d))
            })
            .map(|(_, p)| p)
    }
    pub(super) fn source_graphics(
        &self,
        symbols: &[(R, Vec<String>, usize)],
        st: &Emitted,
        factor: f64,
        default_height: f64,
        report: &mut WriteReport,
    ) -> Result<Vec<Graphic>, WriteError> {
        let mut owners = BTreeMap::new();
        self.source
            .iter()
            .map(|(key, p)| {
                let definition = self
                    .source_definition(key, symbols, st)
                    .ok_or_else(|| invalid(&p.id, "item has no written callout owner"))?;
                if let Some(first) = owners.insert(definition, &p.id) {
                    return Err(invalid(&p.id, &format!("shares a callout definition with {first}; only one placement is supported")));
                }
                if geom::dot(p.text.normal, p.plane[1]).abs() < 1e-6 {
                    report.presentation_warnings.push(format!(
                        "{}: placed text is edge-on to the annotation plane normal",
                        p.id
                    ));
                }
                let kind = match key.0.as_str() {
                    "datums" => "datum",
                    "dimensions" => match self.pmi.dimensions[key.1].kind {
                        DimensionKind::Location { angle: None, .. }
                        | DimensionKind::Size { angle: Some(_), .. } => "general dimension",
                        _ => presentation::dimension_type(&self.pmi.dimensions[key.1]),
                    },
                    "tolerances" => presentation::tolerance_type(&self.pmi.tolerances[key.1]),
                    "threads" => "label",
                    "surface_textures" => "surface roughness",
                    _ => "placed note",
                };
                Ok((
                    vec![definition],
                    kind,
                    p.callout.notation(),
                    [p.plane[0].map(|v| v / factor), p.plane[1], p.plane[2]],
                    p.draw(factor, default_height),
                ))
            })
            .collect()
    }
}

pub(super) fn overlap_warnings(graphics: &[Graphic], placed: &[Graphic], report: &mut WriteReport) {
    if placed.is_empty() {
        return;
    }
    for (i, g) in graphics.iter().enumerate() {
        for other in &graphics[..i] {
            if !placed.iter().any(|p| p.0 == g.0 || p.0 == other.0) {
                continue;
            }
            if geom::dot(g.3[1], other.3[1]).abs() < 0.999 {
                continue;
            }
            let n = g.3[1];
            let x = g.3[2];
            let y = geom::cross(n, x);
            let bounds = |lines: &[Vec<[f64; 3]>]| {
                let mut b = geom::Bounds::empty();
                for &p in lines.iter().flatten() {
                    b.add([geom::dot(p, x), geom::dot(p, y), geom::dot(p, n)]);
                }
                b
            };
            let a = bounds(&g.4);
            let b = bounds(&other.4);
            if (0..2).all(|k| a.min[k] < b.max[k] && b.min[k] < a.max[k])
                && geom::dot(geom::sub(g.3[0], other.3[0]), n).abs() < 1e-6
            {
                report
                    .presentation_warnings
                    .push(format!("placed callout {:?} overlaps {:?}", g.2, other.2));
            }
        }
    }
}
