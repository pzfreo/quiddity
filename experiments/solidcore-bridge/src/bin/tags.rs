//! Phase 0, step A: do face attributes survive solidcore booleans?
//!
//! Tag every face of each operand with a "STEP id" (a long attribute under an application
//! id), run a boolean, and report which tags each face of the result carries.
//!
//! Usage: `solidcore-bridge-prototype [copy|reference|standalone] [hole|slot|merge]`
//!
//! - `hole`:  box minus a through cylinder (faces get holes; no face splits)
//! - `slot`:  box minus a box right across its top (the top face splits in two)
//! - `merge`: box plus a box flush against it (coplanar faces meet)

use solidcore::boolean::api_brep::{boolean_difference, boolean_union};
use solidcore::geom::attribute::{Attribute, AttributeValue, Behavior, ids};
use solidcore::mass::integrator::properties::face_area;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1::faces;
use solidcore::{Brep, EntityRef, Kernel, Point3};

/// Attribute ids from here up name a face's source STEP entity: id `STEP_BASE + n` holds `#n`.
/// One id per entity, because solidcore's default merge and coincident handlers copy an
/// attribute onto the surviving face only when it has none of that id: with one shared id, a
/// merged face would keep one operand's tag and silently drop the other's.
const STEP_BASE: u64 = ids::USER_1 + (1 << 40);

fn tag_faces(brep: &mut Brep, first_id: i64, behavior: Behavior) -> solidcore::Result<Vec<i64>> {
    let mut tags = Vec::new();
    for (i, f) in faces(brep, None)?.into_iter().enumerate() {
        let id = first_id + i as i64;
        let attribute_id = STEP_BASE + id as u64;
        brep.add_attribute(EntityRef::Face(f), Attribute::long(attribute_id, id, behavior))?;
        tags.push(id);
    }
    Ok(tags)
}

fn report(k: &Kernel, label: &str, brep: &mut Brep) -> solidcore::Result<()> {
    println!("{label}:");
    for f in faces(brep, None)? {
        let tags: Vec<i64> = brep
            .entity_attributes(EntityRef::Face(f))
            .into_iter()
            .filter(|(_, a)| a.attribute_id() >= STEP_BASE)
            .filter_map(|(_, a)| match a.value() {
                AttributeValue::Long(v) => Some(*v),
                _ => None,
            })
            .collect();
        let kind: String = brep
            .face(f)?
            .surface
            .as_ref()
            .map_or("none".to_string(), |s| format!("{:?}", s.kind()))
            .chars()
            .take(5)
            .collect();
        let area = face_area(k, brep, f, 1e-6)?;
        println!("  face {:>3}  {kind:<5}  area {area:>9.3}  tags {tags:?}", f.index());
    }
    Ok(())
}

fn main() -> solidcore::Result<()> {
    let mut args = std::env::args().skip(1);
    let behavior = match args.next().as_deref() {
        Some("reference") => Behavior::Reference,
        Some("standalone") => Behavior::StandaloneCopy,
        _ => Behavior::Copy,
    };
    let scenario = args.next().unwrap_or_else(|| "hole".into());

    let mut k = Kernel::default();
    let mut block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0)?;
    let mut tool = match scenario.as_str() {
        "hole" => create_cylinder(&mut k, Point3::new(20.0, 15.0, -5.0), 4.0, 20.0)?,
        "slot" => create_box(&mut k, Point3::new(15.0, -5.0, 5.0), 10.0, 40.0, 10.0)?,
        "merge" => create_box(&mut k, Point3::new(40.0, 0.0, 0.0), 20.0, 30.0, 10.0)?,
        other => {
            eprintln!("unknown scenario {other}");
            std::process::exit(2);
        }
    };
    let block_tags = tag_faces(&mut block, 100, behavior)?;
    let tool_tags = tag_faces(&mut tool, 900, behavior)?;
    println!("scenario {scenario}, behaviour {behavior:?}");
    println!("block tags {block_tags:?}; tool tags {tool_tags:?}");
    report(&k, "block", &mut block)?;
    report(&k, "tool", &mut tool)?;

    let mut result = if scenario == "merge" {
        boolean_union(&mut k, block, tool)?
    } else {
        boolean_difference(&mut k, block, tool)?
    };
    report(&k, "result", &mut result)?;
    Ok(())
}
