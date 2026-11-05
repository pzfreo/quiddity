//! Named checks of the AP242 WHERE, UNIQUE and global rules the PMI writer relies on, over a
//! [`Document`], each a function citing its label in the target edition's long form
//! (`242_mim_lf.exp`, [`crate::express::TARGET`]), and [`check_all`] running them all.
//!
//! [`crate::express`] validates instances (names, attributes, types, bounds, complex
//! combinations) but evaluates no WHERE, UNIQUE or global rule. This module is not a general EXPRESS
//! rule engine: each rule below is transcribed by hand from the schema text, with the schema's
//! own reading of indeterminate values: a WHERE rule is violated only when it evaluates to
//! FALSE (ISO 10303-11 §9.2.2.2), so a comparison with an unset (`$`) value, which is UNKNOWN,
//! never violates; a UNIQUE rule ignores instances whose compared values are indeterminate.
//!
//! The rules ([`RULES`]):
//!
//! - `thread` WR1–WR16 and `turned_knurl` WR1–WR12: the parameters in their
//!   `shape_representation_with_parameters` (count and names) and the 'partial area
//!   occurrence', 'applied shape' and 'thread runout' shape aspects of their own
//!   `product_definition_shape`;
//! - `default_tolerance_table` WR1–WR2 and `default_tolerance_table_cell` WR1–WR5 (the
//!   functions `default_tolerance_table_cell_wr2`…`_wr5`);
//! - `datum` WR1–WR4 and UR1, `datum_feature` WR1–WR2, `datum_target` WR1–WR5 and UR1,
//!   `placed_datum_target_feature` WR1–WR3 (WR3 is the function
//!   `valid_datum_target_parameters`), `datum_system` WR1 and UR1, and the INVERSE
//!   `datum_reference_compartment.owner` (exactly one `datum_system` lists a compartment: PMI
//!   practice §6.9.7, compartments are never shared);
//! - `geometric_tolerance` WR1, WR3, WR5, `geometric_tolerance_with_datum_reference` WR1,
//!   `flatness_tolerance`, `straightness_tolerance`, `roundness_tolerance` and
//!   `cylindricity_tolerance` WR1, `geometric_tolerance_relationship` WR1–WR3;
//! - `plus_minus_tolerance` UR1;
//! - the rules of `item_identified_representation_usage` that the PMI practice relies on for
//!   `geometric_item_specific_usage` (§5.1, §6.1: one usage per item and per aspect in a
//!   representation, the item in that representation): UR1, UR2 and WR1, evaluated over every
//!   usage (the UNIQUE rules are declared on the supertype, so they span its whole population,
//!   draughting model item associations included);
//! - `surface_texture_representation` WR1–WR5 and `general_property_association` WR1–WR2
//!   (ISO 10303-1110's surface texture parameters; the associations of notes and other
//!   general properties);
//! - `mechanical_design_and_draughting_relationship` WR1–WR3 (which representations may be
//!   related to which).
//!
//! The global rules ([`GLOBAL_RULES`]), each a WHERE rule over the whole population of the
//! entity types it is FOR, reported on the instances that make it FALSE:
//!
//! - `restrict_representation_for_surface_condition` WR1 (a surface condition's property
//!   definition and its representation are named alike).
//!
//! Two literal readings worth knowing: `default_tolerance_table` WR2 compares names with `<`
//! (as written; `'general tolerance definition'` and `'default tolerance'` themselves pass);
//! `datum_target` WR5 forbids a description that `placed_datum_target_feature` WR1 requires
//! ('point', …): a placed datum target with a description violates WR5 as the schema is
//! written.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::express::TARGET;
use crate::p21::{Attribute, Document, RawEntity};

/// One instance that violates one rule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuleViolation {
    pub id: u64,
    /// The instance's type: its entity name, or `(a b c)` for a complex instance, lower case.
    pub entity: String,
    /// `entity.label` of the rule, as the schema declares it (`thread.WR3`, `datum.UR1`,
    /// `datum_reference_compartment.owner` for the INVERSE cardinality), or `name.label` of a
    /// global rule (`restrict_representation_for_surface_condition.WR1`).
    pub rule: &'static str,
    pub message: String,
}

impl std::fmt::Display for RuleViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "#{} {} violates {}: {}",
            self.id, self.entity, self.rule, self.message
        )
    }
}

/// A rule: the entity declaring it, its label, and how it is checked.
pub struct Rule {
    /// The declaring entity type (lower case).
    pub entity: &'static str,
    /// `entity.label`.
    pub rule: &'static str,
    /// Its label in the schema (`WR3`, `UR1`) or, for an INVERSE cardinality, the attribute.
    pub label: &'static str,
    check: Check,
}

enum Check {
    /// A WHERE rule (or INVERSE cardinality) of one instance: `Some(why)` when FALSE.
    Where(fn(&Ctx, u64) -> Option<String>),
    /// A UNIQUE rule: the key of an instance, `None` when indeterminate.
    Unique(fn(&Ctx, u64) -> Option<String>),
}

macro_rules! rule {
    ($entity:literal, $label:literal, $check:expr) => {
        Rule {
            entity: $entity,
            rule: concat!($entity, ".", $label),
            label: $label,
            check: $check,
        }
    };
}

/// Every rule [`check_all`] evaluates, by entity and label.
pub const RULES: &[Rule] = &[
    rule!(
        "default_model_geometric_view",
        "UR1",
        Check::Unique(default_view_ur1)
    ),
    rule!(
        "default_model_geometric_view",
        "WR1",
        Check::Where(default_view_wr1)
    ),
    rule!(
        "default_model_geometric_view",
        "WR2",
        Check::Where(default_view_wr2)
    ),
    rule!(
        "default_model_geometric_view",
        "WR3",
        Check::Where(default_view_wr3)
    ),
    rule!(
        "characterized_item_within_representation",
        "UR1",
        Check::Unique(characterized_item_ur1)
    ),
    rule!(
        "characterized_item_within_representation",
        "WR1",
        Check::Where(characterized_item_wr1)
    ),
    rule!("thread", "WR1", Check::Where(thread_wr1)),
    rule!("thread", "WR2", Check::Where(thread_wr2)),
    rule!("thread", "WR3", Check::Where(thread_wr3)),
    rule!("thread", "WR4", Check::Where(thread_wr4)),
    rule!("thread", "WR5", Check::Where(thread_wr5)),
    rule!("thread", "WR6", Check::Where(thread_wr6)),
    rule!("thread", "WR7", Check::Where(thread_wr7)),
    rule!("thread", "WR8", Check::Where(thread_wr8)),
    rule!("thread", "WR9", Check::Where(thread_wr9)),
    rule!("thread", "WR10", Check::Where(thread_wr10)),
    rule!("thread", "WR11", Check::Where(thread_wr11)),
    rule!("thread", "WR12", Check::Where(thread_wr12)),
    rule!("thread", "WR13", Check::Where(thread_wr13)),
    rule!("thread", "WR14", Check::Where(thread_wr14)),
    rule!("thread", "WR15", Check::Where(thread_wr15)),
    rule!("thread", "WR16", Check::Where(thread_wr16)),
    rule!("turned_knurl", "WR1", Check::Where(turned_knurl_wr1)),
    rule!("turned_knurl", "WR2", Check::Where(turned_knurl_wr2)),
    rule!("turned_knurl", "WR3", Check::Where(turned_knurl_wr3)),
    rule!("turned_knurl", "WR4", Check::Where(turned_knurl_wr4)),
    rule!("turned_knurl", "WR5", Check::Where(turned_knurl_wr5)),
    rule!("turned_knurl", "WR6", Check::Where(turned_knurl_wr6)),
    rule!("turned_knurl", "WR7", Check::Where(turned_knurl_wr7)),
    rule!("turned_knurl", "WR8", Check::Where(turned_knurl_wr8)),
    rule!("turned_knurl", "WR9", Check::Where(turned_knurl_wr9)),
    rule!("turned_knurl", "WR10", Check::Where(turned_knurl_wr10)),
    rule!("turned_knurl", "WR11", Check::Where(turned_knurl_wr11)),
    rule!("turned_knurl", "WR12", Check::Where(turned_knurl_wr12)),
    rule!(
        "default_tolerance_table",
        "WR1",
        Check::Where(default_tolerance_table_wr1)
    ),
    rule!(
        "default_tolerance_table",
        "WR2",
        Check::Where(default_tolerance_table_wr2)
    ),
    rule!(
        "default_tolerance_table_cell",
        "WR1",
        Check::Where(default_tolerance_table_cell_wr1)
    ),
    rule!(
        "default_tolerance_table_cell",
        "WR2",
        Check::Where(default_tolerance_table_cell_wr2)
    ),
    rule!(
        "default_tolerance_table_cell",
        "WR3",
        Check::Where(default_tolerance_table_cell_wr3)
    ),
    rule!(
        "default_tolerance_table_cell",
        "WR4",
        Check::Where(default_tolerance_table_cell_wr4)
    ),
    rule!(
        "default_tolerance_table_cell",
        "WR5",
        Check::Where(default_tolerance_table_cell_wr5)
    ),
    rule!("datum", "WR1", Check::Where(datum_wr1)),
    rule!("datum", "WR2", Check::Where(datum_wr2)),
    rule!("datum", "WR3", Check::Where(datum_wr3)),
    rule!("datum", "WR4", Check::Where(datum_wr4)),
    rule!("datum", "UR1", Check::Unique(datum_ur1)),
    rule!("datum_feature", "WR1", Check::Where(datum_feature_wr1)),
    rule!("datum_feature", "WR2", Check::Where(datum_feature_wr2)),
    rule!("datum_target", "WR1", Check::Where(datum_target_wr1)),
    rule!("datum_target", "WR2", Check::Where(datum_target_wr2)),
    rule!("datum_target", "WR3", Check::Where(datum_target_wr3)),
    rule!("datum_target", "WR4", Check::Where(datum_target_wr4)),
    rule!("datum_target", "WR5", Check::Where(datum_target_wr5)),
    rule!("datum_target", "UR1", Check::Unique(datum_target_ur1)),
    rule!(
        "placed_datum_target_feature",
        "WR1",
        Check::Where(placed_datum_target_feature_wr1)
    ),
    rule!(
        "placed_datum_target_feature",
        "WR2",
        Check::Where(placed_datum_target_feature_wr2)
    ),
    rule!(
        "placed_datum_target_feature",
        "WR3",
        Check::Where(placed_datum_target_feature_wr3)
    ),
    rule!("datum_system", "WR1", Check::Where(datum_system_wr1)),
    rule!("datum_system", "UR1", Check::Unique(datum_system_ur1)),
    rule!(
        "datum_reference_compartment",
        "owner",
        Check::Where(datum_reference_compartment_owner)
    ),
    rule!(
        "geometric_tolerance",
        "WR1",
        Check::Where(geometric_tolerance_wr1)
    ),
    rule!(
        "geometric_tolerance",
        "WR3",
        Check::Where(geometric_tolerance_wr3)
    ),
    rule!(
        "geometric_tolerance",
        "WR5",
        Check::Where(geometric_tolerance_wr5)
    ),
    rule!(
        "geometric_tolerance_with_datum_reference",
        "WR1",
        Check::Where(geometric_tolerance_with_datum_reference_wr1)
    ),
    rule!(
        "flatness_tolerance",
        "WR1",
        Check::Where(no_datum_reference)
    ),
    rule!(
        "straightness_tolerance",
        "WR1",
        Check::Where(no_datum_reference)
    ),
    rule!(
        "roundness_tolerance",
        "WR1",
        Check::Where(no_datum_reference)
    ),
    rule!(
        "cylindricity_tolerance",
        "WR1",
        Check::Where(no_datum_reference)
    ),
    rule!(
        "geometric_tolerance_relationship",
        "WR1",
        Check::Where(geometric_tolerance_relationship_wr1)
    ),
    rule!(
        "geometric_tolerance_relationship",
        "WR2",
        Check::Where(geometric_tolerance_relationship_wr2)
    ),
    rule!(
        "geometric_tolerance_relationship",
        "WR3",
        Check::Where(geometric_tolerance_relationship_wr3)
    ),
    rule!(
        "plus_minus_tolerance",
        "UR1",
        Check::Unique(plus_minus_tolerance_ur1)
    ),
    rule!(
        "item_identified_representation_usage",
        "UR1",
        Check::Unique(item_identified_representation_usage_ur1)
    ),
    rule!(
        "item_identified_representation_usage",
        "UR2",
        Check::Unique(item_identified_representation_usage_ur2)
    ),
    rule!(
        "item_identified_representation_usage",
        "WR1",
        Check::Where(item_identified_representation_usage_wr1)
    ),
    rule!(
        "surface_texture_representation",
        "WR1",
        Check::Where(surface_texture_representation_wr1)
    ),
    rule!(
        "surface_texture_representation",
        "WR2",
        Check::Where(surface_texture_representation_wr2)
    ),
    rule!(
        "surface_texture_representation",
        "WR3",
        Check::Where(surface_texture_representation_wr3)
    ),
    rule!(
        "surface_texture_representation",
        "WR4",
        Check::Where(surface_texture_representation_wr4)
    ),
    rule!(
        "surface_texture_representation",
        "WR5",
        Check::Where(surface_texture_representation_wr5)
    ),
    rule!(
        "general_property_association",
        "WR1",
        Check::Where(general_property_association_wr1)
    ),
    rule!(
        "general_property_association",
        "WR2",
        Check::Where(general_property_association_wr2)
    ),
    rule!(
        "mechanical_design_and_draughting_relationship",
        "WR1",
        Check::Where(mechanical_design_and_draughting_relationship_wr1)
    ),
    rule!(
        "mechanical_design_and_draughting_relationship",
        "WR2",
        Check::Where(mechanical_design_and_draughting_relationship_wr2)
    ),
    rule!(
        "mechanical_design_and_draughting_relationship",
        "WR3",
        Check::Where(mechanical_design_and_draughting_relationship_wr3)
    ),
];

/// A global rule: its name, the label of its WHERE rule, the entity types it is FOR, and the
/// instances of their population that make the rule FALSE, each with why.
pub struct GlobalRule {
    /// The rule's name in the schema.
    pub name: &'static str,
    /// `name.label`.
    pub rule: &'static str,
    /// Its WHERE rule's label (`WR1`).
    pub label: &'static str,
    /// The entity types the rule is FOR (lower case).
    pub entities: &'static [&'static str],
    check: fn(&Ctx) -> Vec<(u64, String)>,
}

/// Every global rule [`check_all`] evaluates.
pub const GLOBAL_RULES: &[GlobalRule] = &[GlobalRule {
    name: "restrict_representation_for_surface_condition",
    rule: "restrict_representation_for_surface_condition.WR1",
    label: "WR1",
    entities: &["property_definition_representation"],
    check: restrict_representation_for_surface_condition_wr1,
}];

/// Every violation of [`RULES`] and [`GLOBAL_RULES`] in the document, ordered by instance id,
/// then rule.
pub fn check_all(doc: &Document) -> Vec<RuleViolation> {
    let ctx = Ctx::new(doc);
    let mut out = Vec::new();
    let mut keys: Vec<BTreeMap<String, Vec<u64>>> = RULES.iter().map(|_| BTreeMap::new()).collect();
    for id in doc.ids() {
        for (r, rule) in RULES.iter().enumerate() {
            if !ctx.is(id, rule.entity) {
                continue;
            }
            match rule.check {
                Check::Where(f) => {
                    if let Some(message) = f(&ctx, id) {
                        out.push(ctx.violation(id, rule.rule, message));
                    }
                }
                Check::Unique(f) => {
                    if let Some(key) = f(&ctx, id) {
                        keys[r].entry(key).or_default().push(id);
                    }
                }
            }
        }
    }
    for (rule, keys) in RULES.iter().zip(keys) {
        for (key, ids) in keys {
            if ids.len() < 2 {
                continue;
            }
            for &id in &ids {
                let others: Vec<String> = ids
                    .iter()
                    .filter(|&&o| o != id)
                    .map(|o| format!("#{o}"))
                    .collect();
                out.push(ctx.violation(
                    id,
                    rule.rule,
                    format!("shares {key} with {}", others.join(", ")),
                ));
            }
        }
    }
    for rule in GLOBAL_RULES {
        for (id, message) in (rule.check)(&ctx) {
            out.push(ctx.violation(id, rule.rule, message));
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------------------------
// Instance access
// ---------------------------------------------------------------------------------------------

/// An explicit attribute: (declaring entity, name).
type Slot = (&'static str, &'static str);

struct Ctx<'a> {
    doc: &'a Document,
    /// Per simple entity name: its explicit attributes as `(declaring entity, name)`.
    slots: RefCell<HashMap<String, Option<Vec<Slot>>>>,
}

impl<'a> Ctx<'a> {
    fn new(doc: &'a Document) -> Self {
        Ctx {
            doc,
            slots: RefCell::new(HashMap::new()),
        }
    }

    fn violation(&self, id: u64, rule: &'static str, message: String) -> RuleViolation {
        RuleViolation {
            id,
            entity: self.doc.get(id).map(type_name).unwrap_or_default(),
            rule,
            message,
        }
    }

    /// TYPEOF(x) contains `ty`: some part of the record is `ty` or a subtype.
    fn is(&self, id: u64, ty: &str) -> bool {
        match self.doc.get(id) {
            Some(RawEntity::Simple { name, .. }) => TARGET.is_a(name, ty),
            Some(RawEntity::Complex { parts, .. }) => {
                parts.iter().any(|p| TARGET.is_a(&p.name, ty))
            }
            None => false,
        }
    }

    /// Whether TYPEOF(x) contains every one of `types`.
    fn is_all(&self, id: u64, types: &[&str]) -> bool {
        types.iter().all(|t| self.is(id, t))
    }

    /// Attribute `entity.name` of `#id`, as written.
    fn attr(&self, id: u64, entity: &str, name: &str) -> Option<&'a Attribute> {
        match self.doc.get(id)? {
            RawEntity::Simple {
                name: n,
                attributes,
                ..
            } => {
                let mut cache = self.slots.borrow_mut();
                let slots = cache.entry(n.clone()).or_insert_with(|| {
                    TARGET
                        .explicit_attributes(n)
                        .map(|s| s.iter().map(|s| (s.entity, s.name)).collect())
                });
                let i = slots
                    .as_ref()?
                    .iter()
                    .position(|&(e, a)| e == entity && a == name)?;
                attributes.get(i)
            }
            RawEntity::Complex { parts, .. } => {
                let part = parts.iter().find(|p| p.name.eq_ignore_ascii_case(entity))?;
                let i = TARGET
                    .entity(entity)?
                    .attributes
                    .iter()
                    .position(|a| a.name == name)?;
                part.attributes.get(i)
            }
        }
    }

    /// The instance an attribute references.
    fn reference(&self, id: u64, entity: &str, name: &str) -> Option<u64> {
        match self.attr(id, entity, name)? {
            Attribute::EntityRef(t) => Some(*t),
            _ => None,
        }
    }

    /// Every instance an attribute references (an aggregate's elements, through typed values).
    fn references(&self, id: u64, entity: &str, name: &str) -> Vec<u64> {
        let mut out = Vec::new();
        if let Some(a) = self.attr(id, entity, name) {
            refs(a, &mut out);
        }
        out
    }

    /// A STRING attribute, decoded; `None` when unset or not a string.
    fn text(&self, id: u64, entity: &str, name: &str) -> Option<String> {
        match self.attr(id, entity, name)? {
            Attribute::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// A LOGICAL or BOOLEAN attribute: `Some(true)` for `.T.`, `Some(false)` for `.F.`,
    /// `None` for `.U.` or unset.
    fn logical(&self, id: u64, entity: &str, name: &str) -> Option<bool> {
        match self.attr(id, entity, name)? {
            Attribute::Enum(e) if e == "T" => Some(true),
            Attribute::Enum(e) if e == "F" => Some(false),
            _ => None,
        }
    }

    /// `representation_item.name`.
    fn item_name(&self, id: u64) -> Option<String> {
        self.text(id, "representation_item", "name")
    }

    /// USEDIN(x, 'ENTITY.ATTRIBUTE'): instances of `entity` whose `attribute` references `x`.
    fn used_in(&self, x: u64, entity: &str, attribute: &str) -> Vec<u64> {
        self.doc
            .referrers(x)
            .iter()
            .copied()
            .filter(|&r| self.is(r, entity) && self.references(r, entity, attribute).contains(&x))
            .collect()
    }

    /// `representation.items`.
    fn items(&self, rep: u64) -> Vec<u64> {
        self.references(rep, "representation", "items")
    }

    /// USEDIN(x, 'PROPERTY_DEFINITION.DEFINITION').
    fn property_definitions(&self, x: u64) -> Vec<u64> {
        self.used_in(x, "property_definition", "definition")
    }

    /// The `shape_representation_with_parameters` each property definition of `x` is
    /// represented by (the `impl_rep`s of the thread and knurl rules), per property definition.
    fn parameter_representations(&self, x: u64) -> Vec<(u64, Vec<u64>)> {
        self.property_definitions(x)
            .into_iter()
            .map(|pd| {
                let reps = self
                    .used_in(pd, "property_definition_representation", "definition")
                    .into_iter()
                    .filter_map(|pdr| {
                        self.reference(
                            pdr,
                            "property_definition_representation",
                            "used_representation",
                        )
                    })
                    .filter(|&rep| self.is(rep, "shape_representation_with_parameters"))
                    .collect();
                (pd, reps)
            })
            .collect()
    }

    /// The items of `rep` that are of all `types`, named `name`.
    fn named_items(&self, rep: u64, types: &[&str], name: &str) -> Vec<u64> {
        self.items(rep)
            .into_iter()
            .filter(|&i| self.is_all(i, types) && self.item_name(i).as_deref() == Some(name))
            .collect()
    }

    /// using_representations(item): the representations whose items reference `item`,
    /// directly or through representation items and founded items.
    fn using_representations(&self, item: u64) -> BTreeSet<u64> {
        let mut using = BTreeSet::from([item]);
        let mut work = vec![item];
        while let Some(x) = work.pop() {
            for &r in self.doc.referrers(x) {
                if (self.is(r, "representation_item") || self.is(r, "founded_item"))
                    && using.insert(r)
                {
                    work.push(r);
                }
            }
        }
        using
            .iter()
            .flat_map(|&x| self.used_in(x, "representation", "items"))
            .collect()
    }
}

fn refs(a: &Attribute, out: &mut Vec<u64>) {
    match a {
        Attribute::EntityRef(t) => out.push(*t),
        Attribute::List(items) => items.iter().for_each(|i| refs(i, out)),
        Attribute::Typed { value, .. } => refs(value, out),
        _ => {}
    }
}

fn number(a: &Attribute) -> Option<f64> {
    match a {
        Attribute::Real(v) => Some(*v),
        Attribute::Integer(v) => Some(*v as f64),
        Attribute::Typed { value, .. } => number(value),
        _ => None,
    }
}

fn type_name(e: &RawEntity) -> String {
    match e {
        RawEntity::Simple { name, .. } => name.to_ascii_lowercase(),
        RawEntity::Complex { parts, .. } => format!(
            "({})",
            parts
                .iter()
                .map(|p| p.name.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
}

fn ids(v: &[u64]) -> String {
    let v: Vec<String> = v.iter().map(|i| format!("#{i}")).collect();
    format!("[{}]", v.join(", "))
}

// ---------------------------------------------------------------------------------------------
// thread and turned_knurl
// ---------------------------------------------------------------------------------------------

const LENGTH_ITEM: &[&str] = &["measure_representation_item", "length_measure_with_unit"];
const RATIO_ITEM: &[&str] = &["measure_representation_item", "ratio_measure_with_unit"];
const ANGLE_ITEM: &[&str] = &[
    "measure_representation_item",
    "plane_angle_measure_with_unit",
];
const DESCRIPTIVE_ITEM: &[&str] = &["descriptive_representation_item"];

/// thread WR1, turned_knurl WR2: exactly one property definition of `x` has exactly one
/// `shape_representation_with_parameters` with `lo..=hi` items.
fn one_parameter_set(ctx: &Ctx, x: u64, lo: usize, hi: usize) -> Option<String> {
    let pds = ctx.parameter_representations(x);
    let good = pds
        .iter()
        .filter(|(_, reps)| {
            reps.iter()
                .filter(|&&r| (lo..=hi).contains(&ctx.items(r).len()))
                .count()
                == 1
        })
        .count();
    (good != 1).then(|| {
        format!(
            "{good} of its property definitions {} have exactly one shape_representation_with_parameters of {lo} to {hi} items; exactly 1 required",
            ids(&pds.iter().map(|(pd, _)| *pd).collect::<Vec<_>>())
        )
    })
}

/// The thread and knurl parameter rules: in every `shape_representation_with_parameters` of
/// every property definition of `x`, the number of items of `types` named `name` (whose
/// description is one of `descriptions`, if given) is exactly 1 (`at_most` false) or at most 1.
fn parameter_count(
    ctx: &Ctx,
    x: u64,
    types: &[&str],
    name: &str,
    descriptions: Option<&[&str]>,
    at_most: bool,
) -> Option<String> {
    for (_, reps) in ctx.parameter_representations(x) {
        for rep in reps {
            let n = ctx
                .named_items(rep, types, name)
                .into_iter()
                .filter(|&i| {
                    descriptions.is_none_or(|d| {
                        ctx.text(i, "descriptive_representation_item", "description")
                            .is_some_and(|t| d.contains(&t.as_str()))
                    })
                })
                .count();
            if (at_most && n > 1) || (!at_most && n != 1) {
                let want = if at_most { "at most 1" } else { "exactly 1" };
                let described = descriptions
                    .map(|d| format!(" described as one of {d:?}"))
                    .unwrap_or_default();
                return Some(format!(
                    "#{rep} has {n} {} items named '{name}'{described}; {want} required",
                    types.join("+")
                ));
            }
        }
    }
    None
}

/// thread WR12/WR13/WR16, turned_knurl WR11/WR12: for every `product_definition_shape` of
/// `x`, exactly one of its shape aspects (described as `aspect`, if given) is the related
/// aspect of a number of `shape_defining_relationship`s described as `usage` whose relating
/// aspect is a `relating` that `count_ok` accepts.
fn feature_aspect(
    ctx: &Ctx,
    x: u64,
    aspect: Option<&str>,
    usage: &str,
    relating: &str,
    count_ok: fn(usize) -> bool,
) -> Option<String> {
    for pds in ctx
        .property_definitions(x)
        .into_iter()
        .filter(|&pd| ctx.is(pd, "product_definition_shape"))
    {
        let n = ctx
            .used_in(pds, "shape_aspect", "of_shape")
            .into_iter()
            .filter(|&sa| {
                aspect.is_none_or(|a| {
                    ctx.text(sa, "shape_aspect", "description").as_deref() == Some(a)
                }) && count_ok(
                    ctx.used_in(sa, "shape_aspect_relationship", "related_shape_aspect")
                        .into_iter()
                        .filter(|&sar| {
                            ctx.is(sar, "shape_defining_relationship")
                                && ctx
                                    .text(sar, "shape_aspect_relationship", "description")
                                    .as_deref()
                                    == Some(usage)
                                && ctx
                                    .reference(
                                        sar,
                                        "shape_aspect_relationship",
                                        "relating_shape_aspect",
                                    )
                                    .is_some_and(|r| ctx.is(r, relating))
                        })
                        .count(),
                )
            })
            .count();
        if n != 1 {
            let described = aspect
                .map(|a| format!(" described '{a}'"))
                .unwrap_or_default();
            return Some(format!(
                "#{pds} has {n} shape aspects{described} with the required '{usage}' shape_defining_relationship from a {relating}; exactly 1 required"
            ));
        }
    }
    None
}

fn exactly_one(n: usize) -> bool {
    n == 1
}

fn at_most_one(n: usize) -> bool {
    n <= 1
}

/// thread WR1: one property definition with one parameter representation of 8 to 11 items.
fn thread_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    one_parameter_set(ctx, x, 8, 11)
}
/// thread WR2: exactly one 'major diameter' length item.
fn thread_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "major diameter", None, false)
}
/// thread WR3: at most one 'minor diameter' length item.
fn thread_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "minor diameter", None, true)
}
/// thread WR4: at most one 'pitch diameter' length item.
fn thread_wr4(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "pitch diameter", None, true)
}
/// thread WR5: exactly one 'number of threads' ratio item.
fn thread_wr5(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, RATIO_ITEM, "number of threads", None, false)
}
/// thread WR6: exactly one 'fit class' descriptive item.
fn thread_wr6(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, DESCRIPTIVE_ITEM, "fit class", None, false)
}
/// thread WR7: exactly one 'form' descriptive item.
fn thread_wr7(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, DESCRIPTIVE_ITEM, "form", None, false)
}
/// thread WR8: exactly one 'hand' descriptive item, 'left' or 'right'.
fn thread_wr8(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(
        ctx,
        x,
        DESCRIPTIVE_ITEM,
        "hand",
        Some(&["left", "right"]),
        false,
    )
}
/// thread WR9: at most one 'qualifier' descriptive item.
fn thread_wr9(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, DESCRIPTIVE_ITEM, "qualifier", None, true)
}
/// thread WR10: exactly one 'thread side' descriptive item, 'internal' or 'external'.
fn thread_wr10(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(
        ctx,
        x,
        DESCRIPTIVE_ITEM,
        "thread side",
        Some(&["internal", "external"]),
        false,
    )
}
/// thread WR11: at most one 'crest' length item.
fn thread_wr11(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "crest", None, true)
}
/// thread WR12: exactly one 'partial area occurrence' aspect, with one 'applied area usage'
/// from an `applied_area`.
fn thread_wr12(ctx: &Ctx, x: u64) -> Option<String> {
    feature_aspect(
        ctx,
        x,
        Some("partial area occurrence"),
        "applied area usage",
        "applied_area",
        exactly_one,
    )
}
/// thread WR13: exactly one aspect with one 'applied shape' from a `shape_aspect`.
fn thread_wr13(ctx: &Ctx, x: u64) -> Option<String> {
    feature_aspect(ctx, x, None, "applied shape", "shape_aspect", exactly_one)
}
/// thread WR14: at most one 'fit class 2' descriptive item.
fn thread_wr14(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, DESCRIPTIVE_ITEM, "fit class 2", None, true)
}
/// thread WR15: at most one 'nominal size' length item.
fn thread_wr15(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "nominal size", None, true)
}
/// thread WR16: exactly one 'thread runout' aspect, with at most one 'thread runout usage'
/// from a `thread_runout`.
fn thread_wr16(ctx: &Ctx, x: u64) -> Option<String> {
    feature_aspect(
        ctx,
        x,
        Some("thread runout"),
        "thread runout usage",
        "thread_runout",
        at_most_one,
    )
}

fn knurl_pattern(ctx: &Ctx, x: u64) -> Option<String> {
    ctx.text(x, "characterized_object", "description")
}

/// turned_knurl WR1: the description is 'diamond', 'diagonal' or 'straight'.
fn turned_knurl_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let d = knurl_pattern(ctx, x)?;
    (!["diamond", "diagonal", "straight"].contains(&d.as_str()))
        .then(|| format!("description '{d}' is not 'diamond', 'diagonal' or 'straight'"))
}
/// turned_knurl WR2: one property definition with one parameter representation of 6 to 9
/// items.
fn turned_knurl_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    one_parameter_set(ctx, x, 6, 9)
}
/// turned_knurl WR3: at most one 'number of teeth' item whose value is a COUNT_MEASURE.
fn turned_knurl_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    for (_, reps) in ctx.parameter_representations(x) {
        for rep in reps {
            let n = ctx
                .named_items(rep, &["measure_representation_item"], "number of teeth")
                .into_iter()
                .filter(|&i| {
                    matches!(ctx.attr(i, "measure_with_unit", "value_component"),
                        Some(Attribute::Typed { type_name, .. }) if type_name.eq_ignore_ascii_case("count_measure"))
                })
                .count();
            if n > 1 {
                return Some(format!(
                    "#{rep} has {n} COUNT_MEASURE items named 'number of teeth'; at most 1 required"
                ));
            }
        }
    }
    None
}
/// turned_knurl WR4: exactly one 'major diameter' length item.
fn turned_knurl_wr4(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "major diameter", None, false)
}
/// turned_knurl WR5: exactly one 'nominal diameter' length item.
fn turned_knurl_wr5(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "nominal diameter", None, false)
}
/// turned_knurl WR6: at most one 'tooth depth' length item.
fn turned_knurl_wr6(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "tooth depth", None, true)
}
/// turned_knurl WR7: at most one 'root fillet' length item.
fn turned_knurl_wr7(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "root fillet", None, true)
}
/// turned_knurl WR8: exactly one 'diametral pitch' length item.
fn turned_knurl_wr8(ctx: &Ctx, x: u64) -> Option<String> {
    parameter_count(ctx, x, LENGTH_ITEM, "diametral pitch", None, false)
}
/// turned_knurl WR9: a diamond or diagonal knurl has exactly one 'helix angle' plane angle
/// item.
fn turned_knurl_wr9(ctx: &Ctx, x: u64) -> Option<String> {
    let d = knurl_pattern(ctx, x)?;
    if !["diamond", "diagonal"].contains(&d.as_str()) {
        return None;
    }
    parameter_count(ctx, x, ANGLE_ITEM, "helix angle", None, false)
}
/// turned_knurl WR10: a diagonal knurl has exactly one 'helix hand' descriptive item.
fn turned_knurl_wr10(ctx: &Ctx, x: u64) -> Option<String> {
    if knurl_pattern(ctx, x)? != "diagonal" {
        return None;
    }
    parameter_count(ctx, x, DESCRIPTIVE_ITEM, "helix hand", None, false)
}
/// turned_knurl WR11: as thread WR12.
fn turned_knurl_wr11(ctx: &Ctx, x: u64) -> Option<String> {
    thread_wr12(ctx, x)
}
/// turned_knurl WR12: as thread WR13.
fn turned_knurl_wr12(ctx: &Ctx, x: u64) -> Option<String> {
    thread_wr13(ctx, x)
}

// ---------------------------------------------------------------------------------------------
// default_tolerance_table and its cells
// ---------------------------------------------------------------------------------------------

/// default_tolerance_table WR1: every item is a `default_tolerance_table_cell`.
fn default_tolerance_table_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let bad: Vec<u64> = ctx
        .items(x)
        .into_iter()
        .filter(|&i| !ctx.is(i, "default_tolerance_table_cell"))
        .collect();
    (!bad.is_empty()).then(|| format!("items {} are not default_tolerance_table_cells", ids(&bad)))
}

/// default_tolerance_table WR2: no `representation_relationship` from it named before
/// 'general tolerance definition' (`<`, as written), none named 'general tolerance
/// definition' to a representation named before 'default tolerance', and none to it.
fn default_tolerance_table_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    let rr = "representation_relationship";
    for r in ctx.used_in(x, rr, "rep_1") {
        let Some(name) = ctx.text(r, rr, "name") else {
            continue;
        };
        if name.as_str() < "general tolerance definition" {
            return Some(format!(
                "#{r} relates it under the name '{name}', which sorts before 'general tolerance definition'"
            ));
        }
        if name == "general tolerance definition"
            && let Some(rep_2) = ctx.reference(r, rr, "rep_2")
            && let Some(n2) = ctx.text(rep_2, "representation", "name")
            && n2.as_str() < "default tolerance"
        {
            return Some(format!(
                "#{r} relates it to #{rep_2} named '{n2}', which sorts before 'default tolerance'"
            ));
        }
    }
    let to = ctx.used_in(x, rr, "rep_2");
    (!to.is_empty()).then(|| format!("{} relate other representations to it", ids(&to)))
}

/// The cell's `item_element` elements; `None` when not resolvable.
fn cell_elements(ctx: &Ctx, x: u64) -> Option<Vec<u64>> {
    let a = ctx.attr(x, "compound_representation_item", "item_element")?;
    let mut out = Vec::new();
    refs(a, &mut out);
    Some(out)
}

fn count_named(ctx: &Ctx, items: &[u64], ty: &str, name: &str) -> usize {
    items
        .iter()
        .filter(|&&i| ctx.is(i, ty) && ctx.item_name(i).as_deref() == Some(name))
        .count()
}

/// default_tolerance_table_cell WR1: exactly one `default_tolerance_table` lists it.
fn default_tolerance_table_cell_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let tables: Vec<u64> = ctx
        .used_in(x, "representation", "items")
        .into_iter()
        .filter(|&r| ctx.is(r, "default_tolerance_table"))
        .collect();
    (tables.len() != 1).then(|| {
        format!(
            "listed by {} default_tolerance_tables {}; exactly 1 required",
            tables.len(),
            ids(&tables)
        )
    })
}

/// default_tolerance_table_cell WR2 (function `default_tolerance_table_cell_wr2`): at most 5
/// elements.
fn default_tolerance_table_cell_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    let e = cell_elements(ctx, x)?;
    (e.len() > 5).then(|| format!("{} elements; at most 5 allowed", e.len()))
}

/// default_tolerance_table_cell WR3 (function `…_wr3`): one 'significant number of digits'
/// measure item, or one 'lower limit' and one 'upper limit'.
fn default_tolerance_table_cell_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    let e = cell_elements(ctx, x)?;
    let m = "measure_representation_item";
    let ok = count_named(ctx, &e, m, "significant number of digits") == 1
        || (count_named(ctx, &e, m, "lower limit") == 1
            && count_named(ctx, &e, m, "upper limit") == 1);
    (!ok).then(|| {
        "neither one 'significant number of digits' nor one 'lower limit' and one 'upper limit' measure item".to_string()
    })
}

/// default_tolerance_table_cell WR4 (function `…_wr4`): one 'plus minus tolerance value'
/// measure item, or one 'lower tolerance value' and one 'upper tolerance value'.
fn default_tolerance_table_cell_wr4(ctx: &Ctx, x: u64) -> Option<String> {
    let e = cell_elements(ctx, x)?;
    let m = "measure_representation_item";
    let ok = count_named(ctx, &e, m, "plus minus tolerance value") == 1
        || (count_named(ctx, &e, m, "lower tolerance value") == 1
            && count_named(ctx, &e, m, "upper tolerance value") == 1);
    (!ok).then(|| {
        "neither one 'plus minus tolerance value' nor one 'lower tolerance value' and one 'upper tolerance value' measure item".to_string()
    })
}

/// default_tolerance_table_cell WR5 (function `…_wr5`): at most one descriptive item, named
/// 'cell description'.
fn default_tolerance_table_cell_wr5(ctx: &Ctx, x: u64) -> Option<String> {
    let e = cell_elements(ctx, x)?;
    let d = "descriptive_representation_item";
    let all = e.iter().filter(|&&i| ctx.is(i, d)).count();
    let named = count_named(ctx, &e, d, "cell description");
    (all > 1 || all != named).then(|| {
        format!("{all} descriptive items, {named} of them named 'cell description'; at most 1, so named, allowed")
    })
}

// ---------------------------------------------------------------------------------------------
// Datums, datum features, datum targets, datum systems
// ---------------------------------------------------------------------------------------------

const SA: &str = "shape_aspect";
const SAR: &str = "shape_aspect_relationship";

/// The `shape_aspect_relationship`s whose `related_shape_aspect` is `x`.
fn related_by(ctx: &Ctx, x: u64) -> Vec<u64> {
    ctx.used_in(x, SAR, "related_shape_aspect")
}

/// The `shape_aspect_relationship`s whose `relating_shape_aspect` is `x`.
fn relating_in(ctx: &Ctx, x: u64) -> Vec<u64> {
    ctx.used_in(x, SAR, "relating_shape_aspect")
}

fn product_definitional_is(ctx: &Ctx, x: u64, want: bool) -> Option<String> {
    let v = ctx.logical(x, SA, "product_definitional")?;
    (v != want).then(|| {
        format!(
            "product_definitional is {}; {} required",
            if v { ".T." } else { ".F." },
            if want { ".T." } else { ".F." }
        )
    })
}

fn name_is_empty(ctx: &Ctx, x: u64) -> Option<String> {
    let n = ctx.text(x, SA, "name")?;
    (!n.is_empty()).then(|| format!("name is '{n}'; '' required"))
}

/// datum WR1: a `common_datum`, or established by at least one relationship from a datum
/// feature or a datum target (exactly one of the two types) — not both.
fn datum_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let common = ctx.is(x, "common_datum");
    let established = related_by(ctx, x).into_iter().any(|r| {
        ctx.reference(r, SAR, "relating_shape_aspect")
            .is_some_and(|f| {
                usize::from(ctx.is(f, "datum_feature")) + usize::from(ctx.is(f, "datum_target"))
                    == 1
            })
    });
    (common == established).then(|| {
        if common {
            "a common_datum established by a datum feature or target".to_string()
        } else {
            "established by no datum feature or datum target".to_string()
        }
    })
}

/// datum WR2: at most one relationship from a datum feature.
fn datum_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    let features: Vec<u64> = related_by(ctx, x)
        .into_iter()
        .filter(|&r| {
            ctx.reference(r, SAR, "relating_shape_aspect")
                .is_some_and(|f| ctx.is(f, "datum_feature"))
        })
        .collect();
    (features.len() > 1).then(|| {
        format!(
            "established by {} relationships from datum features {}; at most 1 allowed",
            features.len(),
            ids(&features)
        )
    })
}

/// datum WR3: product_definitional is FALSE.
fn datum_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    product_definitional_is(ctx, x, false)
}

/// datum WR4: the name is empty.
fn datum_wr4(ctx: &Ctx, x: u64) -> Option<String> {
    name_is_empty(ctx, x)
}

/// datum UR1: (identification, of_shape) is unique among datums.
fn datum_ur1(ctx: &Ctx, x: u64) -> Option<String> {
    let id = ctx.text(x, "datum", "identification")?;
    let of = ctx.reference(x, SA, "of_shape")?;
    Some(format!("identification '{id}' on #{of}"))
}

/// datum_feature WR1: exactly one relationship from it to a datum.
fn datum_feature_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let n = relating_in(ctx, x)
        .into_iter()
        .filter(|&r| {
            ctx.reference(r, SAR, "related_shape_aspect")
                .is_some_and(|d| ctx.is(d, "datum"))
        })
        .count();
    (n != 1).then(|| format!("{n} relationships to a datum; exactly 1 required"))
}

/// datum_feature WR2: product_definitional is TRUE.
fn datum_feature_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    product_definitional_is(ctx, x, true)
}

/// datum_target.the_datum: the datums its relationships relate it to.
fn the_datum(ctx: &Ctx, x: u64) -> Vec<u64> {
    relating_in(ctx, x)
        .into_iter()
        .filter_map(|r| ctx.reference(r, SAR, "related_shape_aspect"))
        .filter(|&d| ctx.is(d, "datum"))
        .collect()
}

/// datum_target WR1: it establishes exactly one datum.
fn datum_target_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let d = the_datum(ctx, x);
    (d.len() != 1).then(|| format!("relates to datums {}; exactly 1 required", ids(&d)))
}

/// datum_target WR2: product_definitional is TRUE.
fn datum_target_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    product_definitional_is(ctx, x, true)
}

/// datum_target WR3: `0 < VALUE(target_id)` (a target_id that is not a number makes the rule
/// UNKNOWN, which does not violate it).
fn datum_target_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    let id = ctx.text(x, "datum_target", "target_id")?;
    let v: f64 = id.trim().parse().ok()?;
    (v <= 0.0).then(|| format!("target_id '{id}' is not positive"))
}

/// datum_target WR4: the name is empty.
fn datum_target_wr4(ctx: &Ctx, x: u64) -> Option<String> {
    name_is_empty(ctx, x)
}

/// datum_target WR5: no description.
fn datum_target_wr5(ctx: &Ctx, x: u64) -> Option<String> {
    let d = ctx.text(x, SA, "description")?;
    Some(format!("description '{d}'; none allowed"))
}

/// datum_target UR1: (the datum's identification + target_id, of_shape) is unique.
fn datum_target_ur1(ctx: &Ctx, x: u64) -> Option<String> {
    let datum = *the_datum(ctx, x).first()?;
    let label = ctx.text(datum, "datum", "identification")?;
    let id = ctx.text(x, "datum_target", "target_id")?;
    let of = ctx.reference(x, SA, "of_shape")?;
    Some(format!("target '{label}{id}' on #{of}"))
}

/// placed_datum_target_feature WR1: the description is a target shape.
fn placed_datum_target_feature_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let d = ctx.text(x, SA, "description")?;
    (!["point", "line", "rectangle", "circle", "circular curve"].contains(&d.as_str()))
        .then(|| format!("description '{d}' is not a datum target shape"))
}

/// The `shape_representation_with_parameters` of a placed datum target, through the
/// `shape_definition_representation`s of its property definitions
/// (get_shape_aspect_property_definition_representations).
fn target_parameters(ctx: &Ctx, x: u64) -> Vec<u64> {
    let mut reps = BTreeSet::new();
    for pd in ctx.property_definitions(x) {
        for pdr in ctx.used_in(pd, "property_definition_representation", "definition") {
            if ctx.is(pdr, "shape_definition_representation")
                && let Some(rep) = ctx.reference(
                    pdr,
                    "property_definition_representation",
                    "used_representation",
                )
                && ctx.is(rep, "shape_representation_with_parameters")
            {
                reps.insert(rep);
            }
        }
    }
    reps.into_iter().collect()
}

/// placed_datum_target_feature WR2: exactly one parameter representation.
fn placed_datum_target_feature_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    let mut n = 0;
    for pd in ctx.property_definitions(x) {
        for pdr in ctx.used_in(pd, "property_definition_representation", "definition") {
            if ctx.is(pdr, "shape_definition_representation")
                && ctx
                    .reference(
                        pdr,
                        "property_definition_representation",
                        "used_representation",
                    )
                    .is_some_and(|r| ctx.is(r, "shape_representation_with_parameters"))
            {
                n += 1;
            }
        }
    }
    (n != 1).then(|| {
        format!("{n} shape_definition_representations with parameters; exactly 1 required")
    })
}

/// placed_datum_target_feature WR3 (function `valid_datum_target_parameters`): exactly one
/// parameter representation has one 'orientation' placement, and the parameters match the
/// target's shape (description).
fn placed_datum_target_feature_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    let reps = target_parameters(ctx, x);
    let oriented = reps
        .iter()
        .filter(|&&r| {
            ctx.items(r)
                .into_iter()
                .filter(|&i| {
                    ctx.is(i, "placement") && ctx.item_name(i).as_deref() == Some("orientation")
                })
                .count()
                == 1
        })
        .count();
    if oriented != 1 {
        return Some(format!(
            "{oriented} parameter representations with one 'orientation' placement; exactly 1 required"
        ));
    }
    let with_items = |n: usize| reps.iter().filter(|&&r| ctx.items(r).len() == n).count();
    let with_length = |name: &str| {
        reps.iter()
            .filter(|&&r| ctx.named_items(r, LENGTH_ITEM, name).len() == 1)
            .count()
    };
    // CASE on an indeterminate description executes nothing: the function returns ?, and the
    // rule is UNKNOWN, which does not violate it.
    let shape = ctx.text(x, SA, "description")?;
    let ok = match shape.as_str() {
        "point" => with_items(1) == 1,
        "circle" | "circular curve" => with_items(2) == 1 && with_length("target diameter") == 1,
        "line" => with_length("target length") == 1,
        "rectangle" => {
            with_items(3) == 1
                && with_length("target length") == 1
                && with_length("target width") == 1
        }
        _ => false,
    };
    (!ok).then(|| format!("its parameters do not match a '{shape}' target"))
}

/// datum_system WR1: product_definitional is FALSE.
fn datum_system_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    product_definitional_is(ctx, x, false)
}

/// datum_system UR1: (of_shape, name) is unique among datum systems.
fn datum_system_ur1(ctx: &Ctx, x: u64) -> Option<String> {
    let of = ctx.reference(x, SA, "of_shape")?;
    let name = ctx.text(x, SA, "name")?;
    Some(format!("name '{name}' on #{of}"))
}

/// datum_reference_compartment.owner (INVERSE, exactly one): one datum system lists it (PMI
/// practice §6.9.7: a compartment is never shared).
fn datum_reference_compartment_owner(ctx: &Ctx, x: u64) -> Option<String> {
    let owners = ctx.used_in(x, "datum_system", "constituents");
    (owners.len() != 1).then(|| {
        format!(
            "listed by datum systems {}; exactly 1 required",
            ids(&owners)
        )
    })
}

// ---------------------------------------------------------------------------------------------
// Geometric tolerances
// ---------------------------------------------------------------------------------------------

const GT: &str = "geometric_tolerance";

/// geometric_tolerance WR1: the magnitude is not negative.
fn geometric_tolerance_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let m = ctx.reference(x, GT, "magnitude")?;
    let v = number(ctx.attr(m, "measure_with_unit", "value_component")?)?;
    (v < 0.0).then(|| format!("magnitude #{m} is {v}"))
}

/// geometric_tolerance WR3: a tolerance on a shape aspect relationship relates aspects of
/// the same product definition shape.
fn geometric_tolerance_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    let t = ctx.reference(x, GT, "toleranced_shape_aspect")?;
    if !ctx.is(t, SAR) {
        return None;
    }
    let a = ctx.reference(
        ctx.reference(t, SAR, "relating_shape_aspect")?,
        SA,
        "of_shape",
    )?;
    let b = ctx.reference(
        ctx.reference(t, SAR, "related_shape_aspect")?,
        SA,
        "of_shape",
    )?;
    (a != b).then(|| format!("relationship #{t} relates aspects of #{a} and #{b}"))
}

/// geometric_tolerance WR5: it is the relating tolerance of at most one 'composite
/// tolerance' relationship.
fn geometric_tolerance_wr5(ctx: &Ctx, x: u64) -> Option<String> {
    let composite: Vec<u64> = ctx
        .used_in(
            x,
            "geometric_tolerance_relationship",
            "relating_geometric_tolerance",
        )
        .into_iter()
        .filter(|&r| {
            ctx.text(r, "geometric_tolerance_relationship", "name")
                .as_deref()
                == Some("composite tolerance")
        })
        .collect();
    (composite.len() > 1).then(|| {
        format!(
            "relating tolerance of composite relationships {}; at most 1 allowed",
            ids(&composite)
        )
    })
}

/// geometric_tolerance_with_datum_reference WR1: a datum system is its only datum reference.
fn geometric_tolerance_with_datum_reference_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let refs = ctx.references(
        x,
        "geometric_tolerance_with_datum_reference",
        "datum_system",
    );
    let systems = refs.iter().filter(|&&r| ctx.is(r, "datum_system")).count();
    (systems > 0 && refs.len() != 1).then(|| {
        format!(
            "datum_system {} holds a datum_system among {} references; a datum_system must be alone",
            ids(&refs),
            refs.len()
        )
    })
}

/// flatness, straightness, roundness and cylindricity tolerance WR1: no datum reference.
fn no_datum_reference(ctx: &Ctx, x: u64) -> Option<String> {
    ctx.is(x, "geometric_tolerance_with_datum_reference")
        .then(|| "a form tolerance with a datum reference".to_string())
}

const GTR: &str = "geometric_tolerance_relationship";

fn is_composite(ctx: &Ctx, x: u64) -> bool {
    ctx.text(x, GTR, "name").as_deref() == Some("composite tolerance")
}

/// geometric_tolerance_relationship WR1: a composite relates two position, two line profile
/// or two surface profile tolerances.
fn geometric_tolerance_relationship_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    if !is_composite(ctx, x) {
        return None;
    }
    let a = ctx.reference(x, GTR, "relating_geometric_tolerance")?;
    let b = ctx.reference(x, GTR, "related_geometric_tolerance")?;
    let ok = [
        "position_tolerance",
        "line_profile_tolerance",
        "surface_profile_tolerance",
    ]
    .iter()
    .any(|t| ctx.is(a, t) && ctx.is(b, t));
    (!ok).then(|| format!("composite of #{a} and #{b}, not two position, line profile or surface profile tolerances"))
}

/// geometric_tolerance_relationship WR2: a composite relates tolerances of the same target.
fn geometric_tolerance_relationship_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    if !is_composite(ctx, x) {
        return None;
    }
    let a = ctx.reference(x, GTR, "relating_geometric_tolerance")?;
    let b = ctx.reference(x, GTR, "related_geometric_tolerance")?;
    let ta = ctx.reference(a, GT, "toleranced_shape_aspect")?;
    let tb = ctx.reference(b, GT, "toleranced_shape_aspect")?;
    (ta != tb).then(|| format!("composite of #{a} on #{ta} and #{b} on #{tb}"))
}

/// WR3: the tolerance relationship graph is acyclic (all relationship names).
fn geometric_tolerance_relationship_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    let start = ctx.reference(x, GTR, "related_geometric_tolerance")?;
    let mut pending = vec![ctx.reference(x, GTR, "relating_geometric_tolerance")?];
    let mut seen = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if node == start {
            return Some(format!("cyclic tolerance relationship through #{start}"));
        }
        if seen.insert(node) {
            pending.extend(
                ctx.used_in(node, GTR, "related_geometric_tolerance")
                    .into_iter()
                    .filter_map(|r| ctx.reference(r, GTR, "relating_geometric_tolerance")),
            );
        }
    }
    None
}

/// plus_minus_tolerance UR1: one plus/minus tolerance per toleranced dimension.
fn plus_minus_tolerance_ur1(ctx: &Ctx, x: u64) -> Option<String> {
    let d = ctx.reference(x, "plus_minus_tolerance", "toleranced_dimension")?;
    Some(format!("toleranced_dimension #{d}"))
}

// ---------------------------------------------------------------------------------------------
// item_identified_representation_usage (geometric_item_specific_usage)
// ---------------------------------------------------------------------------------------------

const IIRU: &str = "item_identified_representation_usage";

fn written(a: &Attribute) -> String {
    match a {
        Attribute::EntityRef(t) => format!("#{t}"),
        Attribute::List(items) => format!(
            "({})",
            items.iter().map(written).collect::<Vec<_>>().join(",")
        ),
        Attribute::Typed { type_name, value } => format!("{type_name}({})", written(value)),
        other => format!("{other:?}"),
    }
}

/// item_identified_representation_usage UR1: one usage per (representation, item).
fn item_identified_representation_usage_ur1(ctx: &Ctx, x: u64) -> Option<String> {
    let rep = ctx.reference(x, IIRU, "used_representation")?;
    let item = ctx.attr(x, IIRU, "identified_item")?;
    Some(format!(
        "used_representation #{rep} and identified_item {}",
        written(item)
    ))
}

/// item_identified_representation_usage UR2: one usage per (representation, definition).
fn item_identified_representation_usage_ur2(ctx: &Ctx, x: u64) -> Option<String> {
    let rep = ctx.reference(x, IIRU, "used_representation")?;
    let def = ctx.reference(x, IIRU, "definition")?;
    Some(format!("used_representation #{rep} and definition #{def}"))
}

/// item_identified_representation_usage WR1 (function
/// `valid_identified_item_in_representation`): the identified item, or each element of a
/// set or list of items, is in the used representation.
fn item_identified_representation_usage_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let rep = ctx.reference(x, IIRU, "used_representation")?;
    let items = match ctx.attr(x, IIRU, "identified_item")? {
        Attribute::EntityRef(t) if ctx.is(*t, "representation_item") => vec![*t],
        a @ Attribute::Typed { .. } => {
            let mut out = Vec::new();
            refs(a, &mut out);
            out
        }
        _ => return None,
    };
    let missing: Vec<u64> = items
        .into_iter()
        .filter(|&i| !ctx.using_representations(i).contains(&rep))
        .collect();
    (!missing.is_empty()).then(|| format!("identified items {} are not in #{rep}", ids(&missing)))
}

// ---------------------------------------------------------------------------------------------
// surface_texture_representation and general_property_association (surface conditions)
// ---------------------------------------------------------------------------------------------

const RR: &str = "representation_relationship";
const PDR: &str = "property_definition_representation";
const GPA: &str = "general_property_association";

/// The number of `types` in TYPEOF(i): `SIZEOF([…] * TYPEOF(i))`.
fn types_of(ctx: &Ctx, i: u64, types: &[&str]) -> usize {
    types.iter().filter(|t| ctx.is(i, t)).count()
}

/// surface_texture_representation WR1: every item is exactly one of a
/// `measure_representation_item`, a `value_range` and a `descriptive_representation_item`.
fn surface_texture_representation_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let kinds = [
        "measure_representation_item",
        "value_range",
        "descriptive_representation_item",
    ];
    let bad: Vec<u64> = ctx
        .items(x)
        .into_iter()
        .filter(|&i| types_of(ctx, i, &kinds) != 1)
        .collect();
    (!bad.is_empty()).then(|| {
        format!(
            "items {} are not exactly one of measure_representation_item, value_range, descriptive_representation_item",
            ids(&bad)
        )
    })
}

/// surface_texture_representation WR2: exactly one descriptive item, and exactly one
/// descriptive item named 'measuring method' (QUERY selects only what is TRUE: an item whose
/// name is unset is not selected).
fn surface_texture_representation_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    let d = "descriptive_representation_item";
    let items = ctx.items(x);
    let all = items.iter().filter(|&&i| ctx.is(i, d)).count();
    let named = count_named(ctx, &items, d, "measuring method");
    (all != 1 || named != 1).then(|| {
        format!("{all} descriptive items, {named} of them named 'measuring method'; exactly 1, so named, required")
    })
}

/// surface_texture_representation WR3: some item is exactly one of a
/// `measure_representation_item` and a `value_range`.
fn surface_texture_representation_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    let kinds = ["measure_representation_item", "value_range"];
    let any = ctx
        .items(x)
        .into_iter()
        .any(|i| types_of(ctx, i, &kinds) == 1);
    (!any).then(|| "no item is a measure_representation_item or a value_range".to_string())
}

/// surface_texture_representation WR4: it is the `rep_1` of at most one
/// `representation_relationship` and the `rep_2` of none, and every relationship from it
/// relates a representation named 'measuring direction' (QUERY: an unset name is not
/// selected).
fn surface_texture_representation_wr4(ctx: &Ctx, x: u64) -> Option<String> {
    let from = ctx.used_in(x, RR, "rep_1");
    let to = ctx.used_in(x, RR, "rep_2");
    if from.len() > 1 || !to.is_empty() {
        return Some(format!(
            "rep_1 of {} and rep_2 of {}; at most 1 and none allowed",
            ids(&from),
            ids(&to)
        ));
    }
    let other: Vec<u64> = from
        .into_iter()
        .filter(|&r| {
            ctx.reference(r, RR, "rep_2")
                .and_then(|r2| ctx.text(r2, "representation", "name"))
                .as_deref()
                != Some("measuring direction")
        })
        .collect();
    (!other.is_empty()).then(|| {
        format!(
            "{} relate it to a representation not named 'measuring direction'",
            ids(&other)
        )
    })
}

/// surface_texture_representation WR5: exactly one `property_definition_representation` uses
/// it, and its definition is the derived definition of exactly one
/// `general_property_association` whose base is a `general_property` named
/// 'surface_condition'.
fn surface_texture_representation_wr5(ctx: &Ctx, x: u64) -> Option<String> {
    let pdrs = ctx.used_in(x, PDR, "used_representation");
    if pdrs.len() != 1 {
        return Some(format!(
            "used by property_definition_representations {}; exactly 1 required",
            ids(&pdrs)
        ));
    }
    // An unset definition is treated as UNKNOWN (not a violation), as elsewhere here.
    let def = ctx.reference(pdrs[0], PDR, "definition")?;
    let gpas: Vec<u64> = ctx
        .used_in(def, GPA, "derived_definition")
        .into_iter()
        .filter(|&g| {
            ctx.reference(g, GPA, "base_definition").is_some_and(|b| {
                ctx.is(b, "general_property")
                    && ctx.text(b, "general_property", "name").as_deref()
                        == Some("surface_condition")
            })
        })
        .collect();
    (gpas.len() != 1).then(|| {
        format!(
            "the definition of #{} has general_property_associations {} with the general property 'surface_condition'; exactly 1 required",
            pdrs[0],
            ids(&gpas)
        )
    })
}

/// The derived definition is a `dimensional_location`, a `dimensional_size` or a
/// `geometric_tolerance` (general_property_association WR1 and WR2's first disjunct).
fn derives_dimension_or_tolerance(ctx: &Ctx, derived: u64) -> bool {
    types_of(
        ctx,
        derived,
        &[
            "dimensional_location",
            "dimensional_size",
            "geometric_tolerance",
        ],
    ) > 0
}

/// general_property_association WR1: unless the derived definition is a dimension or a
/// tolerance, no other association derives it.
fn general_property_association_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let derived = ctx.reference(x, GPA, "derived_definition")?;
    if derives_dimension_or_tolerance(ctx, derived) {
        return None;
    }
    let all = ctx.used_in(derived, GPA, "derived_definition");
    (all.len() != 1).then(|| {
        format!(
            "derived definition #{derived} is derived by associations {}; exactly 1 required",
            ids(&all)
        )
    })
}

/// general_property_association WR2: unless the derived definition is a dimension or a
/// tolerance, its name equals the base definition's (the derived definition is then a
/// `property_definition`, `action_property` or `resource_property`; an unset name makes the
/// rule UNKNOWN).
fn general_property_association_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    let derived = ctx.reference(x, GPA, "derived_definition")?;
    if derives_dimension_or_tolerance(ctx, derived) {
        return None;
    }
    let base = ctx.reference(x, GPA, "base_definition")?;
    let want = ctx.text(base, "general_property", "name")?;
    let name = [
        "property_definition",
        "action_property",
        "resource_property",
    ]
    .iter()
    .find_map(|e| ctx.text(derived, e, "name"))?;
    (name != want).then(|| {
        format!("derived definition #{derived} is named '{name}', its base #{base} '{want}'")
    })
}

// ---------------------------------------------------------------------------------------------
// mechanical_design_and_draughting_relationship
// ---------------------------------------------------------------------------------------------

/// mechanical_design_and_draughting_relationship WR1–WR3: `NOT (ty IN TYPEOF(rep_2)) OR
/// (ty IN TYPEOF(rep_1)) OR (shape_representation IN TYPEOF(rep_1))`.
fn rep_1_matches_rep_2(ctx: &Ctx, x: u64, ty: &str) -> Option<String> {
    let rep_1 = ctx.reference(x, RR, "rep_1")?;
    let rep_2 = ctx.reference(x, RR, "rep_2")?;
    (ctx.is(rep_2, ty) && !ctx.is(rep_1, ty) && !ctx.is(rep_1, "shape_representation")).then(|| {
        format!(
            "rep_2 #{rep_2} is a {ty}, rep_1 #{rep_1} neither a {ty} nor a shape_representation"
        )
    })
}

/// mechanical_design_and_draughting_relationship WR1: a `draughting_model` is related from a
/// `draughting_model` or a `shape_representation`.
fn mechanical_design_and_draughting_relationship_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    rep_1_matches_rep_2(ctx, x, "draughting_model")
}

/// mechanical_design_and_draughting_relationship WR2: likewise a
/// `mechanical_design_geometric_presentation_representation`.
fn mechanical_design_and_draughting_relationship_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    rep_1_matches_rep_2(
        ctx,
        x,
        "mechanical_design_geometric_presentation_representation",
    )
}

/// mechanical_design_and_draughting_relationship WR3: likewise a
/// `mechanical_design_shaded_presentation_representation`.
fn mechanical_design_and_draughting_relationship_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    rep_1_matches_rep_2(
        ctx,
        x,
        "mechanical_design_shaded_presentation_representation",
    )
}

// ---------------------------------------------------------------------------------------------
// Global rules
// ---------------------------------------------------------------------------------------------

/// The property definition names `surface_condition_correlation` constrains.
const SURFACE_CONDITIONS: [&str; 6] = [
    "visual appearance",
    "tactile appearance",
    "contact ratio",
    "hardness",
    "treatment result",
    "surface texture",
];

/// restrict_representation_for_surface_condition WR1, as the schema states it:
///
/// ```text
/// RULE restrict_representation_for_surface_condition FOR (property_definition_representation);
/// WHERE
///   WR1: SIZEOF(QUERY(pdr
///                     <* property_definition_representation
///                     | NOT surface_condition_correlation(pdr.definition, pdr.used_representation))) =
///        0;
/// END_RULE;
///
/// FUNCTION surface_condition_correlation(pd : property_definition;
///                                        rep : representation) : LOGICAL;
///   CASE pd.name OF
///     'visual appearance', 'tactile appearance', 'contact ratio', 'hardness', 'treatment result', 'surface texture' : RETURN(pd.name =
///                                                                                                                            rep.name);
///     OTHERWISE: RETURN(UNKNOWN);
///   END_CASE;
/// END_FUNCTION;
/// ```
///
/// QUERY selects the representations for which the correlation is FALSE: a definition named
/// one of [`SURFACE_CONDITIONS`] (exactly, case and all) whose representation is named
/// otherwise. Any other name is UNKNOWN, as is an unset name on either side; neither is
/// selected. `pdr.definition` is a `represented_definition`, of which only a
/// `property_definition` (or subtype) is type compatible with the function's `pd`; for the
/// others (`general_property`, `shape_aspect`, the relationships) the call has no defined
/// value, so it is read as indeterminate and the rule is not violated.
fn restrict_representation_for_surface_condition_wr1(ctx: &Ctx) -> Vec<(u64, String)> {
    ctx.doc
        .ids()
        .filter(|&pdr| ctx.is(pdr, PDR))
        .filter_map(|pdr| {
            let pd = ctx.reference(pdr, PDR, "definition")?;
            let rep = ctx.reference(pdr, PDR, "used_representation")?;
            if !ctx.is(pd, "property_definition") {
                return None;
            }
            let name = ctx.text(pd, "property_definition", "name")?;
            if !SURFACE_CONDITIONS.contains(&name.as_str()) {
                return None;
            }
            let rep_name = ctx.text(rep, "representation", "name")?;
            (rep_name != name).then(|| {
                (
                    pdr,
                    format!(
                        "property definition #{pd} is named '{name}', its representation #{rep} '{rep_name}'"
                    ),
                )
            })
        })
        .collect()
}

// AP242 ed4 long form: default_model_geometric_view's two name slots
// agree, both descriptions are absent, and of_shape is unique per model.
fn default_view_ur1(ctx: &Ctx, x: u64) -> Option<String> {
    ctx.reference(x, "shape_aspect", "of_shape")
        .map(|id| id.to_string())
}
fn default_view_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let a = ctx.text(x, "characterized_object", "name")?;
    let b = ctx.text(x, "shape_aspect", "name")?;
    (a != b).then(|| "characterized_object.name differs from shape_aspect.name".into())
}
fn default_view_wr2(ctx: &Ctx, x: u64) -> Option<String> {
    ctx.text(x, "characterized_object", "description")
        .map(|_| "characterized_object.description exists".into())
}
fn default_view_wr3(ctx: &Ctx, x: u64) -> Option<String> {
    ctx.text(x, "shape_aspect", "description")
        .map(|_| "shape_aspect.description exists".into())
}
fn characterized_item_ur1(ctx: &Ctx, x: u64) -> Option<String> {
    let item = ctx.reference(x, "characterized_item_within_representation", "item")?;
    let rep = ctx.reference(x, "characterized_item_within_representation", "rep")?;
    Some(format!("{item},{rep}"))
}
fn characterized_item_wr1(ctx: &Ctx, x: u64) -> Option<String> {
    let item = ctx.reference(x, "characterized_item_within_representation", "item")?;
    let rep = ctx.reference(x, "characterized_item_within_representation", "rep")?;
    (!ctx.using_representations(item).contains(&rep))
        .then(|| format!("item #{item} is not used by representation #{rep}"))
}
