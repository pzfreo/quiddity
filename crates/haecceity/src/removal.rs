//! The removal plan: which instances of a [`Document`] go when a set of instances (the *seeds*,
//! e.g. the instances the PMI reader consumed for a part) is removed, which kept instances are
//! rewritten, and — when something that stays would be left referencing a removed instance and
//! the policy does not allow removing it — a refusal naming every such instance. It is a
//! document-level operation, independent of the PMI model (design, Architecture item 6).
//!
//! **The removed set R** is the least fixpoint of:
//!
//! 1. every seed is in R;
//! 2. an instance `t` that an instance of R references is in R when (a) every instance that
//!    references `t` is in R or *does not hold* `t` (below), and (b) `t` is not *shared
//!    infrastructure*.
//!
//! *Shared infrastructure* is never removed by 2, even when only removed instances referenced
//! it (it is then kept, unreferenced, and listed in [`RemovalReport::left_unreferenced`]):
//!
//! - an instance with a part that is a [`INFRASTRUCTURE`] type: units (`named_unit`,
//!   `derived_unit`, `derived_unit_element`, `dimensional_exponents`), contexts
//!   (`representation_context`, `application_context`, `application_context_element`,
//!   `application_protocol_definition`) and product structure (`product`,
//!   `product_definition_formation`, `product_definition`, `product_definition_shape`,
//!   `product_definition_relationship`, `product_category`), except a `product_definition_shape`
//!   whose definition is a `characterized_object` (a feature definition's own shape, e.g. a
//!   `thread`'s, which thread WR13 requires): `product_definition_shape` WR1 allows only a
//!   product definition (or relationship) or a characterized object as its definition and UR1
//!   one shape per definition, so that shape is its object's alone and goes with it;
//! - the B-rep: every `topological_representation_item`, every representation a
//!   `property_definition_representation` uses for a `product_definition_shape` (the parts'
//!   shape representations), and every instance of family [`Family::Other`] reachable from
//!   those by forward references through instances of family Other (their items, geometry,
//!   topology, placements and contexts).
//!
//! Nor does rule 2 remove a presentation `representation` (a draughting model, a view): the
//! associations and items in it only use it; it is rewritten or the plan refused (below).
//!
//! A seed that is shared infrastructure is refused.
//!
//! **Blockers.** Every kept instance that references an instance of R is a blocker, classified
//! by [`instance_family`]:
//!
//! - [`Family::Presentation`] and [`Family::ValidationProperty`]: with
//!   [`PresentationPolicy::Refuse`] the plan is refused, naming each blocker. With
//!   [`PresentationPolicy::RemovePresentation`]:
//!   - a *list container* ([`LIST_CONTAINERS`]: a presentation `representation` —
//!     `draughting_model`, presentation views, sets of styled or mapped items — by its
//!     `items`) whose references to R are all direct members of that list is rewritten without
//!     them; a rewrite that would empty an aggregate whose lower bound is at least 1 is
//!     refused, naming the instance and the attribute (a model or view is an entity in its own
//!     right, with a name, a context and views of it: emptying it is the caller's decision);
//!   - a *group* ([`GROUPS`]: `annotation_plane` by `elements`, `draughting_callout` by
//!     `contents`, `invisibility` by `invisible_items`, `presentation_layer_assignment` by
//!     `assigned_items`: instances that only state something about their members) whose
//!     references to R are all direct members of its list is rewritten without them, or joins
//!     R when they were all its members (it then states nothing);
//!   - any other blocker joins R, and with it, by rule 2, what it exclusively owns (a
//!     callout's annotation occurrences, their geometry, styles and tessellations);
//!
//!   and the blockers of the grown R are classified again, until none is new. Under
//!   `RemovePresentation` two refinements of rule 2 apply: a list container or group does not
//!   hold the direct members of its list (they may go; it is rewritten), and the item a removed
//!   `draughting_model_item_association` presents (its `identified_item`, and the placeholder
//!   or image of its subtypes) is not held by presentation or validation-property instances
//!   other than draughting model item associations: those depend on the presented item and
//!   become blockers in turn (a callout's relationships, its annotation plane, the
//!   validation properties of it). An association that stays still holds what it presents.
//! - anything else ([`Family::SemanticPmi`] that is not removed, [`Family::Other`]): the plan
//!   is refused, naming it, whatever the policy.
//!
//! An instance the file's ANCHOR section names is refused too. The plan is deterministic: R is
//! a least fixpoint, independent of visiting order, and every list is in id order.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::express::{FAMILY_RULES, Family, TARGET, Ty, VALIDATION_PROPERTY_NAMES};
use crate::p21::{Attribute, Document, Edit, RawEntity, visit_attributes};

/// What to do with presentation and validation properties that reference removed instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresentationPolicy {
    /// Refuse the removal, naming them.
    #[default]
    Refuse,
    /// Remove them too (and rewrite the containers that list them).
    RemovePresentation,
}

/// Types whose instances are shared infrastructure (see the module documentation).
pub const INFRASTRUCTURE: &[&str] = &[
    "named_unit",
    "derived_unit",
    "derived_unit_element",
    "dimensional_exponents",
    "representation_context",
    "application_context",
    "application_context_element",
    "application_protocol_definition",
    "product",
    "product_definition_formation",
    "product_definition",
    "product_definition_shape",
    "product_definition_relationship",
    "product_category",
];

/// Presentation containers rewritten without their removed members, by the aggregate attribute
/// that lists them (module documentation).
pub const LIST_CONTAINERS: &[(&str, &str)] = &[("representation", "items")];

/// Presentation instances that group their members: rewritten without removed members, removed
/// when all their members are (module documentation).
pub const GROUPS: &[(&str, &str)] = &[
    ("annotation_plane", "elements"),
    ("draughting_callout", "contents"),
    ("invisibility", "invisible_items"),
    ("presentation_layer_assignment", "assigned_items"),
];

/// A kept instance that references removed ones.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Blocker {
    pub id: u64,
    /// Its type: the entity name, or `(a b c)` for a complex instance, lower case.
    pub entity: String,
    pub family: Family,
    /// The removed instances it references, ascending.
    pub references: Vec<u64>,
}

/// A container whose aggregate a rewrite would empty although its lower bound is at least 1.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Emptied {
    pub id: u64,
    pub entity: String,
    /// `entity.attribute`.
    pub attribute: String,
    pub lower: u32,
}

/// Why a removal cannot be planned: everything found, each list in id order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemovalRefusal {
    pub policy: PresentationPolicy,
    /// Seeds that name no DATA instance.
    pub missing: Vec<u64>,
    /// Seeds that are shared infrastructure, with their types.
    pub infrastructure: Vec<(u64, String)>,
    /// Kept instances referencing removed ones that the policy does not allow to remove.
    pub blockers: Vec<Blocker>,
    /// Rewrites that would empty a non-empty-bounded aggregate.
    pub emptied: Vec<Emptied>,
    /// Removed instances the ANCHOR section names: `(id, anchor name)`.
    pub anchored: Vec<(u64, String)>,
}

impl RemovalRefusal {
    fn is_empty(&self) -> bool {
        self.missing.is_empty()
            && self.infrastructure.is_empty()
            && self.blockers.is_empty()
            && self.emptied.is_empty()
            && self.anchored.is_empty()
    }
}

impl std::fmt::Display for RemovalRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "removal refused ({:?})", self.policy)?;
        for id in &self.missing {
            write!(f, "; seed #{id} is not an instance")?;
        }
        for (id, entity) in &self.infrastructure {
            write!(f, "; seed #{id} {entity} is shared infrastructure")?;
        }
        for b in &self.blockers {
            let refs: Vec<String> = b.references.iter().map(|r| format!("#{r}")).collect();
            write!(
                f,
                "; #{} {} ({}) references removed {}",
                b.id,
                b.entity,
                b.family.as_str(),
                refs.join(", ")
            )?;
        }
        for e in &self.emptied {
            write!(
                f,
                "; #{} {} would empty {} (at least {})",
                e.id, e.entity, e.attribute, e.lower
            )?;
        }
        for (id, name) in &self.anchored {
            write!(f, "; #{id} is named by anchor {name}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RemovalRefusal {}

/// A container rewritten without some members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pub id: u64,
    pub entity: String,
    /// `entity.attribute`.
    pub attribute: String,
    /// The removed members dropped from it, ascending.
    pub dropped: Vec<u64>,
}

/// What a plan does, for the caller's report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemovalReport {
    /// Removed instances, counted by type.
    pub removed_by_type: BTreeMap<String, usize>,
    /// Rewritten containers, by id.
    pub rewrites: Vec<Rewrite>,
    /// Kept instances that only removed instances referenced (shared infrastructure and
    /// presentation models, which rule 2 keeps), with their types: left unreferenced.
    pub left_unreferenced: Vec<(u64, String)>,
}

/// The instances to remove and to rewrite.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalPlan {
    pub removed: BTreeSet<u64>,
    /// Kept instances replaced by a record without their removed members.
    pub rewritten: BTreeMap<u64, RawEntity>,
    /// The removed instances that are there because of [`PresentationPolicy::RemovePresentation`]
    /// (not the seeds or what they own), with their types, in id order.
    pub presentation_removed: Vec<(u64, String)>,
    pub report: RemovalReport,
}

impl RemovalPlan {
    /// Adds the plan's removals and replacements to `edit`.
    pub fn into_edit(&self, edit: &mut Edit) {
        for &id in &self.removed {
            edit.remove(id);
        }
        for (&id, record) in &self.rewritten {
            edit.replace(id, record.clone());
        }
    }
}

/// The removal of `seeds` and what depends on them (module documentation), or why not.
// A refusal lists everything that blocks the plan; it is the cold path, so it is not boxed.
#[allow(clippy::result_large_err)]
pub fn plan(
    doc: &Document,
    seeds: &BTreeSet<u64>,
    policy: PresentationPolicy,
) -> Result<RemovalPlan, RemovalRefusal> {
    plan_with_rebindings(doc, seeds, policy, &BTreeSet::new())
}

/// Writer-only edges which will be rebound to equivalent newly emitted datums.
#[allow(
    clippy::result_large_err,
    reason = "same complete cold-path refusal as plan"
)]
pub(crate) fn plan_with_rebindings(
    doc: &Document,
    seeds: &BTreeSet<u64>,
    policy: PresentationPolicy,
    rebound: &BTreeSet<(u64, u64)>,
) -> Result<RemovalPlan, RemovalRefusal> {
    let mut refusal = RemovalRefusal {
        policy,
        ..RemovalRefusal::default()
    };
    let infrastructure = infrastructure(doc);
    for &s in seeds {
        match doc.get(s) {
            None => refusal.missing.push(s),
            Some(e) if infrastructure.contains(&s) => {
                refusal.infrastructure.push((s, entity_name(e)))
            }
            Some(_) => {}
        }
    }
    if !refusal.is_empty() {
        return Err(refusal);
    }

    let mut p = Planner {
        doc,
        policy,
        infrastructure,
        removed: seeds.clone(),
        containers: HashMap::new(),
        rebound: rebound.clone(),
    };
    p.close(seeds.iter().copied().collect());
    let owned = p.removed.clone();

    let mut rewrites: BTreeMap<u64, Vec<u64>>;
    loop {
        rewrites = BTreeMap::new();
        let mut added = Vec::new();
        for (k, references) in p.blockers() {
            let family = instance_family(doc, k);
            let presentational =
                matches!(family, Family::Presentation | Family::ValidationProperty);
            if policy == PresentationPolicy::RemovePresentation && presentational {
                match p.container(k) {
                    Some(c) if references.iter().all(|r| !c.holds.contains(r)) => {
                        if c.group && c.members.iter().all(|m| p.removed.contains(m)) {
                            added.push(k);
                        } else {
                            rewrites.insert(k, references);
                        }
                    }
                    _ => added.push(k),
                }
            } else {
                refusal.blockers.push(Blocker {
                    id: k,
                    entity: entity_name(instance(doc, k)),
                    family,
                    references,
                });
            }
        }
        if !refusal.blockers.is_empty() {
            return Err(refusal);
        }
        if added.is_empty() {
            break;
        }
        // A rewrite planned this round is reconsidered with the grown R.
        p.removed.extend(added.iter().copied());
        p.close(added);
    }

    let mut rewritten = BTreeMap::new();
    let mut report = RemovalReport::default();
    for (&k, dropped) in &rewrites {
        // A rewrite is planned only for an instance whose container was found.
        let Some(container) = p.container(k) else {
            continue;
        };
        let mut record = instance(doc, k).clone();
        let entity = entity_name(&record);
        #[allow(
            clippy::unreachable,
            reason = "find_container read the leaf and the list attribute from this record"
        )]
        let attributes = match (&mut record, container.leaf) {
            (RawEntity::Simple { attributes, .. }, None) => attributes,
            (RawEntity::Complex { parts, .. }, Some(leaf)) => &mut parts[leaf].attributes,
            _ => unreachable!("a container's position matches its record"),
        };
        #[allow(
            clippy::unreachable,
            reason = "find_container read the leaf and the list attribute from this record"
        )]
        let Attribute::List(items) = &mut attributes[container.index] else {
            unreachable!("a container's attribute is a list")
        };
        items.retain(|a| !matches!(a, Attribute::EntityRef(t) if p.removed.contains(t)));
        if items.is_empty() && container.lower >= 1 {
            refusal.emptied.push(Emptied {
                id: k,
                entity: entity.clone(),
                attribute: container.attribute.clone(),
                lower: container.lower,
            });
        }
        report.rewrites.push(Rewrite {
            id: k,
            entity,
            attribute: container.attribute.clone(),
            dropped: dropped.clone(),
        });
        rewritten.insert(k, record);
    }
    for (name, id) in &doc.graph().anchors {
        if p.removed.contains(id) {
            refusal.anchored.push((*id, name.clone()));
        }
    }
    refusal.anchored.sort();
    if !refusal.is_empty() {
        return Err(refusal);
    }

    // Kept instances that only removed instances (or the dropped members of a rewrite)
    // referenced.
    let mut targets = BTreeSet::new();
    for &r in &p.removed {
        targets.extend(forward(instance(doc, r)));
    }
    for t in targets {
        if p.removed.contains(&t) || doc.get(t).is_none() {
            continue;
        }
        let referenced = doc.referrers(t).iter().any(|r| {
            !p.removed.contains(r)
                && rewritten
                    .get(r)
                    .is_none_or(|record| forward(record).contains(&t))
        });
        if !referenced {
            report
                .left_unreferenced
                .push((t, entity_name(instance(doc, t))));
        }
    }
    for &r in &p.removed {
        *report
            .removed_by_type
            .entry(entity_name(instance(doc, r)))
            .or_insert(0) += 1;
    }
    let presentation_removed = p
        .removed
        .difference(&owned)
        .map(|&r| (r, entity_name(instance(doc, r))))
        .collect();
    Ok(RemovalPlan {
        removed: p.removed,
        rewritten,
        presentation_removed,
        report,
    })
}

/// The record of an instance the planner knows is in `doc`: a seed (checked present), an
/// instance rule 2 reached only through `doc.get`, a referrer from the document's own index, or
/// an id the document lists.
#[allow(
    clippy::expect_used,
    reason = "every caller passes an id the document holds (see above)"
)]
fn instance(doc: &Document, id: u64) -> &RawEntity {
    doc.get(id)
        .expect("the planner only names instances of the document")
}

/// A list container's or group's droppable aggregate.
#[derive(Debug, Clone)]
struct Container {
    /// The part of a complex record holding the attribute; `None` for a simple record.
    leaf: Option<usize>,
    /// The attribute's position in the record (or in the part).
    index: usize,
    /// `entity.attribute`.
    attribute: String,
    /// The aggregate's lower bound (the largest of its declaration and redeclarations).
    lower: u32,
    /// A group ([`GROUPS`]) rather than a list container.
    group: bool,
    /// The direct members of the aggregate.
    members: BTreeSet<u64>,
    /// The instances the record references other than as direct members of that aggregate.
    holds: BTreeSet<u64>,
}

struct Planner<'a> {
    doc: &'a Document,
    rebound: BTreeSet<(u64, u64)>,
    policy: PresentationPolicy,
    infrastructure: BTreeSet<u64>,
    removed: BTreeSet<u64>,
    containers: HashMap<u64, Option<Container>>,
}

impl Planner<'_> {
    /// Rule 2 to a fixpoint from the newly removed instances `work`.
    fn close(&mut self, mut work: Vec<u64>) {
        while let Some(x) = work.pop() {
            let record = instance(self.doc, x);
            let presented = self.presented(record);
            for t in forward(record) {
                if self.removed.contains(&t)
                    || self.doc.get(t).is_none()
                    || self.infrastructure.contains(&t)
                    || self.is_model(t)
                {
                    continue;
                }
                let is_presented = presented.contains(&t);
                let referrers = self.doc.referrers(t).to_vec();
                if referrers.iter().all(|&r| {
                    self.removed.contains(&r)
                        || !self.holds(r, t)
                        || (is_presented && self.depends_on_presented(r))
                }) {
                    self.removed.insert(t);
                    work.push(t);
                }
            }
        }
    }

    /// What a removed `draughting_model_item_association` presents (under
    /// `RemovePresentation`): the references of its `identified_item` and later attributes.
    fn presented(&self, record: &RawEntity) -> BTreeSet<u64> {
        let mut out = BTreeSet::new();
        if self.policy == PresentationPolicy::Refuse {
            return out;
        }
        if let RawEntity::Simple {
            name, attributes, ..
        } = record
            && TARGET.is_a(name, "draughting_model_item_association")
        {
            for a in attributes.iter().skip(4) {
                walk(a, &mut |a| {
                    if let Attribute::EntityRef(t) = a {
                        out.insert(*t);
                    }
                });
            }
        }
        out
    }

    /// Whether kept `r` depends on an item a removed association presents: a presentation or
    /// validation-property instance that is not itself a draughting model item association.
    fn depends_on_presented(&self, r: u64) -> bool {
        matches!(
            instance_family(self.doc, r),
            Family::Presentation | Family::ValidationProperty
        ) && !self
            .doc
            .get(r)
            .is_some_and(|e| is_a(e, "draughting_model_item_association"))
    }

    /// Whether `t` is a presentation `representation` (a draughting model or view: a list
    /// container), which rule 2 never removes.
    fn is_model(&self, t: u64) -> bool {
        self.doc.get(t).is_some_and(|e| is_a(e, "representation"))
            && instance_family(self.doc, t) == Family::Presentation
    }

    /// Kept instances referencing removed ones, with the removed ones they reference.
    fn blockers(&self) -> BTreeMap<u64, Vec<u64>> {
        let mut out: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for &r in &self.removed {
            for &k in self.doc.referrers(r) {
                if !self.removed.contains(&k) && !self.rebound.contains(&(k, r)) {
                    out.entry(k).or_default().push(r);
                }
            }
        }
        out
    }

    /// Whether `r` (kept) holds `t` for rule 2: always, except that under
    /// `RemovePresentation` a list container or group does not hold the members of its list.
    fn holds(&mut self, r: u64, t: u64) -> bool {
        if self.policy == PresentationPolicy::Refuse {
            return true;
        }
        match self.container(r) {
            Some(c) => c.holds.contains(&t),
            None => true,
        }
    }

    /// `k`'s droppable aggregate, if it is a presentation or validation-property list
    /// container or group.
    fn container(&mut self, k: u64) -> Option<Container> {
        if let Some(c) = self.containers.get(&k) {
            return c.clone();
        }
        let c = self.find_container(k);
        self.containers.insert(k, c.clone());
        c
    }

    fn find_container(&self, k: u64) -> Option<Container> {
        let record = self.doc.get(k)?;
        if !matches!(
            instance_family(self.doc, k),
            Family::Presentation | Family::ValidationProperty
        ) {
            return None;
        }
        let names = names(record);
        let (root, attribute, group) = GROUPS
            .iter()
            .map(|&(r, a)| (r, a, true))
            .chain(LIST_CONTAINERS.iter().map(|&(r, a)| (r, a, false)))
            .find(|(root, _, _)| names.iter().any(|n| TARGET.is_a(n, root)))?;
        // The attribute's position and type, with the redeclarations of the record's types.
        let (leaf, index, ty, redeclared) = match record {
            RawEntity::Simple { name, .. } => {
                let slots = TARGET.explicit_attributes(name)?;
                let index = slots
                    .iter()
                    .position(|s| s.entity == root && s.name == attribute)?;
                (
                    None,
                    index,
                    slots[index].ty,
                    slots[index].redeclared.clone(),
                )
            }
            RawEntity::Complex { parts, .. } => {
                let leaf = parts
                    .iter()
                    .position(|p| p.name.eq_ignore_ascii_case(root))?;
                let decl = TARGET.entity(root)?;
                let index = decl.attributes.iter().position(|a| a.name == attribute)?;
                let redeclared = names
                    .iter()
                    .filter_map(|n| TARGET.entity(n))
                    .flat_map(|e| e.redeclared)
                    .filter(|r| r.entity == root && r.attribute == attribute && !r.derived)
                    .map(|r| r.ty)
                    .collect();
                (Some(leaf), index, decl.attributes[index].ty, redeclared)
            }
        };
        let attributes = match (record, leaf) {
            (RawEntity::Simple { attributes, .. }, None) => attributes,
            (RawEntity::Complex { parts, .. }, Some(leaf)) => &parts[leaf].attributes,
            _ => return None,
        };
        let Some(Attribute::List(list)) = attributes.get(index) else {
            return None;
        };
        let lower = std::iter::once(ty)
            .chain(redeclared)
            .filter_map(|t| match t {
                Ty::Aggregate(a) => a.lower,
                _ => None,
            })
            .max()?;
        let members = list
            .iter()
            .filter_map(|a| match a {
                Attribute::EntityRef(t) => Some(*t),
                _ => None,
            })
            .collect();
        // Every reference of the record except the list's direct members (references nested
        // deeper in the list are held too).
        let mut holds = BTreeSet::new();
        let mut add = |a: &Attribute| {
            if let Attribute::EntityRef(t) = a {
                holds.insert(*t);
            }
        };
        let all: Vec<&Attribute> = match record {
            RawEntity::Simple { attributes, .. } => attributes.iter().collect(),
            RawEntity::Complex { parts, .. } => {
                parts.iter().flat_map(|p| p.attributes.iter()).collect()
            }
        };
        for a in all {
            if std::ptr::eq(a, &attributes[index]) {
                for item in list {
                    if !matches!(item, Attribute::EntityRef(_)) {
                        walk(item, &mut add);
                    }
                }
            } else {
                walk(a, &mut add);
            }
        }
        Some(Container {
            leaf,
            index,
            attribute: format!("{root}.{attribute}"),
            lower,
            group,
            members,
            holds,
        })
    }
}

fn walk(a: &Attribute, f: &mut impl FnMut(&Attribute)) {
    f(a);
    match a {
        Attribute::List(items) => items.iter().for_each(|i| walk(i, f)),
        Attribute::Typed { value, .. } => walk(value, f),
        _ => {}
    }
}

/// The instances a record references, ascending.
fn forward(e: &RawEntity) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    visit_attributes(e, &mut |a| {
        if let Attribute::EntityRef(t) = a {
            out.insert(*t);
        }
    });
    out
}

/// The entity names of a record, lower case.
fn names(e: &RawEntity) -> Vec<String> {
    match e {
        RawEntity::Simple { name, .. } => vec![name.to_ascii_lowercase()],
        RawEntity::Complex { parts, .. } => {
            parts.iter().map(|p| p.name.to_ascii_lowercase()).collect()
        }
    }
}

/// An instance's type as [`crate::express::Violation`] writes it: the entity name, or
/// `(a b c)` for a complex instance, lower case.
pub fn entity_name(e: &RawEntity) -> String {
    match e {
        RawEntity::Simple { name, .. } => name.to_ascii_lowercase(),
        RawEntity::Complex { .. } => format!("({})", names(e).join(" ")),
    }
}

fn is_a(e: &RawEntity, ancestor: &str) -> bool {
    names(e).iter().any(|n| TARGET.is_a(n, ancestor))
}

/// The family of instance `#id` under the target edition, as a PMI edit treats it:
///
/// - a simple instance: [`crate::express::Edition::instance_family`] (a `property_definition`
///   named as a CAx-IF validation property is [`Family::ValidationProperty`]);
/// - a complex instance: the family of the first [`FAMILY_RULES`] root one of its parts is;
/// - a `property_definition_representation` (a `shape_definition_representation` too) whose
///   `definition` is a validation property, and a `characterized_item_within_representation`
///   that a validation property is defined on (PMI practice §10, validation properties of
///   presentation), belong to that validation property: [`Family::ValidationProperty`];
/// - any other `characterized_item_within_representation` whose `rep` is a presentation
///   representation (a callout within a draughting model) is [`Family::Presentation`];
/// - a type the target edition does not declare: [`Family::Other`].
pub fn instance_family(doc: &Document, id: u64) -> Family {
    let Some(e) = doc.get(id) else {
        return Family::Other;
    };
    let family = match e {
        RawEntity::Simple {
            name, attributes, ..
        } => {
            let label = match attributes.first() {
                Some(Attribute::String(s)) => Some(s.as_str()),
                _ => None,
            };
            TARGET.instance_family(name, label).unwrap_or(Family::Other)
        }
        RawEntity::Complex { .. } => {
            let names = names(e);
            FAMILY_RULES
                .iter()
                .find(|(root, _)| names.iter().any(|n| TARGET.is_a(n, root)))
                .map_or(Family::Other, |&(_, f)| f)
        }
    };
    if family != Family::Other {
        return family;
    }
    let validation_property = |t: u64| {
        doc.get(t).is_some_and(|d| match d {
            RawEntity::Simple {
                name, attributes, ..
            } => {
                TARGET.is_a(name, "property_definition")
                    && matches!(attributes.first(),
                        Some(Attribute::String(s)) if VALIDATION_PROPERTY_NAMES.contains(&s.as_str()))
            }
            RawEntity::Complex { .. } => false,
        })
    };
    if let RawEntity::Simple {
        name, attributes, ..
    } = e
    {
        if TARGET.is_a(name, "property_definition_representation")
            && matches!(attributes.first(), Some(Attribute::EntityRef(t)) if validation_property(*t))
        {
            return Family::ValidationProperty;
        }
        if TARGET.is_a(name, "characterized_item_within_representation") {
            if doc.referrers(id).iter().any(|&r| validation_property(r)) {
                return Family::ValidationProperty;
            }
            if matches!(attributes.get(3), Some(Attribute::EntityRef(rep))
                if instance_family(doc, *rep) == Family::Presentation)
            {
                return Family::Presentation;
            }
        }
    }
    Family::Other
}

/// Whether `e` is the `product_definition_shape` of a `characterized_object` (a feature
/// definition: a `thread`'s or `turned_knurl`'s, thread WR13), not of a product definition.
/// `product_definition_shape` WR1 allows exactly those two kinds of definition and UR1 gives a
/// definition one shape, so such a shape is its object's alone, not product structure.
fn is_feature_shape(doc: &Document, e: &RawEntity) -> bool {
    matches!(e, RawEntity::Simple { name, attributes, .. }
        if TARGET.is_a(name, "product_definition_shape")
            && matches!(attributes.get(2), Some(Attribute::EntityRef(d))
                if doc.get(*d).is_some_and(|d| is_a(d, "characterized_object"))))
}

/// The shared infrastructure of the document (module documentation).
fn infrastructure(doc: &Document) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    let mut brep = Vec::new();
    for id in doc.ids() {
        let e = instance(doc, id);
        if INFRASTRUCTURE.iter().any(|root| is_a(e, root)) && !is_feature_shape(doc, e) {
            out.insert(id);
        }
        if is_a(e, "topological_representation_item") {
            brep.push(id);
        }
        if let RawEntity::Simple {
            name, attributes, ..
        } = e
            && TARGET.is_a(name, "property_definition_representation")
            && let [Attribute::EntityRef(d), Attribute::EntityRef(r), ..] = attributes.as_slice()
            && doc
                .get(*d)
                .is_some_and(|d| is_a(d, "product_definition_shape"))
        {
            brep.push(*r);
        }
    }
    // The B-rep: its roots and what they reach through instances of family Other.
    let mut seen = BTreeSet::new();
    while let Some(id) = brep.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(e) = doc.get(id) else { continue };
        if instance_family(doc, id) != Family::Other {
            continue;
        }
        out.insert(id);
        brep.extend(forward(e));
    }
    out
}
