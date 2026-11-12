//! The feature recognisers, ported family by family from the Python implementation, and the
//! per-run machinery they share.
//!
//! Each family module has a crate-private `recognise_*` (records only, exposed solely by
//! the unsupported crate tests) and a `discover` taking the run's `Context`, returning
//! [`Occurrence`]s carrying the faces that define each record. Shared analysis — the cylinder
//! inventory, the point classifier, the part's box — lives in the context so a run computes it
//! once however many families ask.

pub mod analytic_surfaces;
pub mod angled_steps;
pub mod bevel;
pub mod blend_view;
pub mod blends;
pub mod body;
pub mod bosses;
pub mod chamfers;
pub mod channels;
pub mod circular_blind_steps;
pub mod circular_face_patterns;
mod duplicate_references;
mod edge_treatments;
mod isolation;
pub use isolation::FamilyFailure;
pub mod through_notches;
pub mod v_channels;
internal_entry! { pub mod context; }
pub mod annular_grooves;
pub mod countersinks;
pub mod cylinders;
pub mod cylindrical_channels;
pub mod cylindrical_end_surface;
pub mod cylindrical_passages;
pub mod cylindrical_pockets;
pub mod cylindrical_profiles;
pub mod cylindrical_seats;
pub mod discovery;
pub mod edge_open;
pub mod edge_open_circular;
pub mod edge_open_prismatic;
pub mod effective_surfaces;
pub mod entry_treatments;
pub mod evidence;
pub mod experimental_geometry;
pub mod face_grooves;
pub mod fillets;
pub mod flats;
pub mod freeform_surfaces;
pub mod geometry_evidence;
pub mod graph;
pub mod grooves;
pub mod gussets;
pub mod helical_springs;
pub mod hole_patterns;
pub mod holes;
pub mod interior_voids;
pub mod internal_bevels;
pub mod levels;
pub mod local_steps;
pub mod modelled_threads;
pub mod oblique_through_steps;
pub mod oriented_chamfers;
pub mod oriented_slots;
pub mod outer_profile;
pub mod pads;
pub mod paired_ramp_steps;
pub mod passage_compat;
pub mod passages;
pub(crate) mod pattern_geometry;
pub mod placed;
pub mod plane_envelope_passages;
pub mod planes;
pub mod plates;
pub mod pockets;
pub mod policy;
pub mod polygonal_bosses;
pub mod polygonal_oblique_through_steps;
pub mod prismatic_pockets;
pub mod prismatic_through_cuts;
pub mod probes;
pub mod profiled_bores;
pub mod recess_core;
pub mod recess_faces;
pub mod recess_obround;
pub mod recess_patterns;
pub mod recess_radii;
pub mod recess_records;
pub mod recess_reduce;
pub mod reconcile;
pub mod rectangular_blind_slots;
pub mod regions;
pub mod repeating_profiles;
pub mod rings;
pub mod round_bottom_slots;
pub mod section_passages;
pub mod section_recess;
pub mod section_recess_discovery;
pub mod section_recess_family;
pub mod section_recess_geometry;
pub mod sections;
pub mod semicircular_blind_slots;
pub mod sheet_metal;
pub mod slots;
pub mod solid_properties;
pub mod stacks;
pub mod support_patches;
pub mod thin_walls;
pub mod through_steps;
pub mod turned;
pub mod turned_bodies;
pub mod turned_steps;
pub mod volume_probe;
pub mod wire_seed;

internal_entry! { pub use context::Context; }
pub use evidence::{EvidenceError, Occurrence};

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::kernel::brep::Part;

/// Every ported family's records for one part, computed in one run that shares its analysis.
/// `recognise` gives the records the Python aggregate's cross-family reconciliation
/// (`_reconcile_existing`, `reconcile`) accepts, with patterns derived from accepted members
/// only; `inventory` keeps every candidate and the decisions.
#[derive(Clone, Debug, Default, Serialize)]
#[non_exhaustive]
pub struct Features {
    /// Panicking or blocked families; absent from legacy family-array serialization.
    #[serde(skip)]
    pub failures: Vec<FamilyFailure>,
    /// The completed run, independent of the thread-local retry flag after recognition.
    #[serde(skip)]
    pub run_mode: evidence::RunMode,
    /// Source faces of refused discovery candidates, keyed by their refusal family.
    #[serde(skip)]
    pub discovery_refusals: BTreeMap<&'static str, Vec<discovery::DiscoveryRefusal>>,
    /// Strict proof on each occurrence's defining AND consulted faces, kept through filtering.
    #[serde(skip)]
    pub evidence_status: BTreeMap<&'static str, Vec<evidence::EvidenceStatus>>,
    /// Complete round-wire helical springs, omitted when absent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub helical_springs: Vec<helical_springs::HelicalSpring>,
    pub fillets: Vec<fillets::Fillet>,
    /// Residual surface projection after all accepted claims (#127), omitted when absent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cylindrical_profiles: Vec<cylindrical_profiles::CylindricalProfile>,
    pub chamfers: Vec<chamfers::Chamfer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub internal_bevels: Vec<internal_bevels::InternalBevel>,
    pub bosses: Vec<bosses::BossRecord>,
    pub angled_steps: Vec<angled_steps::AngledStep>,
    pub flats: Vec<flats::Flat>,
    pub paired_ramp_steps: Vec<paired_ramp_steps::PairedRampStep>,
    pub oriented_chamfers: Vec<oriented_chamfers::OrientedChamfer>,
    pub circular_face_patterns: Vec<circular_face_patterns::CircularFacePattern>,
    pub oblique_through_steps: Vec<oblique_through_steps::ObliqueThroughStep>,
    /// Polygonal oblique steps have their own ranks; absent on legacy parts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub polygonal_oblique_through_steps:
        Vec<polygonal_oblique_through_steps::PolygonalObliqueThroughStep>,
    pub circular_blind_steps: Vec<circular_blind_steps::CircularBlindStep>,
    pub holes: Vec<holes::HoleRecord>,
    pub countersinks: Vec<countersinks::CounterSink>,
    pub hole_patterns: Vec<hole_patterns::HolePattern>,
    pub gusset_ribs: Vec<gussets::GussetRib>,
    pub gusset_rib_patterns: Vec<gussets::GussetRibPattern>,
    pub thin_wall_bodies: Vec<thin_walls::ThinWallBody>,
    pub interior_voids: Vec<interior_voids::InteriorVoid>,
    /// Refusals retain bounded-air measurements in the document, without feature claims.
    #[serde(skip)]
    pub interior_void_refusals: Vec<interior_voids::InteriorVoidRefusal>,
    pub through_steps: Vec<through_steps::ThroughStep>,
    /// Partial-run rectangular step remnants, omitted when absent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub local_steps: Vec<local_steps::LocalStep>,
    pub turned_steps: Vec<turned_steps::TurnedStep>,
    pub grooves: Vec<grooves::Groove>,
    /// Annular face recesses, omitted when absent for existing readers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub annular_grooves: Vec<annular_grooves::AnnularGroove>,
    /// Intrinsic face recesses on otherwise unclaimed faces (for #123).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub face_grooves: Vec<face_grooves::FaceGroove>,
    pub plates: Vec<plates::Plate>,
    /// Complete analytic turned solids (#123), co-owning faces with their features.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub turned_bodies: Vec<turned_bodies::TurnedBody>,
    pub round_bottom_blind_slots: Vec<round_bottom_slots::RoundBottomBlindSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub semicircular_blind_slots: Vec<semicircular_blind_slots::SemicircularBlindSlot>,
    pub rectangular_blind_slots: Vec<rectangular_blind_slots::RectangularBlindSlot>,
    pub double_d_bores: Vec<profiled_bores::DoubleDBore>,
    pub edge_open_circular_pockets: Vec<edge_open_circular::EdgeOpenCircularPocket>,
    pub edge_open_prismatic_recesses: Vec<edge_open_prismatic::EdgeOpenPrismaticRecess>,
    pub blends: Vec<blends::Blend>,
    pub sheet_metal_bodies: Vec<sheet_metal::SheetMetalBody>,
    pub slots: Vec<slots::Slot>,
    pub pockets: Vec<pockets::Pocket>,
    pub channels: Vec<channels::Channel>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub v_channels: Vec<v_channels::VChannel>,
    /// Rectangular notches through a plate's edge, omitted when absent.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rectangular_through_notches: Vec<through_notches::RectangularThroughNotch>,
    pub slot_patterns: Vec<recess_patterns::SlotPattern>,
    pub pocket_patterns: Vec<recess_patterns::PocketPattern>,
    pub repeating_radial_profiles: Vec<repeating_profiles::RepeatingRadialProfile>,
    pub freeform_surfaces: Vec<freeform_surfaces::FreeformSurface>,
    /// Measured helical threads. Omitted when absent to retain legacy output bytes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modelled_threads: Vec<modelled_threads::ModelledThread>,
    /// Refusals publish as document records, without feature fingerprints or face claims.
    #[serde(skip)]
    pub modelled_thread_refusals: Vec<modelled_threads::ModelledThreadRefusal>,
    pub polygonal_bosses: Vec<polygonal_bosses::PolygonalBoss>,
    pub polygonal_stock: Vec<polygonal_bosses::PolygonalStock>,
    /// Python's legacy inventory field (`_LegacyRecognitionResult.section_passages`): every
    /// accepted section passage (reconciliation rejects a passage a slot or an oriented slot
    /// dimensions).
    pub section_passages: Vec<passages::SectionPassage>,
    /// Nominal through-cuts with an observed intact core, possibly interrupted elsewhere.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prismatic_through_cuts: Vec<prismatic_through_cuts::PrismaticThroughCut>,
    pub prismatic_pockets: Vec<prismatic_pockets::PrismaticPocket>,
    pub oriented_slots: Vec<oriented_slots::OrientedSlot>,
    pub oriented_slot_patterns: Vec<oriented_slots::OrientedSlotPattern>,
    pub pads: Vec<pads::RaisedPad>,
    /// The aggregate's step levels (`RecognitionResult.step_levels`, `_discover_step_levels`),
    /// unproved like every family here (Python's proof: `levels::proved`).
    pub step_levels: Vec<levels::FaceLevel>,
    /// The aggregate's risers (`RecognitionResult.risers`, `_discover_risers`), less those
    /// reconciliation rejects for a thin-wall body.
    pub risers: Vec<levels::RiserEvidence>,
    /// The aggregate's section recesses (`RecognitionResult.section_recesses`, projected by
    /// `_project_result` from the same run: [`section_recess_family`]), in Python's order, each
    /// defined by its record's own defining faces. `recognise` fills them; the inventory's
    /// candidates have none. Published alongside the source families in the recognition
    /// document (maintainer decision, 2026-10-11).
    pub section_recesses: Vec<section_recess::SectionRecess>,
    /// Patterns over the published recess indices, from the same reconciled projection.
    #[serde(skip)]
    pub section_recess_patterns: Vec<section_recess::SectionRecessPattern>,
    /// Accepted source candidates that cannot publish unified geometry, in Python's order.
    /// The recognition document appends these to the section-recess family.
    #[serde(skip)]
    pub section_recess_refusals: Vec<section_recess::SectionRecessRefusal>,
    /// The section-recess projection's refusal, where `recognise` carries one (and so has no
    /// section recesses).
    #[serde(skip)]
    pub section_recess_refusal: Option<section_recess_family::SectionRecessFamilyError>,
    /// Each family's defining faces, record by record, under the family's field name. Derived
    /// families (hole, gusset rib, slot, pocket and oriented slot patterns) have none of their own: their
    /// members' faces are theirs ([`crate::correspondence`]).
    #[serde(skip)]
    pub defining: Defining,
}

/// Defining faces by family field name, then record.
pub type Defining = BTreeMap<&'static str, Vec<Vec<usize>>>;

internal_entry! {
/// Recognise every ported family on *part* with default options, as Python's aggregate does:
/// holes carry their countersinks, every candidate the reconciliation rejects is dropped (with
/// its defining faces), and patterns are found among the accepted holes, gusset ribs, slots,
/// pockets and oriented slots (`_take_inventory_once`); the section recesses are projected from
/// the same run (`_project_result`). Where Python's strict run refuses an unproved hole on a
/// part whose solids are not all valid, the run is a locally degraded one, as Python's retry
/// is (`take_inventory`). A reconciliation refusal is retained as a whole-run refusal,
/// covering the input faces without issuing feature claims. Section-recess projection
/// refusals are carried in [`Features::section_recess_refusal`].
pub fn recognise(part: &Part) -> Features {
    recognise_with(part, |_, features| features)
}
}

/// Keep the inventory's context alive for downstream geometry consumers and fingerprints.
pub(crate) fn recognise_with<T>(part: &Part, then: impl FnOnce(&Context<'_>, Features) -> T) -> T {
    let mut then = Some(then);
    take_inventory(part, |ctx, inventory| {
        then.take().unwrap()(ctx, recognise_in(ctx, inventory))
    })
    .unwrap_or_else(|error| {
        let ctx = Context::new(part);
        let features = Features {
            discovery_refusals: BTreeMap::from([(
                "reconciliation_refusals",
                vec![discovery::DiscoveryRefusal::new(error, 0..part.faces.len())],
            )]),
            ..Features::default()
        };
        then.take().unwrap()(&ctx, features)
    })
}

/// `recognise` over one completed run.
pub(crate) fn recognise_in(ctx: &Context<'_>, mut inventory: Inventory) -> Features {
    let projection = project_recesses(ctx, &mut inventory);
    let constituents = |family: &str| -> BTreeMap<Vec<usize>, Vec<usize>> {
        inventory.physical.defining[family]
            .iter()
            .cloned()
            .zip(inventory.constituent[family].iter().cloned())
            .collect()
    };
    let hole_constituents = constituents("holes");
    let pocket_constituents = constituents("pockets");
    let prismatic_pocket_constituents = constituents("prismatic_pockets");
    let boss_constituents = constituents("polygonal_bosses");
    let mut features = inventory.accepted();
    let (section_recesses, refusals, patterns, refusal) = match projection {
        Some(Ok(p)) => (p.section_recesses, p.refusals, p.patterns, None),
        Some(Err(e)) => (Vec::new(), Vec::new(), Vec::new(), Some(e)),
        None => (Vec::new(), Vec::new(), Vec::new(), None),
    };
    let defining = section_recesses
        .iter()
        .map(|r| r.evidence().defining_faces().to_vec())
        .collect();
    features.defining.insert("section_recesses", defining);
    features.evidence_status.insert(
        "section_recesses",
        section_recesses
            .iter()
            .map(|r| {
                evidence::strict_status(
                    ctx.part,
                    r.evidence().defining_faces(),
                    r.evidence().constituent_faces(),
                )
            })
            .collect(),
    );
    features.section_recesses = section_recesses;
    features.section_recess_refusals = refusals;
    features.section_recess_patterns = patterns;
    features.section_recess_refusal = refusal;
    let mut failures = std::mem::take(&mut features.failures);
    let before = failures.len();
    macro_rules! post {
        ($family:expr, $stage:literal, $work:expr $(,)?) => {
            // Never let a later pass inspect state mutated by a panicking pass.
            if failures.len() == before {
                isolation::guarded(&mut failures, $family, $stage, $work);
            }
        };
    }
    post!("holes", "claim_drill_point", || {
        // Complete published face claims only after every recognition, reconciliation
        // and projection decision. Constituent geometry must not alter family selection.
        for (record, faces) in features
            .holes
            .iter()
            .zip(features.defining.get_mut("holes").unwrap())
        {
            if let Some(constituent) = hole_constituents.get(faces) {
                holes::claim_drill_point(ctx.part, record, faces, constituent);
            }
        }
    });
    post!("pockets", "claim_floor_spheres", || {
        // Spherical floor corners were also attached to the legacy vertical
        // fillet's cylinder. Keep that wall fillet and assign its bottom cap to
        // the accepted pocket, after every selection/reconciliation decision.
        let spheres: BTreeSet<_> = features
            .pockets
            .iter()
            .flat_map(|p| &p.floor_fillets)
            .map(|f| f.face)
            .filter(|&f| {
                matches!(
                    ctx.part.faces[f].surface,
                    crate::kernel::geom::Surface::Sphere { .. }
                )
            })
            .collect();
        if spheres.is_empty() {
            return;
        }
        for faces in features.defining.get_mut("fillets").unwrap() {
            faces.retain(|f| !spheres.contains(f));
            assert!(
                !faces.is_empty(),
                "floor-only fillets must yield during reconciliation"
            );
        }
    });
    post!("slots", "claim_rectangular_end_walls", || {
        for (record, faces) in features
            .slots
            .iter()
            .zip(features.defining.get_mut("slots").unwrap())
        {
            slots::claim_rectangular_end_walls(ctx.part, record, faces);
        }
    });
    post!("polygonal_bosses", "claim_terminal_cap", || {
        for (record, faces) in features
            .polygonal_bosses
            .iter()
            .zip(features.defining.get_mut("polygonal_bosses").unwrap())
        {
            if let Some(constituent) = boss_constituents.get(faces) {
                polygonal_bosses::claim_terminal_cap(ctx.part, record, faces, constituent);
            }
        }
    });
    post!("prismatic_through_cuts", "append", || {
        prismatic_through_cuts::append(ctx, &mut features)
    });
    post!("cylindrical_profiles", "append", || {
        cylindrical_profiles::append(ctx, &mut features)
    });
    post!("turned_bodies", "suppress_od_bosses", || {
        turned_bodies::suppress_od_bosses(ctx.part, &mut features)
    });
    post!("face_grooves", "append", || {
        face_grooves::append(ctx, &mut features)
    });
    // Complete recess floors in this same post-reconciliation completion pass, after
    // append/suppression has finished: added claims must not feed recognition decisions.
    if failures.len() == before {
        // Adapt accepted records once; every family then uses the same rule and the
        // owner's failure boundary. A failed hole family must not disable pocket floors.
        let mut floors = Vec::new();
        for (index, record) in features.holes.iter().enumerate() {
            if record.bottom == holes::Bottom::Flat {
                floors.push(("holes", index, record.axis, &hole_constituents));
            }
        }
        for (index, record) in features.pockets.iter().enumerate() {
            let axis = recess_records::third_axis(
                recess_records::axis_index(record.width_axis),
                recess_records::axis_index(record.long_axis),
            );
            let mut direction = [0.; 3];
            direction[axis] = -f64::from(record.open_sign);
            floors.push(("pockets", index, direction, &pocket_constituents));
        }
        for (index, record) in features.prismatic_pockets.iter().enumerate() {
            let axis = recess_records::axis_index(record.axis.chars().next().unwrap());
            let mut direction = [0.; 3];
            direction[axis] = -f64::from(record.open_sign);
            floors.push((
                "prismatic_pockets",
                index,
                direction,
                &prismatic_pocket_constituents,
            ));
        }
        // Individual manufacturing claims are exclusive. The first new owner wins,
        // in order: holes, pockets, then prismatic pockets, even for identical walls.
        // Evidence and body classifications may coexist; section recesses stay intact.
        let occupied = floor_claims_occupied(&features.defining);
        let mut floor_owners = BTreeSet::new();
        for (family, index, direction, constituents) in floors {
            post!(family, "claim_flat_floor", || {
                let faces = &mut features.defining.get_mut(family).unwrap()[index];
                if let Some(constituent) = constituents.get(faces) {
                    holes::claim_flat_floor(
                        ctx.part,
                        direction,
                        faces,
                        constituent,
                        &occupied,
                        &mut floor_owners,
                    );
                }
            });
        }
    }
    if failures.len() != before {
        // A post-pass may already have mutated records or claims. Discard everything,
        // then rediscover and reconcile with the failed/blocked families revoked.
        let inventory = inventory_in_with(ctx, failures)
            .expect("reconciliation remains outside the panic boundary")
            .expect("a completed run retains its degradation mode");
        return recognise_in(ctx, inventory);
    }
    failures.sort_by(|a, b| (&a.family, &a.stage).cmp(&(&b.family, &b.stage)));
    features.failures = failures;
    duplicate_references::mark(ctx.part, &mut features);
    features
}

/// Reserve individual manufacturing claims before floor completion. Levels and risers
/// are evidence families, not individual manufacturing features (also in draftwright).
/// They and the existing whole-body classifications are the only co-owner exceptions.
pub(crate) fn floor_claims_occupied(defining: &Defining) -> BTreeSet<usize> {
    defining
        .iter()
        .filter_map(|(&family, claims)| {
            (!matches!(
                family,
                "step_levels"
                    | "risers"
                    | "thin_wall_bodies"
                    | "sheet_metal_bodies"
                    | "plates"
                    | "polygonal_stock"
                    | "turned_bodies"
            ))
            .then_some(claims)
        })
        .flatten()
        .flatten()
        .copied()
        .collect()
}

/// Both consumers of the recess projection share the same boundary and failure ledger.
pub(crate) fn project_recesses(
    ctx: &Context<'_>,
    inventory: &mut Inventory,
) -> Option<
    Result<
        section_recess_family::SectionRecessProjection,
        section_recess_family::SectionRecessFamilyError,
    >,
> {
    let mut failures = std::mem::take(&mut inventory.physical.failures);
    let projection = isolation::guarded(
        &mut failures,
        "section_recesses",
        "section_recess_projection",
        || {
            Some(section_recess_family::section_recess_projection_in(
                ctx, inventory,
            ))
        },
    );
    match &projection {
        Some(Ok(p)) if !p.source_refusals.is_empty() => {
            inventory
                .physical
                .discovery_refusals
                .insert("section_recess_source_refusals", p.source_refusals.clone());
        }
        Some(Err(error)) => failures.push(FamilyFailure {
            family: "section_recesses".into(),
            stage: "section_recess_projection".into(),
            message: Some(error.to_string()),
            blocked_by: Vec::new(),
        }),
        _ => {}
    }
    inventory.physical.failures = failures;
    projection
}

/// One run's physical candidates and the aggregate's decisions on them (Python's
/// `InventoryProduct.physical`, its evidence and its `reconciliation`).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Inventory {
    /// Every family's candidates before reconciliation, each with its defining faces. The
    /// derived pattern families are empty: Python derives them from accepted members only.
    pub physical: Features,
    /// What the rules read beyond the records.
    pub evidence: reconcile::Evidence,
    /// Every decision the rules make, in rule order (no default acceptances).
    pub dispositions: Vec<reconcile::Disposition>,
    /// Each family's constituent faces, candidate by candidate under the family's field name
    /// (Python's `EvidenceIndex.constituent_of`): its defining faces and the further faces its
    /// proof consulted, sorted. The section-recess projection reads them
    /// ([`section_recess_family`]).
    pub constituent: Defining,
}

impl Inventory {
    internal_entry! {
        /// The accepted inventory (`accepted_set` on every family) with its patterns derived
        /// (`_derive_patterns`): each rejected candidate's record and defining faces dropped, in
        /// order. Section recesses are not projected here (`recognise` does, from the run).
        pub fn accepted(self) -> Features {
            let mut f = self.physical;
            let mut rejected: BTreeMap<reconcile::Family, BTreeSet<usize>> = BTreeMap::new();
            for d in &self.dispositions {
                if d.outcome == reconcile::Outcome::Rejected {
                    rejected
                        .entry(d.candidate.family)
                        .or_default()
                        .insert(d.candidate.index);
                }
            }
            for (family, indices) in &rejected {
                // Risers have no Family::field; use the same slot as their special arm below.
                let field = family.field().unwrap_or("risers");
                let statuses = f
                    .evidence_status
                    .get_mut(field)
                    .expect("every candidate has a status");
                drop_rejected(statuses, indices);
                use reconcile::Family as F;
                match family {
                    F::AngledSteps => drop_rejected(&mut f.angled_steps, indices),
                    F::Blends => drop_rejected(&mut f.blends, indices),
                    F::Bosses => drop_rejected(&mut f.bosses, indices),
                    F::Chamfers => drop_rejected(&mut f.chamfers, indices),
                    F::CircularBlindSteps => drop_rejected(&mut f.circular_blind_steps, indices),
                    F::DoubleDBores => drop_rejected(&mut f.double_d_bores, indices),
                    F::EdgeOpenCircularPockets => {
                        drop_rejected(&mut f.edge_open_circular_pockets, indices)
                    }
                    F::Fillets => drop_rejected(&mut f.fillets, indices),
                    F::Grooves => drop_rejected(&mut f.grooves, indices),
                    F::Holes => drop_rejected(&mut f.holes, indices),
                    F::OrientedSlots => drop_rejected(&mut f.oriented_slots, indices),
                    F::Passages => drop_rejected(&mut f.section_passages, indices),
                    F::Plates => drop_rejected(&mut f.plates, indices),
                    F::Pockets => drop_rejected(&mut f.pockets, indices),
                    F::PrismaticPockets => drop_rejected(&mut f.prismatic_pockets, indices),
                    F::RectangularBlindSlots => drop_rejected(&mut f.rectangular_blind_slots, indices),
                    F::Risers => {
                        // Not a reconciliation field (`Family::field`), so dropped here.
                        drop_rejected(&mut f.risers, indices);
                        let defining = f.defining.get_mut("risers").expect("risers are carried");
                        drop_rejected(defining, indices);
                        continue;
                    }
                    F::Slots => drop_rejected(&mut f.slots, indices),
                    F::ThinWallBodies => drop_rejected(&mut f.thin_wall_bodies, indices),
                    F::TurnedSteps => drop_rejected(&mut f.turned_steps, indices),
                }
                let field = family.field().expect("a carried family has a field");
                let defining = f
                    .defining
                    .get_mut(field)
                    .expect("every family has defining faces");
                drop_rejected(defining, indices);
            }
            // A notch is cut through its plate: none survives the plate's rejection.
            if let Some(plates) = rejected.get(&reconcile::Family::Plates)
                && let Some(defining) = f.defining.get_mut("rectangular_through_notches")
            {
                let gone: BTreeSet<usize> = f
                    .rectangular_through_notches
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| plates.contains(&n.plate))
                    .map(|(i, _)| i)
                    .collect();
                drop_rejected(&mut f.rectangular_through_notches, &gone);
                drop_rejected(defining, &gone);
                if let Some(statuses) = f.evidence_status.get_mut("rectangular_through_notches") {
                    drop_rejected(statuses, &gone);
                }
            }
            f.hole_patterns = isolation::guarded(&mut f.failures, "hole_patterns", "discover", || hole_patterns::recognise_hole_patterns(&f.holes));
            f.gusset_rib_patterns = isolation::guarded(&mut f.failures, "gusset_rib_patterns", "discover", || gussets::recognise_gusset_rib_patterns(&f.gusset_ribs));
            f.slot_patterns = isolation::guarded(&mut f.failures, "slot_patterns", "discover", || recess_patterns::recognise_slot_patterns(&f.slots));
            f.pocket_patterns = isolation::guarded(&mut f.failures, "pocket_patterns", "discover", || recess_patterns::recognise_pocket_patterns(&f.pockets));
            f.oriented_slot_patterns =
                isolation::guarded(&mut f.failures, "oriented_slot_patterns", "discover", || oriented_slots::recognise_oriented_slot_patterns(&f.oriented_slots));
            f.failures.sort_by(|a, b| (&a.family, &a.stage).cmp(&(&b.family, &b.stage)));
            f
        }
    }
}

/// *items* without those at *indices*, in order.
fn drop_rejected<T>(items: &mut Vec<T>, indices: &BTreeSet<usize>) {
    let mut at = 0;
    items.retain(|_| {
        let keep = !indices.contains(&at);
        at += 1;
        keep
    });
}

internal_entry! {
/// Every ported family's candidates on *part* with default options and the reconciliation's
/// decisions on them, or the reconciliation's refusal; locally degraded where Python's
/// aggregate retries so (`take_inventory`).
pub fn inventory(part: &Part) -> Result<Inventory, reconcile::ReconcileError> {
    take_inventory(part, |_, inventory| inventory)
}
}

/// Python's `_take_inventory` with its bounded fallback: one strict run, or, where that run's
/// hole evidence refuses for want of one valid solid on a part whose solids are not all valid
/// ([`holes::strict_refusal_retries`]), one run again from scratch with `local_degradation`
/// set ([`evidence::locally_degraded`]). *then* reads the completed run (its context and
/// inventory) inside the same mode, so a projection of it answers as that run's graph does.
///
/// The port's strict run proves no other family's evidence, so this hole refusal is the only
/// strict refusal it retries on, as Python's aggregate retries on no other.
pub(crate) fn take_inventory<'p, T>(
    part: &'p Part,
    then: impl FnOnce(&Context<'p>, Inventory) -> T,
) -> Result<T, reconcile::ReconcileError> {
    if !evidence::local_degradation() {
        let ctx = Context::new(part);
        if let Some(inventory) = inventory_in(&ctx)? {
            return Ok(then(&ctx, inventory));
        }
    }
    evidence::locally_degraded(|| {
        let ctx = Context::new(part);
        let inventory = inventory_in(&ctx)?.expect("a locally degraded run does not retry");
        Ok(then(&ctx, inventory))
    })
}

/// *found* less, in a locally degraded run, the occurrences *proved* fails: the records such a
/// run skips rather than refuses. A strict run keeps them all (it publishes unproved).
fn degraded_skip<R>(
    found: Vec<Occurrence<R>>,
    proved: impl Fn(&Occurrence<R>) -> bool,
) -> Vec<Occurrence<R>> {
    if !evidence::local_degradation() {
        return found;
    }
    found.into_iter().filter(|o| proved(o)).collect()
}

/// [`degraded_skip`] on each occurrence's defining and consulted faces together
/// ([`evidence::locally_proved`]): the check most of Python's families make on a degraded graph.
fn skip_unproved<R>(part: &Part, found: Vec<Occurrence<R>>) -> Vec<Occurrence<R>> {
    degraded_skip(found, |o| evidence::locally_proved(part, o))
}

/// One run of `inventory` on a context the caller keeps, so a projection of it reads the same
/// analysis, in the current mode ([`evidence::local_degradation`]). `None` where a strict run
/// would retry ([`take_inventory`]): as Python's raises, the run stops at its holes. A locally
/// degraded run skips the records Python's families skip in that mode where the port has the
/// family's check: holes, pockets, slots, channels, fillets, prismatic pockets and edge-open
/// prismatic and circular recesses (on defining and consulted faces, [`skip_unproved`]), step
/// levels and risers (on their defining faces, as `levels::proved`), plates (in
/// [`plates::discover`]) and rectangular pads (in [`pads::discover`]).
pub(crate) fn inventory_in(
    ctx: &Context<'_>,
) -> Result<Option<Inventory>, reconcile::ReconcileError> {
    inventory_in_with(ctx, Vec::new())
}

fn inventory_in_with(
    ctx: &Context<'_>,
    mut failures: Vec<FamilyFailure>,
) -> Result<Option<Inventory>, reconcile::ReconcileError> {
    isolation::install_hook();
    macro_rules! discover {
        ($family:literal, $value:expr) => {
            isolation::guarded(&mut failures, $family, "discover", || $value)
        };
    }
    let part = ctx.part;
    let seats = discover!("countersinks", countersinks::discover(ctx));
    let (found_holes, retry) = discover!("holes", {
        let found = holes::discover(ctx, &seats);
        let retry =
            !evidence::local_degradation() && holes::strict_refusal_retries(part, &seats, &found);
        (skip_unproved(part, found), retry)
    });
    if retry {
        return Ok(None);
    }
    let mut defining = Kept {
        part,
        claimed: Defining::new(),
        defining: Defining::new(),
        constituent: Defining::new(),
        evidence_status: BTreeMap::new(),
    };
    let found_springs = discover!("helical_springs", helical_springs::discover(ctx));
    let helical_springs = if found_springs.is_empty() {
        Vec::new()
    } else {
        kept(&mut defining, "helical_springs", found_springs)
    };
    let holes = kept(&mut defining, "holes", found_holes);
    let countersinks = kept(&mut defining, "countersinks", seats);
    let gusset_ribs = kept(
        &mut defining,
        "gusset_ribs",
        discover!("gusset_ribs", gussets::discover(ctx)),
    );
    let slots = kept(
        &mut defining,
        "slots",
        discover!("slots", skip_unproved(part, slots::discover(ctx))),
    );
    let pocket_occurrences = discover!("pockets", skip_unproved(part, pockets::discover(ctx)));
    let thin_wall_bodies = kept_shared(
        &mut defining,
        "thin_wall_bodies",
        discover!("thin_wall_bodies", thin_walls::discover(ctx)),
    );
    let mut discovery_refusals = BTreeMap::new();
    let (passages, compatibility) =
        isolation::guarded(&mut failures, "section_passages", "discover", || {
            Some((|| {
                let passages = match passages::discover(ctx) {
                    Ok(found) => found,
                    Err(error) => {
                        let faces = passages::proposal_faces(ctx);
                        discovery_refusals.insert(
                            "section_passage_refusals",
                            vec![discovery::DiscoveryRefusal::new(error, faces)],
                        );
                        Vec::new()
                    }
                };
                // Reconciliation must read the same completed passages; a refused family has no
                // compatibility views and cannot suppress a pocket or slot.
                let compatibility = if discovery_refusals.contains_key("section_passage_refusals") {
                    Vec::new()
                } else {
                    passages::compatibility_views(ctx)?
                };
                Ok::<_, reconcile::ReconcileError>((passages, compatibility))
            })())
        })
        .unwrap_or(Ok(Default::default()))?;
    let evidence = reconcile::Evidence::with_passage_views(ctx, &pocket_occurrences, compatibility);
    let pockets = kept(&mut defining, "pockets", pocket_occurrences);
    let oriented_slots = kept(
        &mut defining,
        "oriented_slots",
        discover!(
            "oriented_slots",
            match oriented_slots::discover(ctx, &passages) {
                Ok(found) => found,
                Err(error) => {
                    discovery_refusals.insert(
                        "oriented_slot_refusals",
                        vec![discovery::DiscoveryRefusal::new(
                            error,
                            passages
                                .iter()
                                .flat_map(|p| p.defining.iter().chain(&p.context))
                                .copied(),
                        )],
                    );
                    Vec::new()
                }
            }
        ),
    );
    // A panic after an ordinary refusal must also revoke that family's refusal faces.
    for (family, refusals) in [
        ("section_passages", "section_passage_refusals"),
        ("oriented_slots", "oriented_slot_refusals"),
    ] {
        if failures.iter().any(|failure| failure.family == family) {
            discovery_refusals.remove(refusals);
        }
    }
    let empty_threads = modelled_threads::ThreadAnalysis::default();
    let thread_analysis =
        discover!("modelled_threads", Some(ctx.modelled_threads())).unwrap_or(&empty_threads);
    let claimed: BTreeSet<_> = thread_analysis
        .threads
        .iter()
        .flat_map(|o| o.defining.iter().copied())
        .collect();
    let modelled_threads = if thread_analysis.threads.is_empty() {
        Vec::new()
    } else {
        kept(
            &mut defining,
            "modelled_threads",
            thread_analysis.threads.clone(),
        )
    };
    let freeform_surfaces = kept_shared(
        &mut defining,
        "freeform_surfaces",
        discover!(
            "freeform_surfaces",
            freeform_surfaces::discover(ctx, &thin_wall_bodies)
                .into_iter()
                .filter(|o| !claimed.contains(&o.record.face))
                .collect()
        ),
    );
    // A degraded run's risers carry the step levels it kept, as Python's carry that run's
    // completed levels (`levels::proved` skips only in a degraded run).
    let step_level_occurrences = discover!(
        "step_levels",
        degraded_skip(levels::discover_step_levels(ctx), |o| {
            evidence::locally_valid_solid(part, &o.defining).is_some()
        })
    );
    let found_risers = discover!(
        "risers",
        degraded_skip(levels::discover_risers(ctx, &step_level_occurrences), |o| {
            evidence::locally_valid_solid(part, &o.defining).is_some()
        })
    );
    let risers = kept(&mut defining, "risers", found_risers);
    let step_levels = kept(&mut defining, "step_levels", step_level_occurrences);
    let void_analysis = discover!("interior_voids", interior_voids::analyse(ctx));
    let plate_occurrences = discover!("plates", plates::discover(ctx, &Default::default()));
    let channel_occurrences = discover!("channels", skip_unproved(part, channels::discover(ctx)));
    let notches = discover!(
        "rectangular_through_notches",
        through_notches::discover(ctx, &plate_occurrences, &channel_occurrences)
    );
    let boss_analysis = discover!(
        "polygonal_bosses",
        polygonal_bosses::analyse(ctx, &Default::default())
    );
    if !boss_analysis.refusals.is_empty() {
        discovery_refusals.insert("polygonal_boss_refusals", boss_analysis.refusals);
    }
    let pad_analysis = discover!("pads", pads::analyse(ctx, &Default::default()));
    let prismatic_analysis = discover!("prismatic_pockets", prismatic_pockets::analyse(ctx));
    if !pad_analysis.refusals.is_empty() {
        discovery_refusals.insert("pad_refusals", pad_analysis.refusals);
    }
    if !prismatic_analysis.refusals.is_empty() {
        discovery_refusals.insert("prismatic_pocket_refusals", prismatic_analysis.refusals);
    }
    let floor_refusals = discover!("pockets", recess_core::floor_blend_refusals(ctx));
    if !floor_refusals.is_empty() {
        discovery_refusals.insert("pocket_refusals", floor_refusals);
    }
    let mut physical = Features {
        helical_springs,
        discovery_refusals,
        run_mode: if evidence::local_degradation() {
            evidence::RunMode::LocallyDegraded
        } else {
            evidence::RunMode::Strict
        },
        cylindrical_profiles: Vec::new(),
        chamfers: kept(
            &mut defining,
            "chamfers",
            discover!("chamfers", ctx.reported_chamfers().to_vec()),
        ),
        bosses: kept(
            &mut defining,
            "bosses",
            discover!("bosses", bosses::discover(ctx)),
        ),
        angled_steps: kept(
            &mut defining,
            "angled_steps",
            discover!("angled_steps", angled_steps::discover(ctx)),
        ),
        flats: kept(
            &mut defining,
            "flats",
            discover!("flats", flats::discover(ctx)),
        ),
        paired_ramp_steps: kept(
            &mut defining,
            "paired_ramp_steps",
            discover!("paired_ramp_steps", paired_ramp_steps::discover(ctx)),
        ),
        oriented_chamfers: kept(
            &mut defining,
            "oriented_chamfers",
            discover!(
                "oriented_chamfers",
                ctx.reported_oriented_chamfers().to_vec()
            ),
        ),
        circular_face_patterns: kept(
            &mut defining,
            "circular_face_patterns",
            discover!(
                "circular_face_patterns",
                circular_face_patterns::discover(ctx)
            ),
        ),
        oblique_through_steps: kept(
            &mut defining,
            "oblique_through_steps",
            discover!(
                "oblique_through_steps",
                oblique_through_steps::discover(ctx)
            ),
        ),
        polygonal_oblique_through_steps: {
            let found = discover!(
                "polygonal_oblique_through_steps",
                polygonal_oblique_through_steps::discover(ctx)
            );
            if found.is_empty() {
                Vec::new()
            } else {
                kept(&mut defining, "polygonal_oblique_through_steps", found)
            }
        },
        circular_blind_steps: kept(
            &mut defining,
            "circular_blind_steps",
            discover!("circular_blind_steps", circular_blind_steps::discover(ctx)),
        ),
        hole_patterns: Vec::new(),
        holes,
        gusset_rib_patterns: Vec::new(),
        gusset_ribs,
        countersinks,
        thin_wall_bodies,
        interior_voids: kept(&mut defining, "interior_voids", void_analysis.accepted),
        interior_void_refusals: void_analysis.refusals,
        through_steps: kept(
            &mut defining,
            "through_steps",
            discover!("through_steps", through_steps::discover(ctx)),
        ),
        internal_bevels: {
            let found = discover!("internal_bevels", internal_bevels::discover(ctx));
            if found.is_empty() {
                Vec::new()
            } else {
                kept(&mut defining, "internal_bevels", found)
            }
        },
        local_steps: {
            let found = discover!("local_steps", local_steps::discover(ctx));
            if found.is_empty() {
                Vec::new()
            } else {
                kept(&mut defining, "local_steps", found)
            }
        },
        turned_steps: kept(
            &mut defining,
            "turned_steps",
            discover!(
                "turned_steps",
                turned_steps::sorted_occurrences(turned_steps::discover(ctx))
            ),
        ),
        grooves: kept(
            &mut defining,
            "grooves",
            discover!("grooves", grooves::discover(ctx)),
        ),
        annular_grooves: {
            let found = discover!("annular_grooves", annular_grooves::discover(ctx));
            if found.is_empty() {
                Vec::new()
            } else {
                kept(&mut defining, "annular_grooves", found)
            }
        },
        face_grooves: Vec::new(),
        plates: kept_shared(&mut defining, "plates", plate_occurrences),
        turned_bodies: {
            let found = discover!("turned_bodies", turned_bodies::discover(ctx));
            if found.is_empty() {
                Vec::new()
            } else {
                kept_shared(&mut defining, "turned_bodies", found)
            }
        },
        round_bottom_blind_slots: kept(
            &mut defining,
            "round_bottom_blind_slots",
            discover!(
                "round_bottom_blind_slots",
                round_bottom_slots::discover(ctx)
            ),
        ),
        semicircular_blind_slots: {
            let found = discover!(
                "semicircular_blind_slots",
                semicircular_blind_slots::discover(ctx)
            );
            if found.is_empty() {
                Vec::new()
            } else {
                kept(&mut defining, "semicircular_blind_slots", found)
            }
        },
        rectangular_blind_slots: kept(
            &mut defining,
            "rectangular_blind_slots",
            discover!(
                "rectangular_blind_slots",
                rectangular_blind_slots::discover(ctx)
            ),
        ),
        double_d_bores: kept(
            &mut defining,
            "double_d_bores",
            discover!("double_d_bores", profiled_bores::discover(ctx)),
        ),
        edge_open_circular_pockets: kept(
            &mut defining,
            "edge_open_circular_pockets",
            discover!(
                "edge_open_circular_pockets",
                skip_unproved(part, edge_open_circular::discover(ctx))
            ),
        ),
        edge_open_prismatic_recesses: kept(
            &mut defining,
            "edge_open_prismatic_recesses",
            discover!(
                "edge_open_prismatic_recesses",
                skip_unproved(part, edge_open_prismatic::discover(ctx))
            ),
        ),
        blends: kept_shared(
            &mut defining,
            "blends",
            discover!("blends", blends::discover(ctx)),
        ),
        sheet_metal_bodies: kept_shared(
            &mut defining,
            "sheet_metal_bodies",
            discover!("sheet_metal_bodies", sheet_metal::discover(ctx)),
        ),
        slot_patterns: Vec::new(),
        slots,
        pocket_patterns: Vec::new(),
        pockets,
        v_channels: {
            let found = discover!("v_channels", v_channels::discover(ctx, &passages));
            if found.is_empty() {
                Vec::new()
            } else {
                kept(&mut defining, "v_channels", found)
            }
        },
        channels: kept(&mut defining, "channels", channel_occurrences),
        rectangular_through_notches: if notches.is_empty() {
            Vec::new()
        } else {
            kept(&mut defining, "rectangular_through_notches", notches)
        },
        repeating_radial_profiles: kept(
            &mut defining,
            "repeating_radial_profiles",
            discover!(
                "repeating_radial_profiles",
                repeating_profiles::discover(ctx)
            ),
        ),
        freeform_surfaces,
        modelled_threads,
        modelled_thread_refusals: thread_analysis.refusals.clone(),
        polygonal_bosses: kept(&mut defining, "polygonal_bosses", boss_analysis.accepted),
        polygonal_stock: kept_shared(
            &mut defining,
            "polygonal_stock",
            discover!(
                "polygonal_stock",
                polygonal_bosses::discover_stock(ctx, &Default::default())
            ),
        ),
        section_passages: kept(&mut defining, "section_passages", passages),
        prismatic_through_cuts: Vec::new(),
        prismatic_pockets: kept(
            &mut defining,
            "prismatic_pockets",
            skip_unproved(part, prismatic_analysis.accepted),
        ),
        oriented_slot_patterns: Vec::new(),
        oriented_slots,
        pads: kept(&mut defining, "pads", pad_analysis.accepted),
        step_levels,
        risers,
        section_recesses: Vec::new(),
        section_recess_refusals: Vec::new(),
        section_recess_patterns: Vec::new(),
        section_recess_refusal: None,
        fillets: kept_shared(
            &mut defining,
            "fillets",
            discover!(
                "fillets",
                skip_unproved(part, ctx.reported_fillets().to_vec())
            ),
        ),
        failures,
        defining: defining.defining,
        evidence_status: defining.evidence_status,
    };
    // The thin-wall rule reads the risers this inventory carries, in its order.
    let mut evidence = evidence;
    evidence.risers = physical.defining["risers"].clone();
    let mut dispositions = reconcile::reconcile(&physical, &evidence)?;
    // Specific accepted features reserve their defining faces before edge rounds.
    // Rejected proposals cannot suppress a fillet. Body classifications and generic
    // surface fallbacks use kept_shared; derived patterns inherit their members' claims.
    let rejected: BTreeSet<_> = dispositions
        .iter()
        .filter(|d| d.outcome == reconcile::Outcome::Rejected)
        .map(|d| {
            (
                d.candidate.family.field().unwrap_or("risers"),
                d.candidate.index,
            )
        })
        .collect();
    let claimed: BTreeSet<_> = defining
        .claimed
        .iter()
        .flat_map(|(family, records)| {
            records
                .iter()
                .enumerate()
                .filter(|(i, _)| !rejected.contains(&(*family, *i)))
                .flat_map(|(_, faces)| faces.iter().copied())
        })
        .collect();
    let gone: BTreeSet<_> = physical.defining["fillets"]
        .iter()
        .enumerate()
        // Existing precedence decisions already exclude these proposals and keep their
        // reconciliation evidence. Only otherwise-surviving fillets need filtering.
        .filter(|(i, faces)| {
            !rejected.contains(&("fillets", *i)) && faces.iter().any(|f| claimed.contains(f))
        })
        .map(|(i, _)| i)
        .collect();
    if !gone.is_empty() {
        drop_rejected(&mut physical.fillets, &gone);
        drop_rejected(physical.defining.get_mut("fillets").unwrap(), &gone);
        drop_rejected(physical.evidence_status.get_mut("fillets").unwrap(), &gone);
        drop_rejected(defining.constituent.get_mut("fillets").unwrap(), &gone);
        // A suppressed fillet cannot defeat a blend; indices follow the filtered records.
        dispositions = reconcile::reconcile(&physical, &evidence)?;
    }
    let mut inventory = Inventory {
        physical,
        evidence,
        dispositions,
        constituent: defining.constituent,
    };
    // For #127, new inside rounds also respect the complete defining claims of the
    // section-recess projection. Those claims include cavity treatments that the seed
    // pocket only consulted. Existing convex records retain their established ownership.
    if inventory
        .physical
        .fillets
        .iter()
        .any(|f| f.side == "concave")
        && let Some(Ok(projection)) = project_recesses(ctx, &mut inventory)
    {
        let reserved: BTreeSet<_> = projection
            .section_recesses
            .iter()
            .flat_map(|r| r.evidence().defining_faces().iter().copied())
            .collect();
        let gone: BTreeSet<_> = inventory
            .physical
            .fillets
            .iter()
            .enumerate()
            .filter(|(i, f)| {
                f.side == "concave"
                    && inventory.physical.defining["fillets"][*i]
                        .iter()
                        .any(|f| reserved.contains(f))
            })
            .map(|(i, _)| i)
            .collect();
        if !gone.is_empty() {
            drop_rejected(&mut inventory.physical.fillets, &gone);
            drop_rejected(
                inventory.physical.defining.get_mut("fillets").unwrap(),
                &gone,
            );
            drop_rejected(
                inventory
                    .physical
                    .evidence_status
                    .get_mut("fillets")
                    .unwrap(),
                &gone,
            );
            drop_rejected(inventory.constituent.get_mut("fillets").unwrap(), &gone);
            inventory.dispositions =
                reconcile::reconcile(&inventory.physical, &inventory.evidence)?;
        }
    }
    Ok(Some(inventory))
}

/// Defining and constituent faces by family field name, then record.
struct Kept<'p> {
    part: &'p Part,
    claimed: Defining,
    defining: Defining,
    constituent: Defining,
    evidence_status: BTreeMap<&'static str, Vec<evidence::EvidenceStatus>>,
}

/// The records of a family's occurrences, their defining and constituent faces kept under the
/// family's name.
fn kept<R>(kept: &mut Kept<'_>, family: &'static str, found: Vec<Occurrence<R>>) -> Vec<R> {
    let found = without_spring_claims(kept, family, found);
    kept.claimed
        .insert(family, found.iter().map(|o| o.defining.clone()).collect());
    kept_shared(kept, family, found)
}

/// A proved whole spring takes precedence over descriptions of its individual wire
/// patches. Filter before storing records/evidence, so patterns inherit accepted members.
fn without_spring_claims<R>(
    kept: &Kept<'_>,
    family: &str,
    found: Vec<Occurrence<R>>,
) -> Vec<Occurrence<R>> {
    if family == "helical_springs" {
        return found;
    }
    let Some(springs) = kept.defining.get("helical_springs") else {
        return found;
    };
    found
        .into_iter()
        .filter(|o| {
            !springs
                .iter()
                .any(|faces| o.defining.iter().any(|f| faces.binary_search(f).is_ok()))
        })
        .collect()
}

/// Whole-body classifications and generic surface fallbacks retain evidence without
/// reserving their faces against a specific feature discovered later.
fn kept_shared<R>(kept: &mut Kept<'_>, family: &'static str, found: Vec<Occurrence<R>>) -> Vec<R> {
    let found = without_spring_claims(kept, family, found);
    kept.evidence_status.insert(
        family,
        found
            .iter()
            .map(|o| evidence::strict_status(kept.part, &o.defining, &o.context))
            .collect(),
    );
    kept.defining
        .insert(family, found.iter().map(|o| o.defining.clone()).collect());
    kept.constituent.insert(
        family,
        found
            .iter()
            .map(|o| {
                let faces: BTreeSet<usize> = o.defining.iter().chain(&o.context).copied().collect();
                faces.into_iter().collect()
            })
            .collect(),
    );
    records(found)
}

internal_entry! {
/// The records of a family's occurrences, in order.
pub fn records<R>(found: Vec<Occurrence<R>>) -> Vec<R> {
    found.into_iter().map(|o| o.record).collect()
}
}
