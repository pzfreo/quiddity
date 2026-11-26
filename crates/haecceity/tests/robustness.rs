//! Malformed and hostile STEP input is refused, never a panic or a stack overflow (#52).
//!
//! - Hostile files (deep nesting, long and cyclic reference chains, huge counts, truncation)
//!   are read on a 1 MiB thread stack, the WASM-like constraint, and must come back as a
//!   refusal or a part, never unwind or overflow.
//! - A deterministic mutation sweep truncates and byte-flips fixtures at a few positions each
//!   and reads every mutant through the whole read path; no mutant may panic.
//!   `HAECCEITY_SWEEP=<positions per file>` widens it (the default budget is small).
//! - The bounds the reader enforces refuse no file of the fixtures or the corpus.

mod common;

use std::io::Read;
use std::path::{Path, PathBuf};

use haecceity::p21::Document;
use haecceity::step::{StepError, read_part, read_part_definitions};

/// The WASM-like stack the hostile inputs are read on.
const SMALL_STACK: usize = 1 << 20;

fn read_file(path: &Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap();
    if path.extension().is_some_and(|e| e == "gz") {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&raw[..])
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        raw
    }
}

fn step_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries {
            let p = e.unwrap().path();
            let name = p
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            if p.is_dir() {
                if name != "rust-snapshot" {
                    stack.push(p);
                }
            } else if [".step", ".stp", ".step.gz", ".stp.gz"]
                .iter()
                .any(|s| name.ends_with(s))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every refusal the read path gave, as text.
#[derive(Debug, Default)]
struct Outcome {
    geometry: Option<Result<haecceity::Part, String>>,
    refusals: Vec<String>,
    limits: Vec<String>,
}

fn note<T, E: std::fmt::Display>(out: &mut Outcome, what: &str, r: Result<T, E>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            out.refusals.push(format!("{what}: {e}"));
            None
        }
    }
}

fn step_note<T>(out: &mut Outcome, what: &str, r: Result<T, StepError>) -> Option<T> {
    if let Err(StepError::Limit(why)) = &r {
        out.limits.push(format!("{what}: {why}"));
    }
    note(out, what, r)
}

/// The whole read path: geometry, part definitions and each part, the lossless document, its
/// PMI and its schema validation.
fn read_everything(bytes: &[u8]) -> Outcome {
    let mut out = Outcome::default();
    let geometry = haecceity::read_step(bytes);
    if let Err(StepError::Limit(why)) = &geometry {
        out.limits.push(format!("geometry: {why}"));
    }
    out.geometry = Some(geometry.map_err(|e| e.to_string()));
    let parts = step_note(&mut out, "part definitions", read_part_definitions(bytes));
    for p in parts.iter().flatten() {
        step_note(&mut out, "part", read_part(bytes, p));
    }
    if let Some(doc) = note(&mut out, "document", Document::parse(bytes.to_vec())) {
        let parts = parts.unwrap_or_default();
        if let Some(read) = note(&mut out, "pmi", haecceity::pmi::read(&doc, &parts)) {
            out.limits.extend(
                read.findings
                    .iter()
                    .filter(|f| f.detail.contains("nested more than"))
                    .map(|f| format!("pmi: {}", f.detail)),
            );
        }
        let _ = haecceity::express::validate_document(&doc);
    }
    out
}

/// The kernel queries a reader's caller makes of a part; hidden lines only when `hlr`.
fn query(part: &haecceity::Part, hlr: bool) {
    let _ = part.mesh(0.5, 0.5);
    let _ = part.volume_centroid();
    let view = haecceity::hlr::View::new([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    if let Some(view) = view.filter(|_| hlr) {
        let _ = haecceity::hlr::project(part, &view);
    }
}

/// The largest file (in bytes of Part 21) whose mutants the sweep also projects: HLR of a
/// larger part takes seconds even unmutated (15 s for left_two_start's 170 kB), so the default
/// budget leaves those to the HLR tests themselves.
const SWEEP_HLR_BYTES: usize = 50_000;

/// `f` run on a thread with the WASM-like stack; a panic fails the test, an overflow aborts it.
fn on_small_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e))
}

fn torus() -> String {
    std::fs::read_to_string(common::fixtures().join("full_torus.step")).unwrap()
}

/// `base` with `line` (`#N = …;`) replaced and `extra` instances added before `ENDSEC`.
fn edited(base: &str, from: &str, to: &str, extra: &str) -> String {
    assert!(base.contains(from), "{from}");
    let s = base.replacen(from, to, 1);
    let at = s.rfind("ENDSEC;").unwrap();
    format!("{}{extra}{}", &s[..at], &s[at..])
}

/// A file read on the small stack: refused or read, and a part read is queried.
fn refused_or_read(text: String) -> Outcome {
    on_small_stack(move || {
        let out = read_everything(text.as_bytes());
        if let Some(Ok(part)) = &out.geometry {
            query(part, true);
        }
        out
    })
}

#[test]
fn deep_parameter_nesting_is_refused_on_a_small_stack() {
    let deep = format!("{}0.{}", "(".repeat(100_000), ")".repeat(100_000));
    let text = edited(
        &torus(),
        "#12 = CARTESIAN_POINT('',(0.,0.,0.));",
        &format!("#12 = CARTESIAN_POINT('',{deep});"),
        "",
    );
    let out = refused_or_read(text);
    assert!(matches!(out.geometry, Some(Err(_))), "{out:?}");
    // Nesting the parser accepts, walked by every reader of attributes.
    let depth = step_io::parser::MAX_NESTING_DEPTH - 2;
    let nested = format!("{}0.{}", "(".repeat(depth), ")".repeat(depth));
    for (from, to) in [
        (
            "#12 = CARTESIAN_POINT('',(0.,0.,0.));",
            format!("#12 = CARTESIAN_POINT('',{nested});"),
        ),
        (
            "#78 = PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#7));",
            format!("#78 = PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,{nested});"),
        ),
    ] {
        refused_or_read(edited(&torus(), from, &to, ""));
    }
}

/// Refused, or read with the hostile geometry unresolved.
fn refused_or_degraded(out: &Outcome) -> bool {
    match &out.geometry {
        Some(Ok(part)) => {
            !part.unresolved_faces().is_empty() || !part.unresolved_edges().is_empty()
        }
        Some(Err(_)) => true,
        None => false,
    }
}

/// `n` instances `#first…`, each `line(id, next)`, the last naming `end`.
fn chain(n: usize, first: usize, end: usize, line: impl Fn(usize, usize) -> String) -> String {
    (0..n)
        .map(|k| line(first + k, if k + 1 == n { end } else { first + k + 1 }))
        .collect()
}

const CHAIN: usize = 10_000;

/// A seam reached through 10 000 surface curves (refused at the reader's bound), a curve
/// through 10 000 trimmed curves, and a surface through 10 000 offsets.
#[test]
fn long_reference_chains_are_refused_on_a_small_stack() {
    let seam = |first: usize| format!("#24 = SEAM_CURVE('',#{first},");
    let cases = [
        (
            seam(1000),
            chain(CHAIN, 1000, 25, |id, next| {
                format!("#{id} = SURFACE_CURVE('',#{next},(#30,#42),.PCURVE_S1.);\n")
            }),
        ),
        (
            seam(1000),
            chain(CHAIN, 1000, 25, |id, next| {
                format!(
                    "#{id} = TRIMMED_CURVE('',#{next},(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);\n"
                )
            }),
        ),
    ];
    for (k, (to, extra)) in cases.into_iter().enumerate() {
        let out = refused_or_read(edited(&torus(), "#24 = SEAM_CURVE('',#25,", &to, &extra));
        assert!(refused_or_degraded(&out), "{:?}", out.refusals);
        assert!(k > 0 || !out.limits.is_empty(), "{:?}", out.refusals);
    }
    let surfaces = chain(CHAIN, 1000, 31, |id, next| {
        format!("#{id} = OFFSET_SURFACE('',#{next},0.,.F.);\n")
    });
    let out = refused_or_read(edited(
        &torus(),
        "#17 = ADVANCED_FACE('',(#18),#31,.T.);",
        "#17 = ADVANCED_FACE('',(#18),#1000,.T.);",
        &surfaces,
    ));
    assert!(refused_or_degraded(&out), "{:?}", out.refusals);
}

/// Curves and surfaces that are their own basis, directly or through another.
#[test]
fn cyclic_references_are_refused_on_a_small_stack() {
    let seam = "#24 = SEAM_CURVE('',#1000,";
    for (k, extra) in [
        "#1000 = SURFACE_CURVE('',#1000,(#30,#42),.PCURVE_S1.);\n",
        "#1000 = SURFACE_CURVE('',#1001,(#30,#42),.PCURVE_S1.);\n\
         #1001 = SEAM_CURVE('',#1000,(#30,#42),.PCURVE_S1.);\n",
        "#1000 = TRIMMED_CURVE('',#1001,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);\n\
         #1001 = TRIMMED_CURVE('',#1000,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(1.)),.T.,.PARAMETER.);\n",
    ]
    .into_iter()
    .enumerate()
    {
        let out = refused_or_read(edited(&torus(), "#24 = SEAM_CURVE('',#25,", seam, extra));
        assert!(refused_or_degraded(&out), "{extra}: {:?}", out.refusals);
        // A surface-curve cycle is refused at the reader's bound; a trimmed curve is not
        // followed, so its cycle leaves the edge unresolved.
        assert!(k == 2 || !out.limits.is_empty(), "{extra}: {:?}", out.refusals);
    }
    let out = refused_or_read(edited(
        &torus(),
        "#17 = ADVANCED_FACE('',(#18),#31,.T.);",
        "#17 = ADVANCED_FACE('',(#18),#1000,.T.);",
        "#1000 = OFFSET_SURFACE('',#1000,0.,.F.);\n",
    ));
    assert!(refused_or_degraded(&out), "{:?}", out.refusals);
}

/// A dimension's shape aspect reached through 10 000 composite groups, each the one member of
/// the last: the PMI reader stops at its nesting bound with a finding.
#[test]
fn deep_pmi_feature_nesting_is_refused_on_a_small_stack() {
    let path = common::fixtures().join("ap242/nist/nist_ctc_01_asme1_ap242-e1.stp.gz");
    let text = String::from_utf8(read_file(&path)).unwrap();
    let groups = chain(CHAIN, 100_000, 289, |id, next| {
        format!(
            "#{id}=COMPOSITE_GROUP_SHAPE_ASPECT('','multiple elements',#4269,.F.);\n\
             #{}=SHAPE_ASPECT_RELATIONSHIP('',$,#{id},#{next});\n",
            id + CHAIN
        )
    });
    let text = edited(
        &text,
        "#175=SHAPE_ASPECT_RELATIONSHIP('',$,#219,#289);",
        "#175=SHAPE_ASPECT_RELATIONSHIP('',$,#219,#100000);",
        &groups,
    );
    let findings = on_small_stack(move || {
        let bytes = text.into_bytes();
        let parts = read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        haecceity::pmi::read(&doc, &parts).unwrap().findings
    });
    assert!(
        findings
            .iter()
            .any(|f| f.detail.contains("nested more than")),
        "{findings:?}"
    );
}

/// `wrapped.step` (an assembly placing one plate) with `levels` assemblies between its root and
/// the plate, each placing the next `fan` times.
fn assembly_chain(levels: usize, fan: usize) -> String {
    let base =
        std::fs::read_to_string(common::fixtures().join("ap242/assembly/wrapped.step")).unwrap();
    // Level k (1-based) has its definition at #(base + 20k + 2) and shape at #(base + 20k + 7).
    let at = |k: usize| 2000 + 20 * k;
    let pd = |k: usize| match k {
        0 => (5, 10),
        k if k > levels => (27, 32),
        k => (at(k) + 2, at(k) + 7),
    };
    let mut extra = String::new();
    for k in 1..=levels {
        let b = at(k);
        extra.push_str(&format!(
            "#{b} = SHAPE_DEFINITION_REPRESENTATION(#{},#{});\n\
             #{} = PRODUCT_DEFINITION_SHAPE('','',#{});\n\
             #{} = PRODUCT_DEFINITION('design','',#{},#9);\n\
             #{} = PRODUCT_DEFINITION_FORMATION('','',#{});\n\
             #{} = PRODUCT('level{k}','level{k}','',(#8));\n\
             #{} = SHAPE_REPRESENTATION('',(#11,#15),#19);\n",
            b + 1,
            b + 7,
            b + 1,
            b + 2,
            b + 2,
            b + 3,
            b + 3,
            b + 4,
            b + 4,
            b + 7
        ));
    }
    let mut next = 100_000;
    for k in 1..=levels + 1 {
        let ((parent, parent_rep), (child, child_rep)) = (pd(k - 1), pd(k));
        for _ in 0..if k == 1 { 1 } else { fan } {
            let n = next;
            next += 5;
            extra.push_str(&format!(
                "#{n} = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{},#{});\n\
                 #{} = ( REPRESENTATION_RELATIONSHIP('','',#{child_rep},#{parent_rep}) \
                 REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{}) SHAPE_REPRESENTATION_RELATIONSHIP() );\n\
                 #{} = ITEM_DEFINED_TRANSFORMATION('','',#11,#15);\n\
                 #{} = PRODUCT_DEFINITION_SHAPE('Placement','',#{});\n\
                 #{} = NEXT_ASSEMBLY_USAGE_OCCURRENCE('{n}','x','',#{parent},#{child},$);\n",
                n + 1, n + 3, n + 1, n + 2, n + 2, n + 3, n + 4, n + 4
            ));
        }
    }
    // The original placement of the plate goes; the chain stands for it.
    let s = base.replacen(
        "#956 = CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#957,#959);\n",
        "",
        1,
    );
    let s = s.replacen(
        "#960 = NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','plate','',#5,#27,$);\n",
        "",
        1,
    );
    let s = s.replacen(
        "#959 = PRODUCT_DEFINITION_SHAPE('Placement','Placement of an item',#960\n  );\n",
        "",
        1,
    );
    let s = s.replacen("#957 = ( REPRESENTATION_RELATIONSHIP('','',#32,#10) \nREPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#958) \nSHAPE_REPRESENTATION_RELATIONSHIP() );\n#958 = ITEM_DEFINED_TRANSFORMATION('','',#11,#15);\n", "", 1);
    let at = s.rfind("ENDSEC;").unwrap();
    format!("{}{extra}{}", &s[..at], &s[at..])
}

/// Assemblies nested past the reader's depth, and fanning out past its instance count.
#[test]
fn deep_and_wide_assemblies_are_refused_on_a_small_stack() {
    // Within the bounds: read, the plate placed once at the bottom.
    let out = refused_or_read(assembly_chain(4, 1));
    assert!(
        matches!(&out.geometry, Some(Ok(p)) if !p.faces.is_empty()),
        "{:?}",
        out.refusals
    );
    let out = refused_or_read(assembly_chain(
        haecceity::step::limits::ASSEMBLY_DEPTH + 1,
        1,
    ));
    assert!(!out.limits.is_empty(), "{:?}", out.refusals);
    // 2^40 placements.
    let out = refused_or_read(assembly_chain(40, 2));
    assert!(!out.limits.is_empty(), "{:?}", out.refusals);
}

/// B-splines whose multiplicities or degree would expand into an unbounded knot vector.
#[test]
fn huge_counts_are_refused_on_a_small_stack() {
    let spline = |degree: &str, mults: &str| {
        format!(
            "#25 = ( BOUNDED_CURVE() B_SPLINE_CURVE({degree},(#23,#23,#23,#23),.UNSPECIFIED.,.F.,.F.) \
             B_SPLINE_CURVE_WITH_KNOTS(({mults}),(0.,1.),.UNSPECIFIED.) CURVE() \
             GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE((1.,1.,1.,1.)) \
             REPRESENTATION_ITEM('') );"
        )
    };
    for (degree, mults) in [
        ("3", "4000000000000000000,4"),
        ("3", "9223372036854775807,9223372036854775807"),
        ("1000000000", "4,4"),
        ("3", "-4,4"),
    ] {
        let text = edited(
            &torus(),
            "#25 = CIRCLE('',#26,12.);",
            &spline(degree, mults),
            "",
        );
        let out = refused_or_read(text);
        assert!(
            refused_or_degraded(&out),
            "{degree} {mults}: {:?}",
            out.refusals
        );
    }
    // A bound listing one edge more times than any bound may have edges.
    let edges = vec!["#20"; haecceity::step::limits::LOOP_EDGES + 1].join(",");
    let text = edited(
        &torus(),
        "#19 = EDGE_LOOP('',(#20,#49,#71,#72));",
        &format!("#19 = EDGE_LOOP('',({edges}));"),
        "",
    );
    let out = refused_or_read(text);
    assert!(!out.limits.is_empty(), "{:?}", out.refusals);
}

#[test]
fn truncated_files_are_refused_on_a_small_stack() {
    let text = torus();
    for cut in (0..text.len()).step_by(97) {
        let out = refused_or_read(text[..cut].to_string());
        assert!(matches!(out.geometry, Some(Err(_))), "cut at {cut}");
    }
}

/// splitmix64: deterministic positions without a dependency.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Bytes a flip writes: Part 21's structure and the characters values are made of.
const FLIPS: &[u8] = b"()#,;'=$*.-0123456789E\\/!X";

/// Each file truncated and byte-flipped at `per_file(len)` deterministic positions, every
/// mutant read through the whole read path: any panic is reported with the mutation that caused
/// it.
fn sweep(files: &[PathBuf], per_file: impl Fn(usize) -> usize + Sync) -> (usize, Vec<String>) {
    let found = common::parallel::map(files, |path| {
        let bytes = read_file(path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let seed = name.bytes().fold(0u64, |h, b| mix(h ^ u64::from(b)));
        let mut problems = Vec::new();
        let n = per_file(bytes.len()) as u64;
        for k in 0..n {
            let r = mix(seed ^ k);
            let at = (r % bytes.len().max(1) as u64) as usize;
            let mutant = if k % 2 == 0 {
                bytes[..at].to_vec()
            } else {
                let mut m = bytes.clone();
                if let Some(b) = m.get_mut(at) {
                    *b = FLIPS[(r >> 32) as usize % FLIPS.len()];
                }
                m
            };
            let what = if k % 2 == 0 { "truncated" } else { "flipped" };
            // A mutant that still reads is queried too, as a caller would (mesh, mass, HLR).
            let result = std::panic::catch_unwind(|| {
                if let Some(Ok(part)) = &read_everything(&mutant).geometry {
                    query(part, bytes.len() <= SWEEP_HLR_BYTES);
                }
            });
            if let Err(e) = result {
                let why = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                problems.push(format!("{name} {what} at {at}: panicked: {why}"));
            }
        }
        (n as usize, problems)
    });
    let count = found.iter().map(|(n, _)| n).sum();
    (count, found.into_iter().flat_map(|(_, p)| p).collect())
}

/// The default budget: the fixtures under 200 kB outside `captured/`, six mutations each.
/// `HAECCEITY_SWEEP=N` sweeps every fixture outside `captured/`, every tenth captured one and
/// the corpus, N mutations per file under 1 MB and N/10 for larger ones.
#[test]
fn mutated_fixtures_never_panic() {
    let wide: Option<usize> = std::env::var("HAECCEITY_SWEEP")
        .ok()
        .and_then(|s| s.parse().ok());
    let captured = |p: &PathBuf| p.components().any(|c| c.as_os_str() == "captured");
    let mut files = step_files(&common::fixtures());
    match wide {
        None => {
            files.retain(|p| !captured(p) && std::fs::metadata(p).is_ok_and(|m| m.len() < 200_000))
        }
        Some(_) => {
            let mut k = 0;
            files.retain(|p| {
                k += usize::from(captured(p));
                !captured(p) || k % 10 == 0
            });
            if let Some(dir) = common::corpus_dir() {
                files.extend(step_files(&dir));
            }
        }
    }
    // The hook would print every caught panic; the sweep reports them itself.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (count, problems) = sweep(&files, |len| match wide {
        None => 6,
        Some(n) if len < 1_000_000 => n,
        Some(n) => (n / 10).max(1),
    });
    std::panic::set_hook(hook);
    eprintln!("swept {} files, {count} mutants", files.len());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The reader's bounds (`StepError::Limit`) refuse no fixture and no corpus file.
#[test]
fn bounds_refuse_nothing_in_the_fixtures_or_the_corpus() {
    let mut files = step_files(&common::fixtures());
    if let Some(dir) = common::corpus_dir() {
        files.extend(step_files(&dir));
    }
    let limited: Vec<String> = common::parallel::map(&files, |path| {
        let out = read_everything(&read_file(path));
        out.limits
            .iter()
            .map(|l| format!("{}: {l}", path.display()))
            .collect::<Vec<_>>()
    })
    .into_iter()
    .flatten()
    .collect();
    eprintln!("read {} files", files.len());
    assert!(limited.is_empty(), "{}", limited.join("\n"));
}
