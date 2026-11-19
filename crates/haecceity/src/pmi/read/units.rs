//! Shared schema access and exact unit/factor resolution. Context selection policy stays
//! separate: semantic PMI is lenient, declared metadata requires unambiguous declarations.
use super::*;
use std::borrow::Cow;

pub(super) enum Source<'a> {
    Document(&'a Document),
    Graph {
        graph: &'a dyn RecordGraph,
        header: &'a [RawEntity],
        bytes: &'a [u8],
    },
}

pub(super) struct UnitReader<'a> {
    source: Source<'a>,
    pub(super) edition: &'static Edition,
    units: HashMap<u64, Result<Unit, MeasureError>>,
    slots: HashMap<String, Vec<(&'static str, &'static str)>>,
    pub(super) metadata_failed: bool,
}

impl<'a> UnitReader<'a> {
    pub(super) fn new(source: Source<'a>) -> Self {
        let schema = match &source {
            Source::Document(doc) => doc.file_schema().ok().and_then(|s| s.into_iter().next()),
            Source::Graph { header, .. } => header.iter().find_map(|entity| {
                let RawEntity::Simple {
                    name, attributes, ..
                } = entity
                else {
                    return None;
                };
                if name != "FILE_SCHEMA" {
                    return None;
                }
                let [Attribute::List(items)] = attributes.as_slice() else {
                    return None;
                };
                match items.first()? {
                    Attribute::String(s) => Some(s.clone()),
                    _ => None,
                }
            }),
        };
        let edition = schema
            .as_deref()
            .and_then(express::edition)
            .unwrap_or(express::TARGET);
        Self {
            source,
            edition,
            units: HashMap::new(),
            slots: HashMap::new(),
            metadata_failed: false,
        }
    }

    fn record(&self, id: u64) -> Option<Cow<'a, RawEntity>> {
        match self.source {
            Source::Document(doc) => doc.graph().entities.get(&id).map(Cow::Borrowed),
            Source::Graph { graph, .. } => graph.record(id),
        }
    }

    pub(super) fn names(&self, id: u64) -> Vec<String> {
        self.record(id)
            .as_deref()
            .map(leaf_names)
            .unwrap_or_default()
    }

    pub(super) fn entity_label(&self, id: u64) -> String {
        let mut n = self.names(id);
        n.sort();
        if n.is_empty() {
            "missing".to_string()
        } else {
            n.join("+")
        }
    }

    pub(super) fn is_a_name(&self, name: &str, ancestor: &str) -> bool {
        self.edition.is_a(name, ancestor)
            || (!self.edition.is_declared(name) && express::TARGET.is_a(name, ancestor))
            || (!self.edition.is_declared(name)
                && !express::TARGET.is_declared(name)
                && express::AP242_ED1.is_a(name, ancestor))
    }

    pub(super) fn is(&self, id: u64, ancestor: &str) -> bool {
        self.names(id).iter().any(|n| self.is_a_name(n, ancestor))
    }

    /// The (declaring entity, attribute) of each explicit attribute of a simple instance.
    fn slots_of(&mut self, name: &str) -> &Vec<(&'static str, &'static str)> {
        if !self.slots.contains_key(name) {
            let slots = self
                .edition
                .explicit_attributes(name)
                .or_else(|| express::TARGET.explicit_attributes(name))
                .or_else(|| express::AP242_ED1.explicit_attributes(name))
                .unwrap_or_default();
            let v = slots.into_iter().map(|s| (s.entity, s.name)).collect();
            self.slots.insert(name.to_string(), v);
        }
        &self.slots[name]
    }

    /// Attribute `attr` declared by `entity` (both lower case) of instance `id`.
    /// Semantic-PMI callers borrow the editing document's stable attribute tree.
    pub(super) fn get(&mut self, id: u64, entity: &str, attr: &str) -> Option<&'a Attribute> {
        let Cow::Borrowed(record) = self.record(id)? else {
            return None;
        };
        self.attribute(record, entity, attr)
    }

    /// Metadata queries can own one decoded record without retaining a second graph.
    fn get_value(&mut self, id: u64, entity: &str, attr: &str) -> Option<Cow<'a, Attribute>> {
        match self.record(id)? {
            Cow::Borrowed(record) => self.attribute(record, entity, attr).map(Cow::Borrowed),
            Cow::Owned(record) => self
                .attribute(&record, entity, attr)
                .cloned()
                .map(Cow::Owned),
        }
    }

    fn attribute<'r>(
        &mut self,
        record: &'r RawEntity,
        entity: &str,
        attr: &str,
    ) -> Option<&'r Attribute> {
        match record {
            RawEntity::Simple {
                name, attributes, ..
            } => {
                let lname = name.to_ascii_lowercase();
                let i = self
                    .slots_of(&lname)
                    .iter()
                    .position(|&(en, an)| en == entity && an == attr)?;
                attributes.get(i)
            }
            RawEntity::Complex { parts, .. } => {
                let leaf = parts.iter().find(|p| p.name.eq_ignore_ascii_case(entity))?;
                let decl = self
                    .edition
                    .entity(entity)
                    .or_else(|| express::TARGET.entity(entity))
                    .or_else(|| express::AP242_ED1.entity(entity))?;
                let i = decl.attributes.iter().position(|a| a.name == attr)?;
                leaf.attributes.get(i)
            }
        }
    }

    pub(super) fn get_ref(&mut self, id: u64, entity: &str, attr: &str) -> Option<u64> {
        ref1(self.get_value(id, entity, attr).as_deref())
    }

    pub(super) fn get_refs(&mut self, id: u64, entity: &str, attr: &str) -> Vec<u64> {
        refs(self.get_value(id, entity, attr).as_deref())
    }

    /// A string attribute, decoded; `None` when unset.
    pub(super) fn get_str(&mut self, id: u64, entity: &str, attr: &str) -> Option<String> {
        match self.get_value(id, entity, attr)?.as_ref() {
            Attribute::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// The text of the REAL `target` (an attribute of instance `id`) as the file writes it.
    pub(super) fn real_text(&self, id: u64, target: &Attribute) -> Option<String> {
        self.record_real_text(id, self.record(id)?.as_ref(), target)
    }

    fn record_real_text(&self, id: u64, record: &RawEntity, target: &Attribute) -> Option<String> {
        if let Attribute::Integer(i) = target {
            return Some(i.to_string());
        }
        let mut n = 0usize;
        let mut found = None;
        let mut count = 0usize;
        visit_attributes(record, &mut |a| {
            if let Attribute::Real(_) = a {
                if std::ptr::eq(a, target) {
                    found = Some(n);
                }
                n += 1;
                count += 1;
            }
        });
        let text = match self.source {
            Source::Document(doc) => &doc.bytes()[doc.span(id)?],
            Source::Graph { bytes, .. } => {
                bytes.get(usize::try_from(record.span().start).ok()?..)?
            }
        };
        // Graph spans locate #N; only this requested unit-factor record is tokenised.
        // No whole-file scanner, source copy, inverse references or edit policy is involved.
        let tokens = real_tokens(text);
        if tokens.len() != count {
            return None;
        }
        tokens.get(found?).cloned()
    }

    pub(super) fn unit(&mut self, id: u64) -> Result<Unit, MeasureError> {
        if let Some(u) = self.units.get(&id) {
            return u.clone();
        }
        if matches!(self.source, Source::Graph { .. }) {
            return self.graph_unit(id);
        }
        self.units.insert(
            id,
            Err(MeasureError::unitless(format!(
                "unit #{id} refers to itself"
            ))),
        );
        let u = self.resolve_unit(id);
        self.units.insert(id, u.clone());
        u
    }

    /// Geometry import can encounter factor chains that semantic PMI never reaches.
    /// Schedule their unit dependencies in postorder, then use the SAME factor resolver;
    /// every nested `unit()` lookup is already cached. A cycle sees the in-progress error.
    fn graph_unit(&mut self, root: u64) -> Result<Unit, MeasureError> {
        let mut pending = vec![(root, false)];
        while let Some((id, resolve)) = pending.pop() {
            if resolve {
                let unit = self.resolve_unit(id);
                self.units.insert(id, unit);
                continue;
            }
            if self.units.contains_key(&id) {
                continue;
            }
            self.units.insert(
                id,
                Err(MeasureError::unitless(format!(
                    "unit #{id} refers to itself"
                ))),
            );
            pending.push((id, true));
            // Preserve resolve_unit's SI-before-conversion precedence for complex units.
            let names = self.names(id);
            if !names.iter().any(|n| n == "si_unit")
                && names.iter().any(|n| n == "conversion_based_unit")
                && let Some(factor) = self.get_ref(id, "conversion_based_unit", "conversion_factor")
                && let Some(child) = self.get_ref(factor, "measure_with_unit", "unit_component")
            {
                pending.push((child, false));
            }
        }
        self.units.get(&root).cloned().unwrap_or_else(|| {
            Err(MeasureError::unitless(format!(
                "unit #{root} does not exist"
            )))
        })
    }

    fn resolve_unit(&mut self, id: u64) -> Result<Unit, MeasureError> {
        let names = self.names(id);
        if names.is_empty() {
            return Err(MeasureError::unitless(format!("unit #{id} does not exist")));
        }
        let has = |n: &str| names.iter().any(|x| x == n);
        if has("si_unit") {
            let prefix = enum_value(self.get_value(id, "si_unit", "prefix").as_deref())
                .map(str::to_ascii_uppercase);
            let name = enum_value(self.get_value(id, "si_unit", "name").as_deref())
                .map(str::to_ascii_uppercase)
                .ok_or_else(|| MeasureError::unitless(format!("si_unit #{id} has no name")))?;
            let power = match prefix.as_deref() {
                None => 0,
                Some(p) => match si_prefix(p) {
                    Some(e) => e,
                    None => {
                        return Err(MeasureError::nonconformance(format!(
                            "si_unit #{id} has unknown prefix {p}"
                        )));
                    }
                },
            };
            let full = format!(
                "{}{}",
                prefix
                    .as_deref()
                    .map_or(String::new(), str::to_ascii_lowercase),
                name.to_ascii_lowercase()
            );
            let one = Decimal::parse("1.")
                .map_err(|e| MeasureError::nonconformance(format!("unit scale: {e:?}")))?;
            return Ok(match name.as_str() {
                "METRE" => Unit::Length(LengthUnit::from_metres(&full, one.scaled(power))),
                "RADIAN" if power == 0 => Unit::Angle(AngleUnit::Radian),
                "RADIAN" => Unit::Angle(AngleUnit::Other {
                    name: full,
                    radians: one.scaled(power),
                }),
                _ => Unit::Other(full),
            });
        }
        if has("conversion_based_unit") {
            let name = self
                .get_str(id, "conversion_based_unit", "name")
                .unwrap_or_default();
            let factor = self
                .get_ref(id, "conversion_based_unit", "conversion_factor")
                .ok_or_else(|| {
                    MeasureError::unitless(format!(
                        "conversion_based_unit #{id} has no conversion factor"
                    ))
                })?;
            let m = self.measure(factor)?;
            let lname = name.to_ascii_lowercase();
            return match m {
                Measured::Length(l) => {
                    let unit = LengthUnit::from_metres(&name, l.value.times(&l.unit.metres()));
                    let expected = match lname.as_str() {
                        "inch" => Some(LengthUnit::Inch),
                        "millimetre" | "millimeter" => Some(LengthUnit::Millimetre),
                        "foot" => Some(LengthUnit::Foot),
                        "metre" | "meter" => Some(LengthUnit::Metre),
                        _ => None,
                    };
                    if let Some(e) = expected
                        && e != unit
                    {
                        return Err(MeasureError::nonconformance(format!(
                            "conversion_based_unit #{id} is named {name:?} but its factor is {} {:?}",
                            l.value, l.unit
                        )));
                    }
                    Ok(Unit::Length(unit))
                }
                Measured::Angle(a) => {
                    let radians = a.rad();
                    let degree = std::f64::consts::PI / 180.0;
                    let unit = if (radians / degree - 1.0).abs() < 1e-9 {
                        AngleUnit::Degree
                    } else if a.unit == AngleUnit::Radian {
                        AngleUnit::Other {
                            name: name.clone(),
                            radians: a.value.clone(),
                        }
                    } else {
                        return Err(MeasureError::nonconformance(format!(
                            "conversion_based_unit #{id}: an angle factor in a unit other than radian"
                        )));
                    };
                    if lname == "degree" && unit != AngleUnit::Degree {
                        return Err(MeasureError::nonconformance(format!(
                            "conversion_based_unit #{id} is named {name:?} but its factor is {} rad",
                            a.value
                        )));
                    }
                    Ok(Unit::Angle(unit))
                }
                _ => Ok(Unit::Other(name)),
            };
        }
        if has("derived_unit") || has("context_dependent_unit") || has("named_unit") {
            let name = if has("context_dependent_unit") {
                self.get_str(id, "context_dependent_unit", "name")
                    .unwrap_or_default()
            } else {
                match self.source {
                    Source::Document(doc) => super::super::unit_identity::identity(doc, id),
                    Source::Graph { .. } => None,
                }
                .ok_or_else(|| {
                    MeasureError::unitless(format!("unit #{id} has no portable content identity"))
                })?
            };
            return Ok(Unit::Other(name));
        }
        Err(MeasureError::unitless(format!(
            "#{id} ({}) is not a unit",
            names.join("+")
        )))
    }

    /// The measure of a `measure_with_unit` instance (simple or a complex with that leaf).
    pub(super) fn measure(&mut self, id: u64) -> Result<Measured, MeasureError> {
        if !self.is(id, "measure_with_unit") {
            return Err(MeasureError::nonconformance(format!(
                "#{id} ({}) is not a measure",
                self.entity_label(id)
            )));
        }
        // Keep the record alive while locating the REAL by pointer within it.
        // Matching by floating-point value would confuse equal-valued factors.
        let record = self
            .record(id)
            .ok_or_else(|| MeasureError::nonconformance(format!("measure #{id} has no value")))?;
        let value = self
            .attribute(record.as_ref(), "measure_with_unit", "value_component")
            .ok_or_else(|| MeasureError::nonconformance(format!("measure #{id} has no value")))?;
        let (type_name, inner) = match value {
            Attribute::Typed { type_name, value } => {
                (Some(type_name.to_ascii_lowercase()), &**value)
            }
            v => (None, v),
        };
        let text = match inner {
            Attribute::Real(_) | Attribute::Integer(_) => self
                .record_real_text(id, record.as_ref(), inner)
                .ok_or_else(|| {
                    MeasureError::nonconformance(format!(
                        "measure #{id}: the value's text could not be read"
                    ))
                })?,
            other => {
                return Err(MeasureError::nonconformance(format!(
                    "measure #{id}: value {other:?} is not a number"
                )));
            }
        };
        let decimal = Decimal::parse(&text)
            .map_err(|e| MeasureError::nonconformance(format!("measure #{id}: {e}")))?;
        let unit_id = self
            .get_ref(id, "measure_with_unit", "unit_component")
            .ok_or_else(|| MeasureError::unitless(format!("measure #{id} has no unit instance")))?;
        let unit = self.unit(unit_id)?;
        let tn = type_name.as_deref().unwrap_or("");
        match tn {
            "ratio_measure" | "positive_ratio_measure" => return Ok(Measured::Ratio(decimal)),
            "count_measure" => return Ok(Measured::Count(decimal)),
            _ => {}
        }
        let length_type = matches!(
            tn,
            "length_measure" | "positive_length_measure" | "non_negative_length_measure"
        );
        let angle_type = matches!(tn, "plane_angle_measure" | "positive_plane_angle_measure");
        match unit {
            Unit::Length(u) if length_type || tn.is_empty() => Ok(Measured::Length(Length {
                value: decimal,
                unit: u,
            })),
            Unit::Angle(u) if angle_type || tn.is_empty() => Ok(Measured::Angle(Angle {
                value: decimal,
                unit: u,
            })),
            Unit::Other(u) if !length_type && !angle_type => Ok(Measured::Other {
                measure: tn.to_string(),
                value: decimal,
                unit: u,
            }),
            u => Err(MeasureError::nonconformance(format!(
                "measure #{id}: a {} value in unit {u:?}",
                if tn.is_empty() { "untyped" } else { tn }
            ))),
        }
    }

    /// PMI preserves main's first resolvable length unit, including untyped SI units.
    pub(super) fn context_length_unit(&mut self, rep: u64) -> Option<LengthUnit> {
        let ctx = self.get_ref(rep, "representation", "context_of_items")?;
        for u in self.get_refs(ctx, "global_unit_assigned_context", "units") {
            if let Ok(Unit::Length(l)) = self.unit(u) {
                return Some(l);
            }
        }
        None
    }

    /// The length unit of a representation's context (`global_unit_assigned_context`).
    pub(super) fn declared_context_length_unit(&mut self, rep: u64) -> Option<LengthUnit> {
        let ctx = self.get_ref(rep, "representation", "context_of_items")?;
        let mut length = None;
        for u in self.get_refs(ctx, "global_unit_assigned_context", "units") {
            if !self.is(u, "length_unit") {
                continue;
            }
            let resolved = match self.unit(u) {
                Ok(unit) => unit,
                Err(_) => {
                    self.metadata_failed = true;
                    return None;
                }
            };
            let Unit::Length(unit) = resolved else {
                return None;
            };
            if length.as_ref().is_some_and(|known| *known != unit) {
                return None;
            }
            length = Some(unit);
        }
        length
    }
}
