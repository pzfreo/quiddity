//! Comparison views for known value changes. Stored fingerprints are never rewritten.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::fingerprint::{FingerprintValueChange, Fingerprints, VALUE_CHANGES};

/// The value revisions of the two inputs; 0 means an unrecorded historical vintage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FingerprintRevisions {
    pub old: u32,
    pub new: u32,
}

impl FingerprintRevisions {
    fn crosses(self, change: &FingerprintValueChange) -> bool {
        self.old != self.new
            && (self.old == 0
                || self.new == 0
                || (self.old.min(self.new) < change.revision
                    && change.revision <= self.old.max(self.new)))
    }
}

/// A concrete fingerprint key omitted from both comparison views because its values may
/// have different meanings. Keys use the same paths as `Entry::changes`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExcludedKey {
    pub family: String,
    pub key: String,
}

pub(super) fn excluded_keys(old: &Fingerprints, new: &Fingerprints) -> Vec<ExcludedKey> {
    exclusions(old, new, VALUE_CHANGES)
}

fn exclusions(
    old: &Fingerprints,
    new: &Fingerprints,
    changes: &[FingerprintValueChange],
) -> Vec<ExcludedKey> {
    let revisions = FingerprintRevisions {
        old: old.fingerprint_revision,
        new: new.fingerprint_revision,
    };
    let mut out = BTreeSet::new();
    for change in changes.iter().filter(|c| revisions.crosses(c)) {
        for feature in old.features.iter().chain(&new.features) {
            if !change.families.contains(&feature.family.as_str()) {
                continue;
            }
            let keys = feature
                .traits
                .keys()
                .map(|k| format!("traits.{k}"))
                .chain(feature.sizes.keys().map(|k| format!("sizes.{k}")))
                .chain(feature.axis.map(|_| "axis".into()))
                .chain(feature.position.map(|_| "position".into()))
                .chain((!feature.neighbourhood.is_empty()).then(|| "neighbourhood".into()));
            for key in keys {
                if change.keys.iter().any(|pattern| {
                    *pattern == key
                        || pattern
                            .strip_suffix('*')
                            .is_some_and(|prefix| key.starts_with(prefix))
                }) {
                    out.insert(ExcludedKey {
                        family: feature.family.clone(),
                        key,
                    });
                }
            }
        }
    }
    out.into_iter().collect()
}

pub(super) fn comparison_view(f: &Fingerprints, excluded: &[ExcludedKey]) -> Option<Fingerprints> {
    if excluded.is_empty() {
        return None;
    }
    let mut f = f.clone();
    for feature in &mut f.features {
        for e in excluded.iter().filter(|e| e.family == feature.family) {
            if let Some(key) = e.key.strip_prefix("traits.") {
                feature.traits.remove(key);
            } else if let Some(key) = e.key.strip_prefix("sizes.") {
                feature.sizes.remove(key);
            } else {
                match e.key.as_str() {
                    // A missing position also prevents an unsound alignment anchor. A missing
                    // axis permits a point-only anchor. Faces still provide their own anchors.
                    "position" => feature.position = None,
                    "axis" => feature.axis = None,
                    "neighbourhood" => feature.neighbourhood.clear(),
                    _ => unreachable!("VALUE_CHANGES contains a non-fingerprint key"),
                }
            }
        }
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_intervals_are_symmetric_and_zero_is_unknown() {
        let change = FingerprintValueChange {
            revision: 3,
            families: &[],
            keys: &[],
            evidence: "test",
        };
        for (old, new, crosses) in [
            (0, 0, false),
            (1, 1, false),
            (0, 1, true),
            (1, 0, true),
            (1, 2, false),
            (2, 3, true),
            (3, 2, true),
            (3, 4, false),
        ] {
            assert_eq!(FingerprintRevisions { old, new }.crosses(&change), crosses);
        }
    }
}
