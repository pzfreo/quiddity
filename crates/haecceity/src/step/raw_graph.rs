//! Geometry provenance queries shared by the packed graph and ordinary test graphs.
//!
//! Packed queries decode only their selected attribute. Full records are scoped to
//! the current traversal item; no cache retains a second owned graph.

use std::borrow::Cow;
use step_io::parser::{Attribute, CompactGraph, Graph, RawEntity};

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
    fn attribute_at(&self, id: u64, part: Option<&str>, path: &[usize]) -> Option<Attribute>;
}

impl RecordGraph for CompactGraph {
    fn ids(&self) -> Box<dyn Iterator<Item = u64> + '_> {
        Box::new(self.ids())
    }
    fn record(&self, id: u64) -> Option<Cow<'_, RawEntity>> {
        self.entity(id).map(Cow::Owned)
    }
    fn kind(&self, id: u64) -> Option<&str> {
        self.kind(id)
    }
    fn is_complex(&self, id: u64) -> bool {
        self.is_complex(id) == Some(true)
    }
    fn attribute_at(&self, id: u64, part: Option<&str>, path: &[usize]) -> Option<Attribute> {
        self.attribute_at(id, part, path)
    }
}

impl RecordGraph for Graph {
    fn ids(&self) -> Box<dyn Iterator<Item = u64> + '_> {
        self.entities.ids()
    }
    fn record(&self, id: u64) -> Option<Cow<'_, RawEntity>> {
        self.entities.record(id)
    }
    fn kind(&self, id: u64) -> Option<&str> {
        self.entities.kind(id)
    }
    fn is_complex(&self, id: u64) -> bool {
        self.entities.is_complex(id)
    }
    fn attribute_at(&self, id: u64, part: Option<&str>, path: &[usize]) -> Option<Attribute> {
        self.entities.attribute_at(id, part, path)
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
}
