//! Family panic boundaries. Shared setup, reconciliation and publication stay outside.
use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
#[cfg(panic = "unwind")]
use std::sync::Once;

use serde::{Deserialize, Serialize};

/// A family that could not be evaluated. Exactly one of `message` and `blocked_by` is set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyFailure {
    pub family: String,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
}

thread_local! {
    static GUARDED: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    pub(super) static INJECT: Cell<Option<(&'static str, &'static str)>> = const { Cell::new(None) };
    #[cfg(test)]
    static TRACE: RefCell<Option<Vec<(&'static str, &'static str)>>> = const { RefCell::new(None) };
}

/// Install one dispatcher, rather than swapping process-global hooks around each call.
/// Other threads and uncaught panics still reach the previously installed hook. Embedders
/// installing their own hook later own that policy. No locks are taken per family.
pub(super) fn install_hook() {
    #[cfg(panic = "unwind")]
    static INSTALL: Once = Once::new();
    #[cfg(panic = "unwind")]
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !GUARDED.with(|depth| depth.get() != 0) {
                previous(info);
            }
        }));
    });
}

struct Scope;
impl Drop for Scope {
    fn drop(&mut self) {
        GUARDED.with(|depth| depth.set(depth.get() - 1));
    }
}

/// Explicit record inputs, rather than shared lazily initialized geometry analyses. A
/// panicking OnceLock initializer leaves it empty: independent families may retry it.
fn inputs(family: &str, stage: &str) -> &'static [&'static str] {
    match (family, stage) {
        ("holes", _) => &["countersinks"],
        ("hole_patterns", _) => &["countersinks", "holes"],
        ("gusset_rib_patterns", _) => &["gusset_ribs"],
        ("slot_patterns", _) => &["slots"],
        ("pocket_patterns", _) => &["pockets"],
        ("oriented_slot_patterns", _) => &["oriented_slots"],
        ("oriented_slots" | "v_channels", _) => &["section_passages"],
        ("rectangular_through_notches", _) => &["plates", "channels"],
        ("freeform_surfaces", _) => &["thin_wall_bodies", "modelled_threads"],
        ("risers", _) => &["step_levels"],
        ("section_recesses", _) => &[
            "prismatic_pockets",
            "section_passages",
            "edge_open_prismatic_recesses",
            "edge_open_circular_pockets",
            "rectangular_blind_slots",
            "round_bottom_blind_slots",
            "pockets",
            "channels",
        ],
        ("turned_bodies", "suppress_od_bosses") => &["bosses"],
        // Residual families consume the complete accepted claims and refusals. Missing
        // input claims must never turn into a falsely evaluated residual surface.
        ("cylindrical_profiles" | "face_grooves", _) => CLAIM_INPUTS,
        _ => &[],
    }
}

const CLAIM_INPUTS: &[&str] = &[
    "freeform_surfaces",
    "holes",
    "countersinks",
    "gusset_ribs",
    "slots",
    "pockets",
    "section_passages",
    "oriented_slots",
    "modelled_threads",
    "helical_springs",
    "step_levels",
    "risers",
    "interior_voids",
    "channels",
    "rectangular_through_notches",
    "polygonal_bosses",
    "pads",
    "prismatic_pockets",
    "chamfers",
    "bosses",
    "angled_steps",
    "flats",
    "paired_ramp_steps",
    "oriented_chamfers",
    "circular_face_patterns",
    "oblique_through_steps",
    "polygonal_oblique_through_steps",
    "circular_blind_steps",
    "through_steps",
    "internal_bevels",
    "local_steps",
    "turned_steps",
    "grooves",
    "annular_grooves",
    "round_bottom_blind_slots",
    "semicircular_blind_slots",
    "rectangular_blind_slots",
    "double_d_bores",
    "edge_open_circular_pockets",
    "edge_open_prismatic_recesses",
    "blends",
    "repeating_radial_profiles",
    "fillets",
    "section_recesses",
    "cylindrical_profiles",
    "face_grooves",
];

pub(super) fn guarded<T: Default>(
    failures: &mut Vec<FamilyFailure>,
    family: &'static str,
    stage: &'static str,
    work: impl FnOnce() -> T,
) -> T {
    if !failures.is_empty() {
        if failures.iter().any(|f| f.family == family) {
            return T::default();
        }
        let mut blocked_by: Vec<_> = inputs(family, stage)
            .iter()
            .filter(|input| failures.iter().any(|f| f.family == **input))
            .map(|s| (*s).to_owned())
            .collect();
        if !blocked_by.is_empty() {
            blocked_by.sort();
            failures.push(FamilyFailure {
                family: family.into(),
                stage: stage.into(),
                message: None,
                blocked_by,
            });
            return T::default();
        }
    }
    GUARDED.with(|depth| depth.set(depth.get() + 1));
    let scope = Scope;
    // The only persistent mutations in discovery are retryable OnceLocks. Post-passes
    // discard their whole result and rebuild/reconcile without the failed family.
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        #[cfg(test)]
        inject(family, stage, true);
        let value = work();
        #[cfg(test)]
        inject(family, stage, false);
        value
    }));
    drop(scope);
    match result {
        Ok(value) => value,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".into());
            failures.push(FamilyFailure {
                family: family.into(),
                stage: stage.into(),
                message: Some(message),
                blocked_by: Vec::new(),
            });
            T::default()
        }
    }
}

#[cfg(test)]
fn inject(family: &'static str, stage: &'static str, before: bool) {
    if before {
        TRACE.with(|trace| {
            if let Some(trace) = trace.borrow_mut().as_mut() {
                trace.push((family, stage));
            }
        });
    }
    // Discovery fails on entry; post-passes fail after their mutations to exercise revocation.
    if before == (stage == "discover") {
        INJECT.with(|inject| {
            if inject.get() == Some((family, stage)) {
                panic!("injected {family}/{stage}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{self, discovery::tests::island_pocket};

    fn injected<T>(family: &'static str, stage: &'static str, f: impl FnOnce() -> T) -> T {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                INJECT.set(None);
            }
        }
        INJECT.set(Some((family, stage)));
        let _reset = Reset;
        f()
    }

    fn failed<'a>(failures: &'a [FamilyFailure], family: &str) -> &'a FamilyFailure {
        failures.iter().find(|f| f.family == family).unwrap()
    }

    #[test]
    fn flat_floor_completion_keeps_independent_family_failure_boundaries() {
        let part = crate::read_step_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/flat_floor_controls.step.gz"),
        )
        .unwrap();
        let floor = |x| {
            (0..part.faces.len())
                .find(|&f| {
                    let b = part.face_bounds(f);
                    (b.min[2] - 8.).abs() < 1e-7
                        && (b.max[2] - 8.).abs() < 1e-7
                        && b.min[0] < x
                        && b.max[0] > x
                })
                .unwrap()
        };
        let pocket_floor = floor(74.);
        let hole_floor = floor(12.);
        for family in ["holes", "countersinks"] {
            let found = injected(family, "discover", || features::recognise(&part));
            assert_eq!(failed(&found.failures, family).stage, "discover");
            assert!(found.holes.is_empty());
            assert!(
                found.defining["pockets"]
                    .iter()
                    .any(|faces| faces.contains(&pocket_floor))
            );
        }
        let found = injected("pockets", "claim_flat_floor", || features::recognise(&part));
        assert_eq!(failed(&found.failures, "pockets").stage, "claim_flat_floor");
        assert!(found.pockets.is_empty());
        assert!(found.defining["pockets"].is_empty());
        assert!(
            found.defining["holes"]
                .iter()
                .any(|faces| faces.contains(&hole_floor))
        );
    }

    #[test]
    fn pocket_panic_preserves_independent_families_on_annular_island() {
        // The #805 construction: box stock, a box recess and a box island (16 faces).
        let part = island_pocket();
        assert_eq!(part.faces.len(), 16);
        let baseline = features::recognise(&part);
        assert!(!baseline.pads.is_empty());
        let found = injected("pockets", "discover", || features::recognise(&part));
        let failure = failed(&found.failures, "pockets");
        assert_eq!(
            failure.message.as_deref(),
            Some("injected pockets/discover")
        );
        assert!(found.pockets.is_empty());
        assert!(found.defining["pockets"].is_empty());
        assert!(found.evidence_status["pockets"].is_empty());
        assert_eq!(found.pads, baseline.pads);
        let actual = serde_json::to_value(&found).unwrap();
        let expected = serde_json::to_value(&baseline).unwrap();
        for (family, records) in expected.as_object().unwrap() {
            if !found.failures.iter().any(|f| f.family == *family) {
                assert_eq!(actual.get(family), Some(records), "{family}");
            }
        }
        let evidence = injected("pockets", "discover", || {
            crate::evidence_view::document(&part, false)
        })
        .unwrap();
        assert_eq!(evidence.complete, Some(false));
        assert!(evidence.failures.iter().any(|f| f.family == "pockets"));
        assert!(evidence.features.iter().any(|f| f.family == "pads"));
    }

    #[test]
    fn every_discovery_boundary_revokes_its_family() {
        let part = island_pocket();
        let families = crate::evidence_view::PHYSICAL_FAMILIES
            .into_iter()
            .filter(|f| !matches!(*f, "passages" | "section_recesses"))
            .chain([
                "section_passages",
                "annular_grooves",
                "semicircular_blind_slots",
                "v_channels",
                "rectangular_through_notches",
                "hole_patterns",
                "gusset_rib_patterns",
                "slot_patterns",
                "pocket_patterns",
                "oriented_slot_patterns",
            ]);
        for family in families {
            let found = injected(family, "discover", || features::recognise(&part));
            let failure = failed(&found.failures, family);
            assert_eq!(failure.message, Some(format!("injected {family}/discover")));
            let records = serde_json::to_value(&found).unwrap();
            if let Some(records) = records.get(family) {
                assert_eq!(records, &serde_json::json!([]), "{family}");
            }
            assert!(
                found.defining.get(family).is_none_or(Vec::is_empty),
                "{family}"
            );
            assert!(
                found.evidence_status.get(family).is_none_or(Vec::is_empty),
                "{family}"
            );
        }
    }

    #[test]
    fn countersink_panic_blocks_holes_and_accepted_hole_patterns() {
        let part = crate::read_step_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/filleted_plate_with_holes.step"),
        )
        .unwrap();
        assert!(!features::recognise(&part).holes.is_empty());
        let found = injected("countersinks", "discover", || features::recognise(&part));
        for family in ["holes", "hole_patterns"] {
            let failure = failed(&found.failures, family);
            assert!(failure.blocked_by.iter().any(|f| f == "countersinks"));
            assert!(failure.message.is_none());
        }
        assert!(found.holes.is_empty());
        assert!(found.hole_patterns.is_empty());
        assert!(found.defining["holes"].is_empty());
    }

    #[test]
    fn post_pass_panic_revokes_candidates_claims_and_patterns() {
        let part = crate::read_step_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/filleted_plate_with_holes.step"),
        )
        .unwrap();
        let found = injected("holes", "claim_drill_point", || features::recognise(&part));
        assert_eq!(failed(&found.failures, "holes").stage, "claim_drill_point");
        assert!(found.holes.is_empty());
        assert!(found.hole_patterns.is_empty());
        assert!(found.defining["holes"].is_empty());
        assert!(found.evidence_status["holes"].is_empty());
        // A fresh reconciliation without holes is authoritative, not the earlier decisions.
        let rebuilt =
            features::inventory_in_with(&features::Context::new(&part), found.failures.clone())
                .unwrap()
                .unwrap()
                .accepted();
        assert_eq!(found.fillets, rebuilt.fillets);

        // This post-pass actually removes an OD boss before the injected panic. Recovery
        // must restore that independent boss and remove the failed body and all its faces.
        let disc = crate::read_step_file(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/turned_body_disc.step"),
        )
        .unwrap();
        assert!(features::recognise(&disc).bosses.is_empty());
        TRACE.with(|trace| trace.replace(Some(Vec::new())));
        let found = injected("turned_bodies", "suppress_od_bosses", || {
            features::recognise(&disc)
        });
        let trace = TRACE.with(|trace| trace.replace(None)).unwrap();
        let failed_pass = trace
            .iter()
            .position(|step| *step == ("turned_bodies", "suppress_od_bosses"))
            .unwrap();
        assert_eq!(
            trace[failed_pass + 1],
            ("countersinks", "discover"),
            "rebuild before any later post-pass can inspect partial state"
        );
        assert!(found.turned_bodies.is_empty());
        assert!(
            found
                .defining
                .get("turned_bodies")
                .is_none_or(Vec::is_empty)
        );
        assert_eq!(found.bosses.len(), 1);
        assert!(!found.defining["bosses"][0].is_empty());
        let rebuilt =
            features::inventory_in_with(&features::Context::new(&disc), found.failures.clone())
                .unwrap()
                .unwrap()
                .accepted();
        assert_eq!(found.bosses, rebuilt.bosses);
    }

    #[test]
    fn projection_and_append_panics_are_reported() {
        let part = island_pocket();
        for (family, stage) in [
            ("section_recesses", "section_recess_projection"),
            ("cylindrical_profiles", "append"),
            ("face_grooves", "append"),
            ("turned_bodies", "suppress_od_bosses"),
            ("slots", "claim_rectangular_end_walls"),
            ("polygonal_bosses", "claim_terminal_cap"),
        ] {
            let found = injected(family, stage, || features::recognise(&part));
            assert_eq!(failed(&found.failures, family).stage, stage);
            assert_eq!(found.pads, features::recognise(&part).pads);
        }
    }

    #[test]
    fn document_fields_are_optional_and_failure_order_is_deterministic() {
        let part = island_pocket();
        let baseline = crate::recognition::document(
            &crate::correspondence::recognise(&part),
            part.faces.len(),
        );
        let bytes = serde_json::to_vec(&baseline).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.get("failures").is_none());
        assert!(json.get("complete").is_none());
        assert_eq!(
            bytes,
            serde_json::to_vec(&crate::recognition::document(
                &crate::correspondence::recognise(&part),
                part.faces.len()
            ))
            .unwrap()
        );
        let run = || {
            injected("countersinks", "discover", || {
                crate::recognition::document(
                    &crate::correspondence::recognise(&part),
                    part.faces.len(),
                )
            })
        };
        let first = run();
        assert_eq!(first.complete, Some(false));
        assert_eq!(first.failures, run().failures);
        assert!(first.failures.windows(2).all(|w| w[0].family < w[1].family));
        let roundtrip: crate::recognition::RecognitionDocument =
            serde_json::from_value(serde_json::to_value(&first).unwrap()).unwrap();
        assert_eq!(roundtrip.failures, first.failures);
        let evidence = crate::evidence_view::document(&part, false).unwrap();
        let json = serde_json::to_value(evidence).unwrap();
        assert!(json.get("failures").is_none());
        assert!(json.get("complete").is_none());
    }

    #[test]
    fn shared_cli_wheel_and_server_envelope_marks_incomplete() {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/filleted_plate_with_holes.step"),
        )
        .unwrap();
        let (output, warnings) = injected("pockets", "discover", || {
            crate::serve::recognise_bytes("part.step", bytes)
        })
        .unwrap();
        assert_eq!(output.document.complete, Some(false));
        assert!(
            output
                .document
                .failures
                .iter()
                .any(|f| f.family == "pockets")
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("warning: incomplete recognition"))
        );
        assert!(output.document.records.iter().any(|r| r.family == "holes"));
    }

    #[test]
    fn once_lock_retries_and_payloads_are_retained() {
        install_hook();
        let mut failures = Vec::new();
        let cache = std::sync::OnceLock::<usize>::new();
        guarded(&mut failures, "a", "discover", || {
            cache.get_or_init(|| panic!("initializer"));
        });
        assert!(cache.get().is_none());
        assert_eq!(*cache.get_or_init(|| 42), 42);
        guarded::<()>(&mut failures, "b", "discover", || {
            panic::panic_any(String::from("owned"))
        });
        guarded::<()>(&mut failures, "c", "discover", || panic::panic_any(42));
        assert_eq!(
            failures
                .iter()
                .map(|f| f.message.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("initializer"),
                Some("owned"),
                Some("non-string panic payload")
            ]
        );
    }

    #[test]
    fn hook_suppresses_only_guarded_panics() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["features::isolation::tests::hook_probe", "--nocapture"])
            .env("QUIDDITY_HOOK_PROBE", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains("caught-family-marker"), "{stderr}");
        assert!(stderr.contains("uncaught-marker"), "{stderr}");
    }

    #[test]
    fn hook_probe() {
        if std::env::var_os("QUIDDITY_HOOK_PROBE").is_none() {
            return;
        }
        panic::set_hook(Box::new(|info| eprintln!("previous-hook: {info}")));
        install_hook();
        guarded::<()>(&mut Vec::new(), "test", "discover", || {
            panic!("caught-family-marker")
        });
        assert!(
            std::thread::spawn(|| panic!("uncaught-marker"))
                .join()
                .is_err()
        );
    }
}
