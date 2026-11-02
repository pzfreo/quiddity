//! Read file-owned callouts without regenerating their shape or dropping unlinked graphics.
use super::{Error, Result, Semantic};
use crate::{
    express, geom,
    p21::{Attribute as A, Document, RawEntity},
    pmi::{PartId, PmiRead},
};
use serde::Serialize;
#[cfg(feature = "json-import")]
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[path = "presentation_layout.rs"]
mod layout;
pub use layout::{Plane, TextPlacement, View};

/// Geometry purpose; dimension includes arrowheads, extension includes witnesses.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[serde(rename_all = "snake_case")]
pub enum PolylineRole {
    Text,
    Frame,
    Leader,
    Extension,
    Symbol,
    Dimension,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[serde(deny_unknown_fields)]
pub struct Graphic {
    /// Document-local identity, shared by copies under multiple semantic keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_placements: Vec<TextPlacement>,
    /// Optional model-space leader tip, in millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<[f64; 3]>,
    pub source: Source,
    pub name: String,
    pub polylines: Vec<Vec<[f64; 3]>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polyline_roles: Option<Vec<PolylineRole>>,
    pub triangles: Vec<[[f64; 3]; 3]>,
    pub text: Vec<String>,
    pub step_ids: Vec<u64>,
    pub unsupported: Vec<String>,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Generated,
    File,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(feature = "json-import", derive(serde::Deserialize))]
#[serde(deny_unknown_fields)]
pub struct Presentation {
    pub schema: String,
    pub length_unit: String,
    pub semantic_ids: Vec<String>,
    pub items: BTreeMap<String, Vec<Graphic>>,
    pub unlinked: Vec<Graphic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planes: Vec<Plane>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<View>,
}
impl Default for Presentation {
    fn default() -> Self {
        Self {
            schema: "haecceity.presentation/1".into(),
            length_unit: "millimetre".into(),
            semantic_ids: vec![],
            items: BTreeMap::new(),
            unlinked: vec![],
            planes: vec![],
            views: vec![],
        }
    }
}
/// Decode a companion while retaining the semantic owner in malformed-item errors.
#[cfg(feature = "json-import")]
pub fn from_value(mut value: Value) -> Result<Presentation> {
    let items = value
        .get_mut("items")
        .map(|items| std::mem::replace(items, serde_json::json!({})));
    let mut presentation: Presentation = serde_json::from_value(value)?;
    if let Some(items) = items {
        let Value::Object(items) = items else {
            return Err(Error("presentation items must be an object".into()));
        };
        for (id, graphics) in items {
            let graphics = serde_json::from_value(graphics)
                .map_err(|e| Error(format!("presentation item {id}: {e}")))?;
            presentation.items.insert(id, graphics);
        }
    }
    Ok(presentation)
}

impl Graphic {
    fn new(source: Source, name: String) -> Self {
        Self {
            id: None,
            plane_id: None,
            text_placements: vec![],
            leader: None,
            source,
            name,
            polylines: vec![],
            polyline_roles: None,
            triangles: vec![],
            text: vec![],
            step_ids: vec![],
            unsupported: vec![],
        }
    }
}

fn names(doc: &Document, id: u64) -> Vec<String> {
    match doc.get(id) {
        Some(RawEntity::Simple { name, .. }) => vec![name.to_ascii_lowercase()],
        Some(RawEntity::Complex { parts, .. }) => {
            parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
        }
        None => vec![],
    }
}
pub(super) fn is(doc: &Document, id: u64, t: &str) -> bool {
    names(doc, id)
        .iter()
        .any(|n| express::TARGET.is_a(n, t) || express::AP242_ED1.is_a(n, t))
}
pub(super) fn attr<'a>(doc: &'a Document, id: u64, entity: &str, name: &str) -> Option<&'a A> {
    match doc.get(id)? {
        RawEntity::Simple {
            name: ty,
            attributes,
            ..
        } => {
            let slots = express::TARGET
                .explicit_attributes(ty)
                .or_else(|| express::AP242_ED1.explicit_attributes(ty))?;
            attributes.get(
                slots
                    .iter()
                    .position(|s| s.entity == entity && s.name == name)?,
            )
        }
        RawEntity::Complex { parts, .. } => {
            let p = parts.iter().find(|p| p.name.eq_ignore_ascii_case(entity))?;
            let decl = express::TARGET
                .entity(entity)
                .or_else(|| express::AP242_ED1.entity(entity))?;
            p.attributes
                .get(decl.attributes.iter().position(|a| a.name == name)?)
        }
    }
}
pub(super) fn reference(a: Option<&A>) -> Option<u64> {
    if let Some(A::EntityRef(id)) = a {
        Some(*id)
    } else {
        None
    }
}
fn list(a: Option<&A>) -> &[A] {
    if let Some(A::List(v)) = a { v } else { &[] }
}
fn refs(a: Option<&A>) -> Vec<u64> {
    list(a).iter().filter_map(|a| reference(Some(a))).collect()
}
fn text(a: Option<&A>) -> String {
    if let Some(A::String(s)) = a {
        s.clone()
    } else {
        String::new()
    }
}
fn number(a: &A) -> Option<f64> {
    match a {
        A::Real(n) => Some(*n),
        A::Integer(n) => Some(*n as f64),
        _ => None,
    }
}
fn xyz(a: Option<&A>) -> Option<[f64; 3]> {
    let v = list(a);
    if v.len() != 3 {
        return None;
    }
    Some([number(&v[0])?, number(&v[1])?, number(&v[2])?])
}
fn point(doc: &Document, id: u64) -> Option<[f64; 3]> {
    xyz(attr(doc, id, "cartesian_point", "coordinates"))
}
fn direction(doc: &Document, id: u64) -> Option<[f64; 3]> {
    geom::unit(xyz(attr(doc, id, "direction", "direction_ratios"))?)
}
#[derive(Clone, Copy)]
struct Transform {
    origin: [f64; 3],
    axes: [[f64; 3]; 3],
}
impl Transform {
    fn identity() -> Self {
        Self {
            origin: [0.; 3],
            axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        }
    }
    fn at(self, p: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            self.origin[i] + (0..3).map(|j| self.axes[j][i] * p[j]).sum::<f64>()
        })
    }
    fn then(self, child: Self) -> Self {
        Self {
            origin: self.at(child.origin),
            axes: child
                .axes
                .map(|a| std::array::from_fn(|i| (0..3).map(|j| self.axes[j][i] * a[j]).sum())),
        }
    }
}
fn placement(doc: &Document, id: u64) -> Option<Transform> {
    let location = reference(attr(doc, id, "placement", "location"))?;
    let origin = if is(doc, id, "axis2_placement_2d") {
        let coordinates = list(attr(doc, location, "cartesian_point", "coordinates"));
        [
            number(coordinates.first()?)?,
            number(coordinates.get(1)?)?,
            0.,
        ]
    } else if is(doc, id, "axis2_placement_3d") {
        point(doc, location)?
    } else {
        return None;
    };
    if is(doc, id, "axis2_placement_2d") {
        let x = reference(attr(doc, id, "axis2_placement_2d", "ref_direction"))
            .and_then(|r| {
                let ratios = list(attr(doc, r, "direction", "direction_ratios"));
                geom::unit([number(ratios.first()?)?, number(ratios.get(1)?)?, 0.])
            })
            .unwrap_or([1., 0., 0.]);
        return Some(Transform {
            origin,
            axes: [x, [-x[1], x[0], 0.], [0., 0., 1.]],
        });
    }
    let z = reference(attr(doc, id, "axis2_placement_3d", "axis"))
        .and_then(|r| direction(doc, r))
        .unwrap_or([0., 0., 1.]);
    let proposed = reference(attr(doc, id, "axis2_placement_3d", "ref_direction"))
        .and_then(|r| direction(doc, r))
        .unwrap_or([1., 0., 0.]);
    let x = geom::unit(geom::sub(proposed, geom::scale(z, geom::dot(proposed, z))))?;
    let y = geom::cross(z, x);
    Some(Transform {
        origin,
        axes: [x, y, z],
    })
}
fn children(doc: &Document, id: u64) -> Vec<u64> {
    if is(doc, id, "composite_text") {
        return refs(attr(doc, id, "composite_text", "collected_text"));
    }
    if is(doc, id, "mapped_item") {
        return reference(attr(doc, id, "mapped_item", "mapping_source"))
            .and_then(|r| reference(attr(doc, r, "representation_map", "mapped_representation")))
            .map(|r| refs(attr(doc, r, "representation", "items")))
            .unwrap_or_default();
    }
    if is(doc, id, "draughting_callout") {
        return refs(attr(doc, id, "draughting_callout", "contents"));
    }
    if is(doc, id, "annotation_plane") {
        return refs(attr(doc, id, "annotation_plane", "elements"));
    }
    if is(doc, id, "annotation_occurrence") {
        return reference(attr(doc, id, "styled_item", "item"))
            .into_iter()
            .collect();
    }
    if is(doc, id, "tessellated_geometric_set") {
        return refs(attr(doc, id, "tessellated_geometric_set", "children"));
    }
    if is(doc, id, "geometric_set") {
        return refs(attr(doc, id, "geometric_set", "elements"));
    }
    vec![]
}
fn coordinates(doc: &Document, id: u64, owner: &str) -> Vec<Option<[f64; 3]>> {
    reference(attr(doc, id, owner, "coordinates"))
        .map(|r| {
            list(attr(doc, r, "coordinates_list", "position_coords"))
                .iter()
                .map(|a| xyz(Some(a)))
                .collect()
        })
        .unwrap_or_default()
}
fn indices(a: &A) -> Vec<usize> {
    list(Some(a))
        .iter()
        .map(|v| {
            // Keep every position: deleting a bad index would join different points.
            if let A::Integer(n) = v {
                usize::try_from(*n)
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .unwrap_or(usize::MAX)
            } else {
                usize::MAX
            }
        })
        .collect()
}
fn draw(
    doc: &Document,
    id: u64,
    transform: Transform,
    scale: f64,
    g: &mut Graphic,
    seen: &mut BTreeSet<u64>,
    context: layout::TextContext,
) {
    if context.depth > 64 {
        g.unsupported
            .push(format!("#{id}: presentation nesting exceeds 64"));
        return;
    }
    if !seen.insert(id) {
        g.unsupported
            .push(format!("#{id}: cyclic or shared content path"));
        return;
    }
    g.step_ids.push(id);
    let transform =
        if let Some(loc) = reference(attr(doc, id, "repositioned_tessellated_item", "location")) {
            match placement(doc, loc) {
                Some(p) => transform.then(p),
                None => {
                    g.unsupported
                        .push(format!("#{id}: unresolved repositioned placement"));
                    return;
                }
            }
        } else {
            transform
        };
    let transform = if is(doc, id, "mapped_item") {
        match layout::mapping(doc, id) {
            Some(mapping) => transform.then(mapping),
            None => {
                g.unsupported
                    .push(format!("#{id}: unresolved mapped presentation placement"));
                seen.remove(&id);
                return;
            }
        }
    } else {
        transform
    };
    let context = context.at(doc, id);
    if is(doc, id, "annotation_placeholder_occurrence") {
        layout::placeholder(doc, id, transform, scale, g);
        seen.remove(&id);
        return;
    }
    let at = |p| geom::scale(transform.at(p), scale);
    let kids = children(doc, id);
    if !kids.is_empty() {
        for r in kids {
            draw(doc, r, transform, scale, g, seen, context.descend());
        }
        seen.remove(&id);
        return;
    }
    if is(doc, id, "polyline") {
        let points = refs(attr(doc, id, "polyline", "points"));
        let line: Option<Vec<_>> = points.iter().map(|r| point(doc, *r).map(at)).collect();
        if let Some(line) = line {
            if text(attr(doc, id, "representation_item", "name")) == "haecceity leader" {
                g.leader = line.first().copied();
            }
            g.polylines.push(line);
        } else {
            g.unsupported
                .push(format!("#{id}: unresolved polyline point"));
        }
    } else if is(doc, id, "tessellated_curve_set") {
        let coords = coordinates(doc, id, "tessellated_curve_set");
        for strip in list(attr(doc, id, "tessellated_curve_set", "line_strips")) {
            let line: Option<Vec<_>> = indices(strip)
                .iter()
                .map(|i| coords.get(*i).copied().flatten().map(at))
                .collect();
            if let Some(line) = line {
                g.polylines.push(line);
            } else {
                g.unsupported
                    .push(format!("#{id}: curve index out of range"));
            }
        }
    } else if is(doc, id, "tessellated_surface_set") || is(doc, id, "tessellated_face") {
        let owner = if is(doc, id, "tessellated_surface_set") {
            "tessellated_surface_set"
        } else {
            "tessellated_face"
        };
        let coords = coordinates(doc, id, owner);
        let complex = if is(doc, id, "complex_triangulated_surface_set") {
            "complex_triangulated_surface_set"
        } else {
            "complex_triangulated_face"
        };
        let simple = if owner == "tessellated_face" {
            "triangulated_face"
        } else {
            "triangulated_surface_set"
        };
        let mut triangles = Vec::new();
        let pn = indices(
            attr(doc, id, complex, "pnindex")
                .or_else(|| attr(doc, id, simple, "pnindex"))
                .unwrap_or(&A::Unset),
        );
        for (field, fan) in [("triangle_strips", false), ("triangle_fans", true)] {
            for row in list(attr(doc, id, complex, field)) {
                let row = indices(row);
                for i in 2..row.len() {
                    triangles.push(if fan {
                        [row[0], row[i - 1], row[i]]
                    } else if i % 2 == 0 {
                        [row[i - 2], row[i - 1], row[i]]
                    } else {
                        [row[i - 1], row[i - 2], row[i]]
                    });
                }
            }
        }
        for row in list(
            attr(doc, id, simple, "triangles").or_else(|| attr(doc, id, simple, "coord_index")),
        ) {
            let row = indices(row);
            if let [a, b, c] = row.as_slice() {
                triangles.push([*a, *b, *c]);
            }
        }
        for tri in triangles {
            let vertices: Option<Vec<_>> = tri
                .iter()
                .map(|i| {
                    let i = if pn.is_empty() {
                        Some(*i)
                    } else {
                        pn.get(*i).copied()
                    };
                    i.and_then(|i| coords.get(i)).copied().flatten().map(at)
                })
                .collect();
            if let Some(v) = vertices {
                g.triangles.push([v[0], v[1], v[2]]);
            } else {
                g.unsupported
                    .push(format!("#{id}: triangle index out of range"));
            }
        }
    } else if is(doc, id, "text_literal") {
        layout::literal(doc, id, transform, scale, context, g);
        g.unsupported.push(format!(
            "#{id}: editable literal retained; font glyph outlines unavailable"
        ));
    } else {
        g.unsupported.push(format!(
            "#{id}: unsupported graphic {}",
            names(doc, id).join("+")
        ));
    }
    seen.remove(&id);
}

/// Read the file's own annotations. The semantic item map is for one part; file
/// graphics without a link to that part are retained in `unlinked`.
pub fn read(
    doc: &Document,
    read: &PmiRead,
    part: PartId,
    semantic: &Semantic,
) -> Result<Presentation> {
    if part.0 >= read.parts.len() {
        return Err(Error(format!("unknown part {}", part.0)));
    }
    let mut roots: BTreeSet<_> = doc
        .ids()
        .filter(|id| is(doc, *id, "draughting_callout"))
        .collect();
    let mut contained = BTreeSet::new();
    for &root in &roots {
        let mut stack = children(doc, root);
        while let Some(id) = stack.pop() {
            if contained.insert(id) {
                stack.extend(children(doc, id));
            }
        }
    }
    roots.extend(doc.ids().filter(|id| {
        is(doc, *id, "annotation_occurrence")
            && !is(doc, *id, "annotation_plane")
            && !contained.contains(id)
    }));
    let mut carriers = BTreeMap::<u64, BTreeSet<String>>::new();
    for ((p, c, i), entity) in &read.json_carriers {
        if *p == part
            && let Some(id) = semantic.ids.get(c).and_then(|ids| ids.get(*i))
        {
            carriers.entry(*entity).or_default().insert(id.clone());
        }
    }
    let mut datum_support: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    for (i, datum) in semantic.pmi.datums.iter().enumerate() {
        if let Some(feature) = datum.feature()
            && let Some(carrier) = read
                .json_carriers
                .get(&(part, "features".into(), feature.0))
            && let Some(id) = semantic.ids.get("datums").and_then(|ids| ids.get(i))
        {
            datum_support
                .entry(*carrier)
                .or_default()
                .insert(id.clone());
        }
    }
    // Some features project onto a different native carrier. Resolve the actual
    // datum-feature -> datum relationship rather than guessing by equal faces.
    for relationship in doc.ids().filter(|&id| {
        is(doc, id, "shape_aspect_relationship") && !is(doc, id, "dimensional_location")
    }) {
        let feature = reference(attr(
            doc,
            relationship,
            "shape_aspect_relationship",
            "relating_shape_aspect",
        ));
        let datum = reference(attr(
            doc,
            relationship,
            "shape_aspect_relationship",
            "related_shape_aspect",
        ));
        if let (Some(feature), Some(datum)) = (feature, datum)
            && is(doc, feature, "datum_feature")
            && is(doc, datum, "datum")
            && let Some(keys) = carriers.get(&datum)
        {
            datum_support
                .entry(feature)
                .or_default()
                .extend(keys.iter().cloned());
        }
    }
    let mut support_links: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    let mut links: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    for id in doc
        .ids()
        .filter(|id| is(doc, *id, "draughting_model_item_association"))
    {
        let definition = reference(attr(
            doc,
            id,
            "item_identified_representation_usage",
            "definition",
        ));
        let item = reference(attr(
            doc,
            id,
            "item_identified_representation_usage",
            "identified_item",
        ));
        if let (Some(definition), Some(item)) = (definition, item) {
            if let Some(keys) = carriers.get(&definition) {
                links.entry(item).or_default().extend(keys.iter().cloned());
            }
            if let Some(keys) = datum_support.get(&definition) {
                support_links
                    .entry(item)
                    .or_default()
                    .extend(keys.iter().cloned());
            }
        }
    }
    for (i, callouts) in read.provenance.parts[part.0]
        .note_callouts
        .iter()
        .enumerate()
    {
        if let Some(key) = semantic.ids.get("notes").and_then(|ids| ids.get(i)) {
            for &root in callouts {
                links.entry(root).or_default().insert(key.clone());
            }
        }
    }
    let mut out = Presentation {
        semantic_ids: id_set(semantic)?,
        ..Presentation::default()
    };
    let generated: BTreeSet<_> = doc
        .ids()
        .filter(|&id| {
            is(doc, id, "draughting_model_item_association")
                && text(attr(
                    doc,
                    id,
                    "item_identified_representation_usage",
                    "description",
                )) == "haecceity.generated/1"
        })
        .filter_map(|id| {
            reference(attr(
                doc,
                id,
                "item_identified_representation_usage",
                "identified_item",
            ))
        })
        .collect();
    let plane_index = layout::PlaneIndex::read(doc);
    out.planes = plane_index.planes.clone();
    for root in roots {
        let mut graphic = Graphic::new(
            if generated.contains(&root)
                || children(doc, root)
                    .iter()
                    .any(|child| generated.contains(child))
            {
                Source::Generated
            } else {
                Source::File
            },
            text(attr(doc, root, "representation_item", "name")),
        );
        graphic.id = Some(format!("step:{root}"));
        let context = plane_index.context(root, &mut graphic);
        let scale = scale_for(doc, root);
        draw(
            doc,
            root,
            Transform::identity(),
            scale.unwrap_or(1.),
            &mut graphic,
            &mut BTreeSet::new(),
            context,
        );
        if scale.is_none() {
            graphic.polylines.clear();
            graphic.triangles.clear();
            graphic.text_placements.clear();
            graphic.plane_id = None;
            graphic.unsupported.push(format!(
                "#{root}: length unit unresolved; source retained without guessed coordinates"
            ));
        }
        let mut keys: BTreeSet<_> = graphic
            .step_ids
            .iter()
            .filter_map(|id| links.get(id))
            .flatten()
            .cloned()
            .collect();
        {
            keys.extend(
                graphic
                    .step_ids
                    .iter()
                    .filter_map(|id| support_links.get(id))
                    .flatten()
                    .cloned(),
            );
        }
        graphic.step_ids.sort_unstable();
        graphic.step_ids.dedup();
        graphic.unsupported.sort();
        graphic.unsupported.dedup();
        if keys.is_empty() {
            out.unlinked.push(graphic);
        } else {
            for key in keys {
                out.items.entry(key).or_default().push(graphic.clone());
            }
        }
    }
    out.views = layout::views(doc, &out);
    #[cfg(feature = "json-import")]
    validate(&serde_json::to_value(&out)?)?;
    Ok(out)
}

fn scale_for(doc: &Document, id: u64) -> Option<f64> {
    let mut stack = vec![id];
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if is(doc, id, "representation") {
            let context = reference(attr(doc, id, "representation", "context_of_items"))?;
            for unit in refs(attr(doc, context, "global_unit_assigned_context", "units")) {
                if let Some(s) = unit_scale(doc, unit, 0) {
                    return Some(s);
                }
            }
        }
        for &parent in doc.referrers(id) {
            if is(doc, parent, "representation") || !children(doc, parent).is_empty() {
                stack.push(parent);
            }
        }
    }
    None
}
fn unit_scale(doc: &Document, id: u64, depth: usize) -> Option<f64> {
    if depth > 8 || !is(doc, id, "length_unit") {
        return None;
    }
    if is(doc, id, "si_unit") {
        let prefix = match attr(doc, id, "si_unit", "prefix") {
            Some(A::Enum(p)) => p.to_ascii_lowercase(),
            _ => String::new(),
        };
        return match prefix.as_str() {
            "" => Some(1000.),
            "milli" => Some(1.),
            "centi" => Some(10.),
            "micro" => Some(0.001),
            "deci" => Some(100.),
            "kilo" => Some(1e6),
            _ => None,
        };
    }
    let conversion = reference(attr(doc, id, "conversion_based_unit", "conversion_factor"))?;
    let value = attr(doc, conversion, "measure_with_unit", "value_component")?;
    let value = if let A::Typed { value, .. } = value {
        number(value)?
    } else {
        number(value)?
    };
    Some(
        value
            * unit_scale(
                doc,
                reference(attr(doc, conversion, "measure_with_unit", "unit_component"))?,
                depth + 1,
            )?,
    )
}

/// Validate the companion with the shipped schema.
#[cfg(feature = "json-import")]
pub fn validate(v: &Value) -> Result<()> {
    let schema: Value = serde_json::from_str(super::PRESENTATION_SCHEMA)?;
    super::schema::check(v, &schema, &schema, "")?;
    layout::validate(v)
}

/// Generate current notation placed on the part's geometry in model millimetres.
#[cfg(feature = "presentation")]
pub fn generated(semantic: &Semantic, part: &crate::brep::Part) -> Result<Presentation> {
    use crate::presentation;
    let mut out = Presentation {
        semantic_ids: id_set(semantic)?,
        ..Presentation::default()
    };
    let mut layout = presentation::placement::Layout::for_views(part.bounds());
    let measurements = presentation::linear::measurements_for_part(part, &semantic.pmi);
    let mut placed = BTreeMap::new();
    // Datums are attached first. Linear numbers must not consume bare-label
    // slots which displace the feature flags before the ladder is measured.
    for (i, d) in semantic.pmi.datums.iter().enumerate() {
        let c = presentation::datum(d.label().as_str());
        let p = if let Some((point, normal, _)) = d
            .feature()
            .and_then(|f| presentation::placement::feature_site(part, &semantic.pmi, f))
        {
            layout.on_face(point, normal, &c)
        } else {
            layout.notes(&c)
        };
        placed.insert(("datums", i), (c, p));
    }
    // A feature's size and controls are one drawing stack. Independently
    // minimizing each leader can choose different planes and superimpose ink.
    let mut stacks = BTreeMap::<_, Vec<_>>::new();
    for (i, d) in semantic.pmi.dimensions.iter().enumerate() {
        if measurements[i].is_none()
            && d.nominal.is_some()
            && let crate::pmi::DimensionKind::Size { feature, .. } = d.kind
        {
            stacks.entry(feature.0).or_default().push((
                "dimensions",
                i,
                presentation::dimension(d),
            ));
        }
    }
    for (i, t) in semantic.pmi.threads.iter().enumerate() {
        stacks.entry(t.feature.0).or_default().push((
            "threads",
            i,
            presentation::thread(t, &semantic.pmi),
        ));
    }
    for (i, t) in semantic.pmi.tolerances.iter().enumerate() {
        let feature = match t.target {
            crate::pmi::ToleranceTarget::Feature(f)
            | crate::pmi::ToleranceTarget::Relation { relating: f, .. } => Some(f),
            crate::pmi::ToleranceTarget::Dimension(d) => match semantic.pmi.dimensions[d.0].kind {
                crate::pmi::DimensionKind::Size { feature, .. } => Some(feature),
                _ => None,
            },
            _ => None,
        };
        if let Some(f) = feature {
            stacks.entry(f.0).or_default().push((
                "tolerances",
                i,
                presentation::tolerance(t, &semantic.pmi),
            ));
        }
    }
    for (feature, stack) in stacks {
        if stack.len() < 2 {
            continue;
        }
        if let Some((point, normal, face)) = presentation::placement::feature_site(
            part,
            &semantic.pmi,
            crate::pmi::FeatureId(feature),
        ) {
            let callouts: Vec<_> = stack.iter().map(|(_, _, c)| c).collect();
            let positions = layout.stack(
                point,
                normal,
                &callouts,
                part.face_bounds(face).max_extent(),
            );
            for ((collection, i, c), p) in stack.into_iter().zip(positions) {
                placed.insert((collection, i), (c, p));
            }
        }
    }
    for (i, d) in semantic.pmi.dimensions.iter().enumerate() {
        if measurements[i].is_some() || placed.contains_key(&("dimensions", i)) {
            continue;
        }
        let c = presentation::dimension(d);
        let f = match d.kind {
            crate::pmi::DimensionKind::Size { feature, .. } => feature,
            crate::pmi::DimensionKind::Location { from, .. } => from,
        };
        let p = if let Some((point, normal, face)) =
            presentation::placement::feature_site(part, &semantic.pmi, f)
        {
            layout.on_label(point, normal, &c, part.face_bounds(face).max_extent())
        } else {
            layout.notes(&c)
        };
        placed.insert(("dimensions", i), (c, p));
    }
    for (i, t) in semantic.pmi.tolerances.iter().enumerate() {
        if placed.contains_key(&("tolerances", i)) {
            continue;
        }
        use crate::pmi::{DimensionKind, ToleranceTarget};
        let f = match t.target {
            ToleranceTarget::Feature(f) | ToleranceTarget::Relation { relating: f, .. } => Some(f),
            ToleranceTarget::Dimension(d) => Some(match semantic.pmi.dimensions[d.0].kind {
                DimensionKind::Size { feature, .. } => feature,
                DimensionKind::Location { from, .. } => from,
            }),
            ToleranceTarget::WholePart => None,
        };
        let c = presentation::tolerance(t, &semantic.pmi);
        let p = if let Some((point, normal, face)) =
            f.and_then(|f| presentation::placement::feature_site(part, &semantic.pmi, f))
        {
            let extent = part.face_bounds(face).max_extent();
            let mut candidate = layout.clone();
            let radial = candidate.on_control(point, normal, &c, extent);
            let (p, chosen) = match part.faces[face].surface {
                geom::Surface::Cylinder { frame, .. }
                | geom::Surface::Cone { frame, .. }
                | geom::Surface::Torus { frame, .. } => {
                    let axis = geom::scale(
                        frame.z,
                        if geom::dot(geom::sub(point, part.bounds().centre()), frame.z) < 0. {
                            -1.
                        } else {
                            1.
                        },
                    );
                    let mut axial_layout = layout.clone();
                    let axial = axial_layout.on_axis(point, axis, &c, extent);
                    let distance = |p: &presentation::placement::Placed| {
                        let run = &c.runs[0];
                        geom::dist(
                            point,
                            geom::add(
                                p.text_origin,
                                geom::add(
                                    geom::scale(p.along, run.at[0] * p.text_scale),
                                    geom::scale(
                                        geom::cross(p.normal, p.along),
                                        run.at[1] * p.text_scale,
                                    ),
                                ),
                            ),
                        )
                    };
                    if distance(&axial) < distance(&radial) {
                        (axial, axial_layout)
                    } else {
                        (radial, candidate)
                    }
                }
                _ => (radial, candidate),
            };
            layout = chosen;
            p
        } else {
            layout.notes(&c)
        };
        placed.insert(("tolerances", i), (c, p));
    }
    for (i, t) in semantic.pmi.threads.iter().enumerate() {
        if placed.contains_key(&("threads", i)) {
            continue;
        }
        let c = presentation::thread(t, &semantic.pmi);
        let p = if let Some((point, normal, face)) =
            presentation::placement::feature_site(part, &semantic.pmi, t.feature)
        {
            layout.on_label(point, normal, &c, part.face_bounds(face).max_extent())
        } else {
            layout.notes(&c)
        };
        placed.insert(("threads", i), (c, p));
    }
    let mut ladder = presentation::linear::Ladder::for_views(part.bounds());
    for (_, p) in placed.values() {
        let mut b = geom::Bounds::empty();
        for &q in p
            .lines
            .iter()
            .take(p.lines.len().saturating_sub(1))
            .flatten()
        {
            b.add(q);
        }
        ladder.reserve_dimension(b, p.normal, p.along);
    }
    let mut order: Vec<_> = measurements
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.as_ref().map(|m| (i, m)))
        .collect();
    order.sort_by(|a, b| a.1.length().total_cmp(&b.1.length()).then(a.0.cmp(&b.0)));
    let mut directions: Vec<(geom::V3, geom::V3)> = Vec::new();
    for (i, m) in order {
        let c = presentation::dimension(&semantic.pmi.dimensions[i]);
        let reference = geom::scale(geom::add(m.feet[0], m.feet[1]), 0.5);
        let outward = directions
            .iter()
            .find(|(d, _)| geom::dot(*d, m.direction) > 1. - 1e-8)
            .map(|(_, v)| {
                if geom::dot(geom::sub(reference, part.bounds().centre()), *v) < 0. {
                    geom::scale(*v, -1.)
                } else {
                    *v
                }
            });
        let mut candidates = vec![outward.unwrap_or(m.outward)];
        if outward.is_none() {
            for axis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
                if let Some(v) = geom::unit(geom::sub(
                    axis,
                    geom::scale(m.direction, geom::dot(axis, m.direction)),
                )) {
                    candidates.extend([v, geom::scale(v, -1.)]);
                }
            }
        }
        let mut best = None;
        for outward in candidates {
            let cross = geom::unit(geom::cross(m.direction, outward))
                .ok_or_else(|| Error("degenerate annotation plane".into()))?;
            let mut normal = cross;
            let dominant = geom::dominant_axis_preferring_z(normal);
            if (dominant == 1 && normal[dominant] > 0.) || (dominant != 1 && normal[dominant] < 0.)
            {
                normal = geom::scale(normal, -1.);
            }
            // Prefer the normal front/top/side camera on a tie; also allow the
            // opposite outer plane when the actual feature lies nearer that side.
            for normal in [normal, geom::scale(normal, -1.)] {
                let mut candidate = ladder.clone();
                let mut measurement = m.clone();
                measurement.outward = outward;
                let p = candidate
                    .draw_in_view(&measurement, &c, Some(normal))
                    .placed;
                let distance = geom::dist(p.origin, reference);
                if best
                    .as_ref()
                    .is_none_or(|(d, _, _, _)| distance < *d - 1e-8)
                {
                    best = Some((distance, p, candidate, outward));
                }
            }
        }
        let (_, p, chosen, outward) =
            best.ok_or_else(|| Error("no linear annotation plane".into()))?;
        ladder = chosen;
        if directions
            .iter()
            .all(|(d, _)| geom::dot(*d, m.direction) <= 1. - 1e-8)
        {
            directions.push((m.direction, outward));
        }
        layout.reserve(&p.lines);
        placed.insert(("dimensions", i), (c, p));
    }
    let mut add =
        |collection: &'static str, index: usize, callout: presentation::Callout| -> Result<()> {
            let key = semantic
                .ids
                .get(collection)
                .and_then(|ids| ids.get(index))
                .ok_or_else(|| Error(format!("missing id {collection}/{index}")))?;
            let placed = if let Some((_, p)) = placed.remove(&(collection, index)) {
                p
            } else if collection == "surface_textures" {
                let t = &semantic.pmi.surface_textures[index];
                if let Some((_, _, face)) =
                    t.on.and_then(|f| presentation::placement::feature_site(part, &semantic.pmi, f))
                {
                    layout
                        .surface(
                            &presentation::placement::surface_sites(part, face),
                            &callout,
                        )
                        .unwrap_or_else(|| layout.notes(&callout))
                } else {
                    layout.notes(&callout)
                }
            } else {
                layout.notes(&callout)
            };
            let (g, plane) = generated_graphic(
                format!("generated:{collection}:{key}"),
                callout.notation(),
                callout,
                placed,
            );
            out.planes.push(plane);
            out.items.entry(key.clone()).or_default().push(g);
            Ok(())
        };
    for (i, t) in semantic.pmi.threads.iter().enumerate() {
        add("threads", i, presentation::thread(t, &semantic.pmi))?;
    }
    for (i, d) in semantic.pmi.dimensions.iter().enumerate() {
        add("dimensions", i, presentation::dimension(d))?;
    }
    for (i, t) in semantic.pmi.tolerances.iter().enumerate() {
        add("tolerances", i, presentation::tolerance(t, &semantic.pmi))?;
    }
    for (i, d) in semantic.pmi.datums.iter().enumerate() {
        add("datums", i, presentation::datum(d.label().as_str()))?;
    }
    for (i, t) in semantic
        .pmi
        .surface_textures
        .iter()
        .enumerate()
        .filter(|(_, t)| t.on.is_some())
    {
        add(
            "surface_textures",
            i,
            presentation::notes::surface_texture(t),
        )?;
    }
    let rows = presentation::notes::general_notes(&semantic.pmi);
    if !rows.is_empty() {
        let c = presentation::notes::notes_block(&rows);
        let placed = layout.notes(&c);
        let (g, plane) = generated_graphic(
            "generated:general-notes".into(),
            "General notes".into(),
            c,
            placed,
        );
        out.planes.push(plane);
        // As for a read STEP callout with several linked occurrences, each
        // requirement keeps its key and aliases the same render-once graphic.
        let mut keys = BTreeSet::new();
        for row in rows {
            use presentation::notes::Source;
            let (collection, index) = match row.source {
                Source::General(i) => ("general", i),
                Source::Texture(i) => ("surface_textures", i),
                Source::Attribute(i) => ("attributes", i),
                Source::Note(i) => ("notes", i),
                Source::Material => continue,
            };
            let key = semantic
                .ids
                .get(collection)
                .and_then(|ids| ids.get(index))
                .ok_or_else(|| Error(format!("missing id {collection}/{index}")))?;
            keys.insert(key.clone());
        }
        // Material has no independent PartIdentifiers carrier.
        if keys.is_empty() {
            out.unlinked.push(g);
        } else {
            for key in keys {
                out.items.entry(key).or_default().push(g.clone());
            }
        }
    }
    // Feature-owned fallback labels must not consume the part block's first
    // notes station. File-owned note graphics are handled by read, unchanged.
    for (i, n) in semantic
        .pmi
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.on.is_some())
    {
        let key = &semantic.ids["notes"][i];
        let c = presentation::label(&n.text);
        let placed = layout.notes(&c);
        let (g, plane) =
            generated_graphic(format!("generated:notes:{key}"), c.notation(), c, placed);
        out.planes.push(plane);
        out.items.entry(key.clone()).or_default().push(g);
    }
    // The existing view model stores orientation and membership, not a camera
    // position or scale. Clients fit the part and these graphics themselves.
    for plane in &out.planes {
        let graphics = out
            .items
            .values()
            .flatten()
            .chain(&out.unlinked)
            .filter(|g| g.plane_id.as_ref() == Some(&plane.id))
            .filter_map(|g| g.id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        out.views.push(View {
            id: format!("{}/view", plane.id),
            name: plane.name.clone(),
            direction: Some(geom::scale(plane.normal, -1.)),
            up: Some(geom::cross(plane.normal, plane.x_axis)),
            graphics,
        });
    }
    #[cfg(feature = "json-import")]
    validate(&serde_json::to_value(&out)?)?;
    Ok(out)
}

#[cfg(feature = "presentation")]
fn generated_graphic(
    id: String,
    name: String,
    callout: crate::presentation::Callout,
    placed: crate::presentation::placement::Placed,
) -> (Graphic, Plane) {
    let mut g = Graphic::new(Source::Generated, name);
    g.id = Some(id);
    layout::generated(&mut g, &callout, &placed);
    let plane = Plane::generated(&g, &placed);
    g.plane_id = Some(plane.id.clone());
    let local = callout.polylines();
    // Linear placement prefixes two witness and eight dimension/arrow lines;
    // feature placement appends scope rings and its leader instead.
    let prefix = placed.lines.len() == local.len() + 10;
    let mut roles = Vec::new();
    if prefix {
        roles.extend([PolylineRole::Extension; 2]);
        roles.extend([PolylineRole::Dimension; 8]);
    }
    roles.extend((0..callout.strokes.len()).map(|i| {
        if i < callout.frame_strokes {
            PolylineRole::Frame
        } else if callout.text_symbols {
            PolylineRole::Text
        } else {
            PolylineRole::Symbol
        }
    }));
    roles.extend(vec![
        if callout.text_symbols {
            PolylineRole::Text
        } else {
            PolylineRole::Symbol
        };
        callout.arcs.len()
    ]);
    roles.extend(vec![
        PolylineRole::Text;
        local.len() - callout.strokes.len() - callout.arcs.len()
    ]);
    if !prefix {
        roles.extend(vec![
            PolylineRole::Symbol;
            placed.lines.len().saturating_sub(local.len() + 1)
        ]);
        if placed.lines.len() > local.len() {
            roles.push(PolylineRole::Leader);
        }
    }
    g.polyline_roles = Some(roles);
    g.polylines = placed.lines;
    g.text = callout.cells;
    (g, plane)
}

pub(crate) fn id_set(semantic: &Semantic) -> Result<Vec<String>> {
    let mut ids: Vec<_> = semantic.ids.values().flatten().cloned().collect();
    let encoded = super::to_value(semantic)?;
    for table in ["length", "angle", "density", "other"] {
        ids.extend(
            encoded["units"][table]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|unit| unit["id"].as_str().map(str::to_string)),
        );
    }
    ids.sort();
    Ok(ids)
}
/// Refuse stale presentation links before use.
pub fn validate_links(presentation: &Presentation, semantic: &Semantic) -> Result<()> {
    if presentation.semantic_ids != id_set(semantic)? {
        return Err(Error(
            "presentation belongs to a different PMI id set".into(),
        ));
    }
    if presentation
        .items
        .keys()
        .any(|id| !presentation.semantic_ids.contains(id))
    {
        return Err(Error("presentation contains an unknown semantic id".into()));
    }
    Ok(())
}
