//! Which test files have PMI on faces and only planes, cylinders, lines and circles?
use haecceity::geom::{Curve, Surface};
use haecceity::p21::Document;
use haecceity::pmi::{self, Anchor, Feature};
use haecceity::step::{read_part, read_part_definitions};

fn main() {
    for path in std::env::args().skip(1) {
        let path = std::path::Path::new(&path);
        let Ok(bytes) = solidcore_bridge_prototype::step_bytes(path) else { continue };
        let Ok(defs) = read_part_definitions(&bytes) else { continue };
        if defs.len() != 1 { continue; }
        let Ok(part) = read_part(&bytes, &defs[0]) else { continue };
        let mut kinds = std::collections::BTreeMap::new();
        for f in &part.faces { *kinds.entry(format!("{:?}", f.surface.kind())).or_insert(0) += 1; }
        if std::env::var("KINDS").is_ok() { println!("{kinds:?}"); }
        let simple_faces = part.faces.iter().all(|f| matches!(f.surface, Surface::Plane { .. } | Surface::Cylinder { .. }));
        let simple_edges = part.edges.iter().all(|e| matches!(e.curve, Curve::Line { .. } | Curve::Circle { .. }));
        let Ok(doc) = Document::parse(bytes.clone()) else { continue };
        let Ok(read) = pmi::read(&doc, &defs) else { continue };
        let p = &read.parts[0];
        let face_features = p.features.iter().filter(|f| matches!(f, Feature::Items(a) if a.iter().any(|x| matches!(x, Anchor::Face(_))))).count();
        println!("{:<5} {:<5} faces {:>4} solids {} features {:>3} on faces {:>3} dims {:>2} tols {:>2} datums {:>2}  {}",
            simple_faces, simple_edges, part.faces.len(), part.solids.len(), p.features.len(), face_features,
            p.dimensions.len(), p.tolerances.len(), p.datums.len(), path.display());
    }
}
