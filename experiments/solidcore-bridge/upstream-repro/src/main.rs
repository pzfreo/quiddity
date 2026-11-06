//! Does a boolean cut through a part's axis come back empty with solidcore's own importer,
//! and with plain primitives?
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p1::faces;
use solidcore::{Brep, Kernel, Point3};
use std::io::Read;

fn volume(k: &Kernel, b: &Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes { store: &store, key_of: &|_| ObjectKey(0) };
    let mut copy = b.clone_standing_for_self().unwrap();
    brep_volume(k, &mut copy, 1e-6, &attrs).unwrap_or(f64::NAN)
}

/// Removes everything with coordinate `axis` > `at`, using a 2000-unit box.
fn cut(k: &mut Kernel, b: Brep, axis: usize, at: f64) -> Result<Brep, solidcore::SolidError> {
    let mut o = [-1000.0; 3];
    let mut s = [2000.0; 3];
    o[axis] = at;
    s[axis] = 1000.0 - at;
    let tool = create_box(k, Point3::new(o[0], o[1], o[2]), s[0], s[1], s[2])?;
    let mut r = boolean_difference(k, b, tool)?;
    r.refresh_cached_topology();
    Ok(r)
}

fn run(label: &str, k: &mut Kernel, b: &Brep, axis: usize, at: f64) {
    let before = volume(k, b);
    let copy = b.clone_standing_for_self().unwrap();
    match cut(k, copy, axis, at) {
        Ok(r) => println!(
            "{label}: cut {} > {at}: volume {before:.3} -> {:.3}, faces {} -> {}",
            ["x", "y", "z"][axis],
            volume(k, &r),
            faces(b, None).unwrap().len(),
            faces(&r, None).unwrap().len()
        ),
        Err(e) => println!("{label}: cut {} > {at}: error {e:?}", ["x", "y", "z"][axis]),
    }
}

fn main() {
    let mut k = Kernel::default();
    // 1. Plain primitives: a cylinder on the z axis, cut through its axis.
    let cyl = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    run("cylinder r10 h20", &mut k, &cyl, 0, 0.0);
    run("cylinder r10 h20", &mut k, &cyl, 1, 0.0);
    run("cylinder r10 h20", &mut k, &cyl, 0, 3.0);

    // 2. solidcore's own STEP importer on the test file.
    let path = std::env::args().nth(1).expect("STEP file");
    let raw = std::fs::read(&path).unwrap();
    let bytes = if path.ends_with(".gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice()).read_to_end(&mut out).unwrap();
        out
    } else {
        raw
    };
    let report = step_brep::import_with_report(&bytes, &step_import::Limits::default()).unwrap();
    println!("step-brep import: {} bodies, {} skipped", report.bodies.len(), report.skipped.len());
    let body = &report.bodies[0];
    let sweep: Vec<(usize, f64)> = std::env::args().nth(2).map(|a| {
        let axis = "xyz".find(&a[..1]).unwrap();
        a[2..].split(',').map(|v| (axis, v.parse().unwrap())).collect()
    }).unwrap_or_else(|| vec![(1, 0.0), (0, 0.0), (2, 70.0), (0, -26.0), (1, 0.001), (1, 1.0)]);
    for (axis, at) in sweep {
        run("imported part", &mut k, body, axis, at);
    }
}
