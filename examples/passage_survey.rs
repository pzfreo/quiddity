//! Read-only polygonal-passage diagnostics, caller face IDs and original STEP sources.
//! `passage_survey STEPS_DIR REQUESTS.jsonl [--cuts-only]`.
//! Each request has an `id`; labels are neither read nor used by the geometric detector.
#[path = "../src/engine.rs"]
mod engine;
pub use engine::*;
extern crate self as quiddity;
use serde_json::json;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let steps = std::path::Path::new(&args[1]);
    for line in std::fs::read_to_string(&args[2]).unwrap().lines() {
        let req: serde_json::Value = serde_json::from_str(line).unwrap();
        let id = req["id"].as_str().unwrap();
        let p = read_step_file(&steps.join(format!("{id}.step"))).unwrap();
        let ctx = features::Context::new(&p);
        if args.get(3).is_some_and(|v| v == "--cuts-only") {
            let cuts = features::prismatic_through_cuts::discover(&ctx);
            println!(
                "{}",
                json!({"id":id,"cuts":cuts.iter().map(|o|json!({"faces":o.defining,"record":o.record})).collect::<Vec<_>>() })
            );
            continue;
        }
        let rings: Vec<_> = features::rings::rings(&ctx)
            .iter()
            .map(|r| json!({"faces":r.nodes,"caps":[r.caps().0,r.caps().1],"axis":r.axis}))
            .collect();
        let faces: Vec<_> = p.faces.iter().enumerate().map(|(i,f)|json!({
            "face":i,"source":p.face_source(i).map(|s|s.entity),"kind":format!("{:?}",f.surface.kind()),
            "normal":features::graph::normal(&p,i),"bounds":[p.face_bounds(i).min,p.face_bounds(i).max],
            "neighbours":p.neighbours(i).iter().map(|&j|json!([j,format!("{:?}",p.arc(i,j))])).collect::<Vec<_>>(),
            "loops":f.loops.len(),"vertices":features::graph::face_vertices(&p,i),
        })).collect();
        println!(
            "{}",
            json!({"id":id,"solid_valid":p.solid_is_valid(0),"geometric_valid":p.solid_is_geometrically_valid(0),"bad":p.bad_faces(0),"rings":rings,"faces":faces})
        );
    }
}
