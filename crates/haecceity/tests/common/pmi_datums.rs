//! Datum feature resolution checks on the reader's public semantic result.

use std::collections::BTreeSet;

use haecceity::pmi::{Anchor, Feature, FindingKind, PartId, PmiRead};

/// Datums whose feature reaches no face, identified by part and label (never datum index).
/// Groups and derived features resolve through their members/bases. A target-only datum
/// has no datum feature and is reported too; a target placement is not a face. Datums
/// omitted by the reader are included through its label-bearing Unresolved findings.
pub fn unresolved(read: &PmiRead) -> BTreeSet<(PartId, String)> {
    let mut unresolved: BTreeSet<_> = read
        .parts
        .iter()
        .enumerate()
        .flat_map(|(pi, part)| {
            part.datums.iter().filter_map(move |datum| {
                let mut pending: Vec<_> = datum.feature().into_iter().collect();
                let mut seen = BTreeSet::new();
                while let Some(feature) = pending.pop() {
                    if !seen.insert(feature) {
                        continue;
                    }
                    match &part.features[feature.0] {
                        Feature::Items(items) => {
                            if items.iter().any(|a| matches!(a, Anchor::Face(_))) {
                                return None;
                            }
                        }
                        Feature::Group { members, .. } | Feature::Derived { from: members, .. } => {
                            pending.extend(members);
                        }
                    }
                }
                Some((PartId(pi), datum.label().to_string()))
            })
        })
        .collect();
    for finding in &read.findings {
        if finding.kind == FindingKind::Unresolved
            && finding.entity == "datum"
            && let Some(part) = finding.part
            && let Some(label) = finding
                .detail
                .strip_prefix("datum ")
                .and_then(|s| s.strip_suffix(" is not read: what establishes it does not resolve"))
        {
            unresolved.insert((part, label.to_string()));
        }
    }
    unresolved
}
