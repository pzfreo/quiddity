//! Geometry provenance queries shared by the packed graph and ordinary test graphs.
//!
//! Packed queries decode only their selected attribute. Full records are scoped to
//! the current traversal item; no cache retains a second owned graph.

use std::borrow::Cow;

use crate::p21::{Attribute, RawEntity, attribute_from_step_io_ref, entity_from_step_io_ref};
use step_io::parser::Graph;

pub(crate) trait RecordGraph {
    fn ids(&self) -> Box<dyn Iterator<Item = u64> + '_>;
    fn record(&self, id: u64) -> Option<Cow<'_, RawEntity>>;
    #[allow(
        clippy::expect_used,
        reason = "callers pass ids from this immutable graph's index or already visited records"
    )]
    fn indexed_record(&self, id: u64) -> Cow<'_, RawEntity> {
        self.record(id).expect("indexed raw entity")
    }
    fn kind(&self, id: u64) -> Option<&str>;
    fn is_complex(&self, id: u64) -> bool;
    /// Whether any attribute of record `id`, at any depth and in any part, is an entity
    /// reference into `ids`. Decoding stores override this with an allocation-free scan.
    fn references_any(&self, id: u64, ids: &std::collections::HashSet<u64>) -> bool {
        fn any(a: &Attribute, ids: &std::collections::HashSet<u64>) -> bool {
            match a {
                Attribute::EntityRef(id) => ids.contains(id),
                Attribute::List(items) => items.iter().any(|a| any(a, ids)),
                Attribute::Typed { value, .. } => any(value, ids),
                _ => false,
            }
        }
        self.record(id).is_some_and(|record| match record.as_ref() {
            RawEntity::Simple { attributes, .. } => attributes.iter().any(|a| any(a, ids)),
            RawEntity::Complex { parts, .. } => parts
                .iter()
                .flat_map(|p| &p.attributes)
                .any(|a| any(a, ids)),
        })
    }
    fn attribute_at(&self, id: u64, part: Option<&str>, path: &[usize]) -> Option<Attribute>;
    fn span_start(&self, id: u64) -> Option<u64>;
}

impl RecordGraph for Graph {
    fn ids(&self) -> Box<dyn Iterator<Item = u64> + '_> {
        Box::new(self.entities.keys().copied())
    }
    fn record(&self, id: u64) -> Option<Cow<'_, RawEntity>> {
        self.entities
            .get(&id)
            .map(|e| Cow::Owned(entity_from_step_io_ref(e)))
    }
    fn kind(&self, id: u64) -> Option<&str> {
        Some(match self.entities.get(&id)? {
            step_io::parser::RawEntity::Simple { name, .. } => name,
            step_io::parser::RawEntity::Complex { .. } => "COMPLEX",
        })
    }
    fn is_complex(&self, id: u64) -> bool {
        matches!(
            self.entities.get(&id),
            Some(step_io::parser::RawEntity::Complex { .. })
        )
    }
    fn attribute_at(&self, id: u64, part: Option<&str>, path: &[usize]) -> Option<Attribute> {
        let attributes = match (self.entities.get(&id)?, part) {
            (step_io::parser::RawEntity::Simple { attributes, .. }, None) => attributes,
            (step_io::parser::RawEntity::Complex { parts, .. }, Some(name)) => {
                &parts.iter().find(|p| p.name == name)?.attributes
            }
            _ => return None,
        };
        let (&index, rest) = path.split_first()?;
        let mut value = attributes.get(index)?;
        for &index in rest {
            let step_io::parser::Attribute::List(items) = value else {
                return None;
            };
            value = items.get(index)?;
        }
        Some(attribute_from_step_io_ref(value))
    }
    fn span_start(&self, id: u64) -> Option<u64> {
        self.entities.get(&id).map(|e| e.span().start)
    }
}

impl RecordGraph for std::collections::BTreeMap<u64, RawEntity> {
    fn ids(&self) -> Box<dyn Iterator<Item = u64> + '_> {
        Box::new(self.keys().copied())
    }
    fn record(&self, id: u64) -> Option<Cow<'_, RawEntity>> {
        self.get(&id).map(Cow::Borrowed)
    }
    fn kind(&self, id: u64) -> Option<&str> {
        Some(match self.get(&id)? {
            RawEntity::Simple { name, .. } => name,
            RawEntity::Complex { .. } => "COMPLEX",
        })
    }
    fn is_complex(&self, id: u64) -> bool {
        matches!(self.get(&id), Some(RawEntity::Complex { .. }))
    }
    fn attribute_at(&self, id: u64, part: Option<&str>, path: &[usize]) -> Option<Attribute> {
        let attributes = match (self.get(&id)?, part) {
            (RawEntity::Simple { attributes, .. }, None) => attributes,
            (RawEntity::Complex { parts, .. }, Some(name)) => {
                &parts.iter().find(|p| p.name == name)?.attributes
            }
            _ => return None,
        };
        let (&index, rest) = path.split_first()?;
        let mut value = attributes.get(index)?;
        for &index in rest {
            let Attribute::List(items) = value else {
                return None;
            };
            value = items.get(index)?;
        }
        Some(value.clone())
    }
    fn span_start(&self, _id: u64) -> Option<u64> {
        None
    }
}
