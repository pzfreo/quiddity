//! Optional placement and saved-view metadata for the presentation companion.
use super::*;

/// A model-space frame in millimetres, keyed by a document-local annotation plane id.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[serde(deny_unknown_fields)]
pub struct Plane {
    pub id: String,
    pub name: String,
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub normal: [f64; 3],
}

/// The alignment point and axes of one text entry. A placeholder has no text index:
/// its line spacing is retained separately, never passed off as a character height.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[serde(deny_unknown_fields)]
pub struct TextPlacement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_index: Option<usize>,
    /// Lower-left / upper-right in placement coordinates relative to origin (mm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_box: Option<[[f64; 2]; 2]>,
    /// True when this run is already drawn by the graphic's polylines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroked: Option<bool>,
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub normal: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
}

/// One draughting-model group, or one of its saved camera views. Direction points
/// into the scene (-camera Z); up is camera Y. Unoriented groups omit both axes.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<[f64; 3]>,
    pub graphics: Vec<String>,
}

impl TextPlacement {
    fn at(frame: Transform, scale: f64) -> Option<Self> {
        Some(Self {
            text_index: None,
            cell_box: None,
            stroked: None,
            origin: geom::scale(frame.origin, scale),
            x_axis: geom::unit(frame.axes[0])?,
            normal: geom::unit(geom::cross(frame.axes[0], frame.axes[1]))?,
            height: None,
            line_spacing: None,
            alignment: None,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct TextContext {
    plane: Option<Transform>,
    height: Option<f64>,
    pub depth: usize,
}
impl TextContext {
    pub fn at(self, doc: &Document, id: u64) -> Self {
        Self {
            height: style_height(doc, id).or(self.height),
            ..self
        }
    }
    pub fn descend(self) -> Self {
        Self {
            depth: self.depth + 1,
            ..self
        }
    }
}
fn positive(value: Option<&A>) -> Option<f64> {
    let n = match value? {
        A::Typed { value, .. } => return positive(Some(value)),
        value => number(value)?,
    };
    (n.is_finite() && n > 0.).then_some(n)
}
fn style_height(doc: &Document, owner: u64) -> Option<f64> {
    let mut stack = refs(attr(doc, owner, "styled_item", "styles"));
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        for characteristic in list(attr(
            doc,
            id,
            "text_style_with_box_characteristics",
            "characteristics",
        )) {
            if let A::Typed { type_name, value } = characteristic
                && type_name.eq_ignore_ascii_case("box_height")
                && let Some(height) = positive(Some(value))
            {
                return Some(height);
            }
        }
        stack.extend(refs(attr(
            doc,
            id,
            "presentation_style_assignment",
            "styles",
        )));
    }
    None
}
/// Lower-left offset from the alignment point, in fractions of the text box.
/// Shared by source placement and literal extraction so /1 aliases stay consistent.
pub(crate) fn alignment_lower(alignment: &str) -> Option<[f64; 2]> {
    match alignment.replace('_', " ").to_ascii_lowercase().as_str() {
        "baseline left" | "bottom left" => Some([0., 0.]),
        "baseline centre" | "baseline center" | "bottom centre" | "bottom center" => {
            Some([-0.5, 0.])
        }
        "baseline right" | "bottom right" => Some([-1., 0.]),
        "centre" | "center" | "middle centre" | "middle center" => Some([-0.5, -0.5]),
        "middle left" => Some([0., -0.5]),
        "middle right" => Some([-1., -0.5]),
        "top left" => Some([0., -1.]),
        "top centre" | "top center" => Some([-0.5, -1.]),
        "top right" => Some([-1., -1.]),
        _ => None,
    }
}

pub(super) fn literal(
    doc: &Document,
    id: u64,
    transform: Transform,
    scale: f64,
    context: TextContext,
    graphic: &mut Graphic,
) {
    let index = graphic.text.len();
    graphic
        .text
        .push(text(attr(doc, id, "text_literal", "literal")));
    let frame = reference(attr(doc, id, "text_literal", "placement"))
        .and_then(|id| placement(doc, id))
        .map(|p| transform.then(p))
        .or(context.plane);
    let Some(frame) = frame else {
        graphic
            .unsupported
            .push(format!("#{id}: text placement unresolved"));
        return;
    };
    let Some(mut placed) = TextPlacement::at(frame, scale) else {
        graphic
            .unsupported
            .push(format!("#{id}: degenerate text placement"));
        return;
    };
    placed.text_index = Some(index);
    placed.alignment = match attr(doc, id, "text_literal", "alignment") {
        Some(A::String(value)) => Some(value.clone()),
        _ => None,
    };
    let extent = reference(attr(doc, id, "text_literal_with_extent", "extent"));
    let height = context.height.or_else(|| {
        extent.and_then(|extent| positive(attr(doc, extent, "planar_extent", "size_in_y")))
    });
    placed.height = height.map(|height| height * geom::norm(frame.axes[1]) * scale);
    if let Some(extent) = extent
        && let (Some(width), Some(height)) = (
            positive(attr(doc, extent, "planar_extent", "size_in_x")),
            positive(attr(doc, extent, "planar_extent", "size_in_y")),
        )
    {
        // A literal's extent is a box, distinct from its font/style height.
        // Unknown alignment cannot locate that box relative to its anchor.
        let lower = alignment_lower(placed.alignment.as_deref().unwrap_or(""));
        if let Some([x, y]) = lower {
            let w = width * geom::norm(frame.axes[0]) * scale;
            let h = height * geom::norm(frame.axes[1]) * scale;
            placed.cell_box = Some([[x * w, y * h], [(x + 1.) * w, (y + 1.) * h]]);
        }
    }
    if placed.height.is_none() {
        graphic
            .unsupported
            .push(format!("#{id}: text height not stated by style or extent"));
    }
    graphic.text_placements.push(placed);
}
pub(super) fn placeholder(
    doc: &Document,
    id: u64,
    transform: Transform,
    scale: f64,
    graphic: &mut Graphic,
) {
    let geometry = reference(attr(doc, id, "styled_item", "item"));
    let members = geometry
        .map(|id| refs(attr(doc, id, "geometric_set", "elements")))
        .unwrap_or_default();
    let spacing = attr(doc, id, "annotation_placeholder_occurrence", "line_spacing")
        .and_then(number)
        .filter(|n| n.is_finite() && *n >= 0.);
    if spacing.is_none_or(|n| n == 0.) {
        graphic.unsupported.push(format!(
            "#{id}: non-positive or unresolved placeholder line spacing"
        ));
    }
    let mut found = false;
    for member in members {
        let frame = if is(doc, member, "planar_box") {
            reference(attr(doc, member, "planar_box", "placement"))
                .and_then(|id| placement(doc, id))
        } else {
            placement(doc, member)
        };
        if let Some(frame) = frame
            && let Some(mut placed) = TextPlacement::at(transform.then(frame), scale)
        {
            placed.line_spacing =
                spacing.map(|height| height * scale * geom::norm(transform.axes[1]));
            if is(doc, member, "planar_box") {
                placed.height = positive(attr(doc, member, "planar_extent", "size_in_y"))
                    .map(|height| height * scale * geom::norm(transform.axes[1]));
            }
            if let Some(alignment) = text(attr(doc, id, "representation_item", "name"))
                .strip_prefix("haecceity placed text: ")
            {
                placed.alignment = Some(alignment.into());
            }
            graphic.step_ids.push(member);
            graphic.text_placements.push(placed);
            found = true;
        }
    }
    graphic.unsupported.push(format!(
        "#{id}: placeholder has no editable literal{}",
        if found { "" } else { "; placement unresolved" }
    ));
}

fn inverse(frame: Transform) -> Transform {
    let axes = std::array::from_fn(|i| std::array::from_fn(|j| frame.axes[j][i]));
    Transform {
        origin: frame.axes.map(|axis| -geom::dot(axis, frame.origin)),
        axes,
    }
}
pub(super) fn mapping(doc: &Document, id: u64) -> Option<Transform> {
    let source = reference(attr(doc, id, "mapped_item", "mapping_source"))?;
    let origin = placement(
        doc,
        reference(attr(doc, source, "representation_map", "mapping_origin"))?,
    )?;
    let target = reference(attr(doc, id, "mapped_item", "mapping_target"))?;
    let target = if is(doc, target, "placement") {
        placement(doc, target)?
    } else {
        let location = point(
            doc,
            reference(attr(
                doc,
                target,
                "cartesian_transformation_operator",
                "local_origin",
            ))?,
        )?;
        let axis = |name, default| {
            reference(attr(doc, target, "cartesian_transformation_operator", name))
                .and_then(|id| direction(doc, id))
                .unwrap_or(default)
        };
        let x = axis("axis1", [1., 0., 0.]);
        let y = axis("axis2", [0., 1., 0.]);
        let z = reference(attr(
            doc,
            target,
            "cartesian_transformation_operator_3d",
            "axis3",
        ))
        .and_then(|id| direction(doc, id))
        .or_else(|| geom::unit(geom::cross(x, y)))?;
        let scale = match attr(doc, target, "cartesian_transformation_operator", "scale") {
            Some(A::Unset) | None => 1.,
            value => positive(value)?,
        };
        Transform {
            origin: location,
            axes: [
                geom::scale(x, scale),
                geom::scale(y, scale),
                geom::scale(z, scale),
            ],
        }
    };
    Some(target.then(inverse(origin)))
}

fn descendants(doc: &Document, roots: impl IntoIterator<Item = u64>) -> BTreeSet<u64> {
    let mut stack: Vec<_> = roots.into_iter().collect();
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if seen.insert(id) {
            if is(doc, id, "representation") {
                stack.extend(refs(attr(doc, id, "representation", "items")));
            } else {
                stack.extend(children(doc, id));
            }
        }
    }
    seen
}
pub(super) struct PlaneIndex {
    pub planes: Vec<Plane>,
    frames: BTreeMap<u64, Transform>,
    members: BTreeMap<u64, BTreeSet<u64>>,
}
impl PlaneIndex {
    pub fn read(doc: &Document) -> Self {
        let mut index = Self {
            planes: vec![],
            frames: BTreeMap::new(),
            members: BTreeMap::new(),
        };
        for id in doc.ids().filter(|&id| is(doc, id, "annotation_plane")) {
            for member in descendants(doc, children(doc, id)) {
                index.members.entry(member).or_default().insert(id);
            }
            let frame = reference(attr(doc, id, "styled_item", "item")).and_then(|item| {
                reference(attr(doc, item, "elementary_surface", "position"))
                    .or_else(|| reference(attr(doc, item, "planar_box", "placement")))
                    .and_then(|id| placement(doc, id))
            });
            if let (Some(frame), Some(scale)) = (frame, scale_for(doc, id)) {
                index.frames.insert(id, frame);
                index.planes.push(Plane {
                    id: format!("step:{id}"),
                    name: text(attr(doc, id, "representation_item", "name")),
                    origin: geom::scale(frame.origin, scale),
                    x_axis: frame.axes[0],
                    normal: frame.axes[2],
                });
            }
        }
        index
    }
    pub fn context(&self, root: u64, graphic: &mut Graphic) -> TextContext {
        let Some(planes) = self.members.get(&root) else {
            return TextContext::default();
        };
        if planes.len() != 1 {
            graphic.unsupported.push(format!(
                "#{root}: belongs to multiple annotation planes {planes:?}; no single plane chosen"
            ));
            return TextContext::default();
        }
        let Some(&plane) = planes.first() else {
            return TextContext::default();
        };
        if let Some(&frame) = self.frames.get(&plane) {
            graphic.plane_id = Some(format!("step:{plane}"));
            TextContext {
                plane: Some(frame),
                ..TextContext::default()
            }
        } else {
            graphic.unsupported.push(format!(
                "#{plane}: annotation plane placement or length unit unresolved"
            ));
            TextContext::default()
        }
    }
}
pub(super) fn views(doc: &Document, presentation: &Presentation) -> Vec<View> {
    let graphics: BTreeMap<_, _> = presentation
        .items
        .values()
        .flatten()
        .chain(&presentation.unlinked)
        .filter_map(|g| g.id.as_ref().map(|id| (id, g)))
        .collect();
    let mut views = vec![];
    for model in doc.ids().filter(|&id| {
        is(doc, id, "draughting_model")
            || is(
                doc,
                id,
                "mechanical_design_geometric_presentation_representation",
            )
    }) {
        let members = descendants(doc, [model]);
        let mut cameras: BTreeSet<_> = members
            .iter()
            .copied()
            .filter(|&id| is(doc, id, "camera_model_d3"))
            .collect();
        for id in doc.ids().filter(|&id| is(doc, id, "model_geometric_view")) {
            if reference(attr(
                doc,
                id,
                "characterized_item_within_representation",
                "rep",
            )) == Some(model)
                && let Some(camera) = reference(attr(
                    doc,
                    id,
                    "characterized_item_within_representation",
                    "item",
                ))
            {
                cameras.insert(camera);
            }
        }
        let linked: Vec<_> = graphics
            .iter()
            .filter(|(id, _)| {
                id.strip_prefix("step:")
                    .and_then(|id| id.parse::<u64>().ok())
                    .is_some_and(|root| members.contains(&root))
            })
            .map(|(id, _)| (*id).clone())
            .collect();
        let name = text(attr(doc, model, "representation", "name"));
        if cameras.is_empty() {
            views.push(View {
                id: format!("step:{model}"),
                name,
                direction: None,
                up: None,
                graphics: linked,
            });
        } else {
            for camera in cameras {
                let frame = reference(attr(
                    doc,
                    camera,
                    "camera_model_d3",
                    "view_reference_system",
                ))
                .and_then(|id| placement(doc, id));
                let camera_name = text(attr(doc, camera, "representation_item", "name"));
                views.push(View {
                    id: format!("step:{model}/camera:{camera}"),
                    name: if camera_name.is_empty() {
                        name.clone()
                    } else {
                        camera_name
                    },
                    direction: frame.map(|p| geom::scale(p.axes[2], -1.)),
                    up: frame.map(|p| p.axes[1]),
                    graphics: linked.clone(),
                });
            }
        }
    }
    views
}

#[cfg(feature = "presentation")]
impl Plane {
    pub(super) fn generated(
        graphic: &Graphic,
        placed: &crate::presentation::placement::Placed,
    ) -> Self {
        Self {
            id: format!("{}/plane", graphic.id.as_deref().unwrap_or("generated")),
            name: graphic.name.clone(),
            origin: if placed.origin.iter().all(|n| n.is_finite()) {
                placed.origin
            } else {
                placed.anchor
            },
            x_axis: placed.along,
            normal: placed.normal,
        }
    }
}
#[cfg(feature = "presentation")]
pub(super) fn generated(
    graphic: &mut Graphic,
    callout: &crate::presentation::Callout,
    placed: &crate::presentation::placement::Placed,
) {
    let scale = placed.text_scale;
    if !placed.origin.iter().all(|n| n.is_finite()) {
        graphic
            .unsupported
            .push("generated text placement unavailable for empty ink".into());
        return;
    }
    let up = geom::cross(placed.normal, placed.along);
    let origin = placed.text_origin;
    let mut used = BTreeSet::new();
    for run in &callout.runs {
        if let Some((index, _)) = callout
            .cells
            .iter()
            .enumerate()
            .find(|(i, text)| !used.contains(i) && **text == run.text)
        {
            used.insert(index);
            graphic.text_placements.push(TextPlacement {
                text_index: Some(index),
                cell_box: Some({
                    let b = callout.cell_boxes.get(index).copied().unwrap_or_else(|| {
                        let ink = crate::presentation::font().text(&run.text, run.size);
                        [
                            [run.at[0] + ink.min[0] - 0.6, run.at[1] + ink.min[1] - 0.6],
                            [run.at[0] + ink.max[0] + 0.6, run.at[1] + ink.max[1] + 0.6],
                        ]
                    });
                    b.map(|p| [(p[0] - run.at[0]) * scale, (p[1] - run.at[1]) * scale])
                }),
                // `placed.lines` contains Callout::polylines(): font letters
                // and explicit geometric symbols represented by these runs.
                // This records the rendering operation, not Graphic::source.
                stroked: Some(true),
                origin: geom::add(
                    origin,
                    geom::add(
                        geom::scale(placed.along, run.at[0] * scale),
                        geom::scale(up, run.at[1] * scale),
                    ),
                ),
                x_axis: placed.along,
                normal: placed.normal,
                height: Some(run.size * scale),
                line_spacing: None,
                alignment: Some("centre".into()),
            });
        }
    }
}

#[cfg(feature = "json-import")]
pub(super) fn validate(value: &Value) -> Result<()> {
    let mut planes = BTreeSet::new();
    for plane in value["planes"].as_array().into_iter().flatten() {
        if !planes.insert(plane["id"].as_str().unwrap_or_default()) {
            return Err(Error("duplicate presentation plane id".into()));
        }
    }
    let mut graphics = BTreeMap::new();
    for graphic in value["items"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .flat_map(|v| v.as_array().into_iter().flatten())
        .chain(value["unlinked"].as_array().into_iter().flatten())
    {
        if let Some(id) = graphic["id"].as_str()
            && let Some(before) = graphics.insert(id, graphic)
            && before != graphic
        {
            return Err(Error(format!(
                "graphic id {id:?} has inconsistent semantic-key aliases"
            )));
        }
        if let Some(plane) = graphic["plane_id"].as_str()
            && !planes.contains(plane)
        {
            return Err(Error(format!("unknown presentation plane {plane:?}")));
        }
        if let Some(roles) = graphic["polyline_roles"].as_array()
            && roles.len() != graphic["polylines"].as_array().map_or(0, Vec::len)
        {
            return Err(Error("polyline_roles must be parallel to polylines".into()));
        }
        for placed in graphic["text_placements"].as_array().into_iter().flatten() {
            if let Some(index) = super::super::schema::unsigned(&placed["text_index"])
                && index >= graphic["text"].as_array().map_or(0, Vec::len) as u64
            {
                return Err(Error("text placement index out of range".into()));
            }
            if let Some(b) = placed["cell_box"].as_array()
                && (0..2).any(|k| {
                    b[0][k].as_f64().unwrap_or(f64::NAN) > b[1][k].as_f64().unwrap_or(f64::NAN)
                })
            {
                return Err(Error("inverted text cell_box".into()));
            }
            for field in ["height", "line_spacing"] {
                if placed[field]
                    .as_f64()
                    .is_some_and(|height| height < 0. || (field == "height" && height == 0.))
                {
                    return Err(Error(format!("invalid text {field}")));
                }
            }
        }
    }
    let mut views = BTreeSet::new();
    for view in value["views"].as_array().into_iter().flatten() {
        if !views.insert(view["id"].as_str().unwrap_or_default()) {
            return Err(Error("duplicate presentation view id".into()));
        }
        if view.get("direction").is_some() != view.get("up").is_some() {
            return Err(Error(
                "view direction and up must both be present or absent".into(),
            ));
        }
        for graphic in view["graphics"].as_array().into_iter().flatten() {
            if !graphics.contains_key(graphic.as_str().unwrap_or_default()) {
                return Err(Error(format!("view refers to unknown graphic {graphic}")));
            }
        }
    }
    Ok(())
}
