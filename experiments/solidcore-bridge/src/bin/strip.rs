//! Writes a geometry-only copy of a STEP file: every part's PMI and its presentation removed by
//! haecceity's PMI writer (`Mode::Remove`), everything else kept as it was.
//!
//! Usage: `strip IN.step[.gz] OUT.step`
use haecceity::p21::Document;
use haecceity::pmi::write::{Mode, PresentationPolicy};
use haecceity::pmi::{self, PartId, PartPmi};
use haecceity::step::read_part_definitions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let (input, output) = (args.next().ok_or("IN")?, args.next().ok_or("OUT")?);
    let bytes = solidcore_bridge_prototype::step_bytes(std::path::Path::new(&input))?;
    let parts = read_part_definitions(&bytes)?;
    let doc = Document::parse(bytes)?;
    let empty: Vec<(PartId, PartPmi)> = (0..parts.len()).map(|i| (PartId(i), PartPmi::default())).collect();
    let (edit, report) = pmi::write(&doc, &parts, &empty, Mode::Remove, PresentationPolicy::RemovePresentation)?;
    println!("{report:?}");
    let written = doc.apply(&edit)?;
    std::fs::write(&output, &written.bytes)?;

    // Check: nothing left for the reader, and how big the file is now.
    let again = Document::parse(written.bytes.clone())?;
    let read = pmi::read(&again, &read_part_definitions(&written.bytes)?)?;
    let p = &read.parts[0];
    println!(
        "{output}: {} bytes; remaining features {}, dimensions {}, tolerances {}, datums {}, notes {}, callouts {}, findings {}",
        written.bytes.len(), p.features.len(), p.dimensions.len(), p.tolerances.len(), p.datums.len(),
        p.notes.len(), read.callouts.len(), read.findings.len()
    );
    Ok(())
}
