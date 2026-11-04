//! The lossless Part 21 document: every corpus, NIST and fixture file written back byte for
//! byte with no edit, every instance's extent and record checked against step-io, edits confined
//! to their spans and refused when they would leave a reference dangling, and the encoder's
//! reals, strings and complex records read back by step-io.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use haecceity::p21::{
    Attribute, Complex, Document, Edit, P21Error, RawEntity, RawEntityPart, Record, decode, encode,
    encode_real, encode_real_text, escape, leaf, simple,
};

fn read(path: &Path) -> Vec<u8> {
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

/// Files under `dir` whose names end with one of `suffixes`, sorted.
fn files(dir: &Path, suffixes: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if suffixes
                .iter()
                .any(|s| p.to_string_lossy().to_ascii_lowercase().ends_with(s))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// NIST's MBE PMI test models: `HAECCEITY_NIST_PMI`, else the AP242 track's download.
fn nist_dir() -> Option<PathBuf> {
    let dir = std::env::var("HAECCEITY_NIST_PMI")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(
                "/private/tmp/claude-501/-Users-paul-repos-quiddity-rust/\
                 98a3d09f-22d9-418c-b8db-3b48db0cb346/scratchpad/ap242-nist/NIST-PMI-STEP-Files",
            )
        });
    if dir.is_dir() {
        Some(dir)
    } else {
        assert!(
            std::env::var_os("HAECCEITY_NIST_PMI_REQUIRED").is_none(),
            "NIST PMI files required but {} is missing",
            dir.display()
        );
        None
    }
}

fn nist_file(name: &str) -> Option<PathBuf> {
    nist_dir().map(|d| d.join(name)).filter(|p| p.is_file())
}

/// A record without its span (step-io's spans are the `#N` token's position in the file).
fn unspanned(e: &RawEntity) -> RawEntity {
    let span = simple("X", vec![]).span();
    match e.clone() {
        RawEntity::Simple {
            id,
            name,
            attributes,
            ..
        } => RawEntity::Simple {
            id,
            name,
            attributes,
            span,
        },
        RawEntity::Complex { id, parts, .. } => RawEntity::Complex { id, parts, span },
    }
}

fn with_id(e: &RawEntity, new_id: u64) -> RawEntity {
    match unspanned(e) {
        RawEntity::Simple {
            name,
            attributes,
            span,
            ..
        } => RawEntity::Simple {
            id: new_id,
            name,
            attributes,
            span,
        },
        RawEntity::Complex { parts, span, .. } => RawEntity::Complex {
            id: new_id,
            parts,
            span,
        },
    }
}

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('t'),'2;1');\n\
                    FILE_NAME('t','',(''),(''),'','','');\nFILE_SCHEMA(('T'));\nENDSEC;\nDATA;\n";
const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

/// A minimal file holding `data` as its DATA section.
fn file(data: &str) -> Vec<u8> {
    format!("{HEAD}{data}{TAIL}").into_bytes()
}

/// What `p21` and step-io found in one file, or the problems with it.
fn check_file(path: &Path) -> Result<(String, f64), String> {
    let bytes = read(path);
    let name = path.display().to_string();
    let t = std::time::Instant::now();
    let doc = match Document::parse(bytes.clone()) {
        Ok(d) => d,
        Err(e) => {
            // Refusing is right only where step-io refuses too.
            return match step_io::parser::parse(bytes.as_slice()) {
                Err(_) => Ok((format!("{name}: refused with step-io ({e})"), 0.0)),
                Ok(_) => Err(format!("{name}: refused but step-io reads it: {e}")),
            };
        }
    };
    let secs = t.elapsed().as_secs_f64();
    let mut problems = Vec::new();
    let out = doc.apply(&Edit::new()).unwrap();
    if out.bytes != bytes {
        problems.push("empty edit changed the bytes".to_string());
    }
    let graph = step_io::parser::parse(bytes.as_slice()).unwrap();
    let ids: Vec<u64> = doc.ids().collect();
    if ids != graph.entities.keys().copied().collect::<Vec<_>>() {
        problems.push("ids differ from step-io's".into());
    }
    // Every span is `#N` … `;`, spans do not overlap, and the spans alone, reparsed by step-io,
    // give step-io's records.
    let mut ordered: Vec<(u64, std::ops::Range<usize>)> =
        ids.iter().map(|&id| (id, doc.span(id).unwrap())).collect();
    ordered.sort_by_key(|(_, s)| s.start);
    let mut data = Vec::new();
    let mut last_end = 0;
    for (id, s) in &ordered {
        let text = &bytes[s.clone()];
        let head = format!("#{id}");
        if !text.starts_with(head.as_bytes())
            || text.get(head.len()).is_some_and(u8::is_ascii_digit)
            || text.last() != Some(&b';')
        {
            problems.push(format!("#{id}: span is not `#{id}` … `;`"));
        }
        if s.start < last_end {
            problems.push(format!("#{id}: span overlaps the one before"));
        }
        last_end = s.end;
        data.extend_from_slice(text);
        data.push(b'\n');
    }
    let mut rebuilt = HEAD.as_bytes().to_vec();
    rebuilt.extend_from_slice(&data);
    rebuilt.extend_from_slice(TAIL.as_bytes());
    match step_io::parser::parse(rebuilt.as_slice()) {
        Ok(g) => {
            for (id, e) in &graph.entities {
                if g.entities.get(id).map(unspanned) != Some(unspanned(e)) {
                    problems.push(format!("#{id}: the span's record differs from step-io's"));
                    break;
                }
                if doc.get(*id).map(unspanned) != Some(unspanned(e)) {
                    problems.push(format!("#{id}: record differs from step-io's"));
                    break;
                }
            }
        }
        Err(e) => problems.push(format!("the spans alone do not parse: {e}")),
    }
    // Every record, encoded and read back by step-io, is itself.
    let mut encoded = HEAD.to_string();
    for (id, e) in &graph.entities {
        match encode(e) {
            Ok(t) => encoded.push_str(&format!("#{id}={t};\n")),
            Err(err) => {
                problems.push(format!("#{id} does not encode: {err}"));
                break;
            }
        }
    }
    encoded.push_str(TAIL);
    match step_io::parser::parse(encoded.as_bytes()) {
        Ok(g) => {
            if let Some(id) = graph
                .entities
                .iter()
                .find(|(id, e)| g.entities.get(id).map(unspanned) != Some(unspanned(e)))
                .map(|(id, _)| id)
            {
                problems.push(format!("#{id} encodes to a different record"));
            }
        }
        Err(e) => problems.push(format!("the encoded records do not parse: {e}")),
    }
    if problems.is_empty() {
        Ok((
            format!("{name}: {} instances, parsed in {secs:.2} s", ids.len()),
            secs,
        ))
    } else {
        Err(format!("{name}: {}", problems.join("; ")))
    }
}

#[test]
fn every_file_round_trips_with_step_io_records_and_extents() {
    let mut paths = files(&common::fixtures(), &[".step", ".step.gz"]);
    let fixtures = paths.len();
    let corpus = match common::corpus_dir() {
        Some(dir) => {
            let c = files(&dir, &[".step", ".stp", ".step.gz", ".stp.gz"]);
            assert_eq!(c.len(), 100, "the corpus has 100 STEP files");
            c
        }
        None => {
            assert!(std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none());
            Vec::new()
        }
    };
    let nist = nist_dir().map_or_else(Vec::new, |d| files(&d, &[".stp"]));
    eprintln!(
        "{fixtures} fixtures, {} corpus files, {} NIST files",
        corpus.len(),
        nist.len()
    );
    paths.extend(corpus);
    paths.extend(nist);
    let results = common::parallel::map(&paths, |p| check_file(p));
    let mut problems = Vec::new();
    let mut refused = Vec::new();
    let mut slowest = (0.0, String::new());
    for r in results {
        match r {
            Ok((line, secs)) => {
                if line.contains("refused with step-io") {
                    refused.push(line);
                } else if secs > slowest.0 {
                    slowest = (secs, line);
                }
            }
            Err(p) => problems.push(p),
        }
    }
    eprintln!("slowest: {}", slowest.1);
    for r in &refused {
        eprintln!("{r}");
    }
    // Only the deliberately broken fixtures fail to parse, and step-io refuses them too.
    assert!(
        // Normalise Windows separators: the line holds the fixture's path.
        refused
            .iter()
            .all(|r| r.replace('\\', "/").contains("/broken/")),
        "refused outside fixtures/broken: {refused:#?}"
    );
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Every `#N` in each instance's text after its `=`, outside strings: the references, found
/// without step-io's records.
fn brute_force_referrers(doc: &Document) -> BTreeMap<u64, BTreeSet<u64>> {
    let mut out: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    for id in doc.ids() {
        let text = &doc.bytes()[doc.span(id).unwrap()];
        let eq = text.iter().position(|&c| c == b'=').unwrap();
        let (mut i, mut in_string) = (eq, false);
        while i < text.len() {
            match text[i] {
                b'\'' => in_string = !in_string,
                b'#' if !in_string => {
                    let j = i
                        + 1
                        + text[i + 1..]
                            .iter()
                            .position(|c| !c.is_ascii_digit())
                            .unwrap();
                    let t: u64 = std::str::from_utf8(&text[i + 1..j])
                        .unwrap()
                        .parse()
                        .unwrap();
                    out.entry(t).or_default().insert(id);
                    i = j;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }
    out
}

#[test]
fn referrers_agree_with_a_brute_force_scan() {
    let mut paths = vec![common::fixtures().join("memo_part.step")];
    paths.extend(nist_file("nist_ctc_01_asme1_ap242-e1.stp"));
    for path in paths {
        let doc = Document::parse(read(&path)).unwrap();
        let brute = brute_force_referrers(&doc);
        let mut targets: BTreeSet<u64> = doc.ids().collect();
        targets.extend(brute.keys());
        for t in targets {
            let want: Vec<u64> = brute
                .get(&t)
                .map_or_else(Vec::new, |s| s.iter().copied().collect());
            assert_eq!(doc.referrers(t), want, "{}: #{t}", path.display());
        }
    }
}

const SMALL: &str = "\
#1=CARTESIAN_POINT('origin; ''quoted''',(0.,0.,0.));
#2=DIRECTION('z /* not a comment */',(0.,0.,1.));
/* a comment with ; and ' */
#3=AXIS2_PLACEMENT_3D('',#1,#2,$);
#5 = PRODUCT('p','\\X2\\00D8\\X0\\ 20','',(#6));  #6=PRODUCT_CONTEXT('',#7,'mechanical');
#7=APPLICATION_CONTEXT('x');
#8=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#9=SHAPE_REPRESENTATION('',(#3),#8);
  #10=
  DESCRIPTIVE_REPRESENTATION_ITEM('multi','line');
";

fn point(name: &str, xyz: [f64; 3]) -> RawEntity {
    simple(
        "CARTESIAN_POINT",
        vec![
            Attribute::String(name.into()),
            Attribute::List(xyz.into_iter().map(Attribute::Real).collect()),
        ],
    )
}

#[test]
fn edits_touch_only_their_spans() {
    let doc = Document::parse(file(SMALL)).unwrap();
    assert_eq!(doc.max_id(), 10);
    assert_eq!(doc.referrers(1), [3]);
    assert_eq!(doc.referrers(3), [9]);
    assert_eq!(doc.file_schema().unwrap(), ["T"]);
    let text = |id| String::from_utf8(doc.bytes()[doc.span(id).unwrap()].to_vec()).unwrap();
    assert_eq!(
        text(10),
        "#10=\n  DESCRIPTIVE_REPRESENTATION_ITEM('multi','line');"
    );
    assert_eq!(text(5), "#5 = PRODUCT('p','\\X2\\00D8\\X0\\ 20','',(#6));");

    let mut edit = Edit::new();
    edit.replace(
        2,
        simple(
            "DIRECTION",
            vec![
                Attribute::String("y".into()),
                Attribute::List(vec![
                    Attribute::Real(0.0),
                    Attribute::Real(1.0),
                    Attribute::Real(0.0),
                ]),
            ],
        ),
    );
    edit.remove(10);
    edit.remove(5);
    let a = edit.add(point("new", [1.5, -0.25, 1e-7]));
    let b = edit.add(simple(
        "AXIS2_PLACEMENT_3D",
        vec![
            Attribute::String(String::new()),
            a.to_ref(),
            Attribute::EntityRef(2),
            Attribute::Unset,
        ],
    ));
    edit.set_file_schema(vec![
        "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF".into(),
    ]);
    let out = doc.apply(&edit).unwrap();
    assert_eq!(out.ids, BTreeMap::from([(a, 11), (b, 12)]));
    let want = HEAD.replace(
        "FILE_SCHEMA(('T'));",
        "FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));",
    ) + "\
#1=CARTESIAN_POINT('origin; ''quoted''',(0.,0.,0.));
#2=DIRECTION('y',(0.,1.,0.));
/* a comment with ; and ' */
#3=AXIS2_PLACEMENT_3D('',#1,#2,$);
  #6=PRODUCT_CONTEXT('',#7,'mechanical');
#7=APPLICATION_CONTEXT('x');
#8=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#9=SHAPE_REPRESENTATION('',(#3),#8);
#11=CARTESIAN_POINT('new',(1.5,-0.25,1.E-07));
#12=AXIS2_PLACEMENT_3D('',#11,#2,$);
" + TAIL;
    assert_eq!(String::from_utf8(out.bytes.clone()).unwrap(), want);
    // Deterministic, and step-io reads the result.
    assert_eq!(doc.apply(&edit).unwrap(), out);
    let again = Document::parse(out.bytes).unwrap();
    assert_eq!(
        again.get(12).map(unspanned),
        Some(with_id(
            &simple(
                "AXIS2_PLACEMENT_3D",
                vec![
                    Attribute::String(String::new()),
                    Attribute::EntityRef(11),
                    Attribute::EntityRef(2),
                    Attribute::Unset,
                ],
            ),
            12
        ))
    );
}

#[test]
fn edits_that_would_leave_a_reference_dangling_are_refused() {
    let doc = Document::parse(file(SMALL)).unwrap();
    let refused = |build: &dyn Fn(&mut Edit)| {
        let mut e = Edit::new();
        build(&mut e);
        doc.apply(&e).unwrap_err()
    };
    // A removed instance still referenced by a kept one.
    assert_eq!(
        refused(&|e| e.remove(1)),
        P21Error::StillReferenced { id: 1, by: vec![3] }
    );
    // … or by a replaced one whose new record still references it.
    let rep = simple(
        "SHAPE_REPRESENTATION",
        vec![
            Attribute::String(String::new()),
            Attribute::List(vec![Attribute::EntityRef(3)]),
            Attribute::EntityRef(8),
        ],
    );
    assert_eq!(
        refused(&|e| {
            e.remove(3);
            e.replace(9, rep.clone());
        }),
        P21Error::StillReferenced { id: 3, by: vec![9] }
    );
    // … or by an added one.
    assert_eq!(
        refused(&|e| {
            e.remove(3);
            e.remove(9);
            e.add(simple("X", vec![Attribute::EntityRef(3)]));
        }),
        P21Error::StillReferenced {
            id: 3,
            by: vec![11]
        }
    );
    // Removing the referrer with it, or replacing it without the reference, is fine.
    let mut ok = Edit::new();
    ok.remove(3);
    ok.remove(9);
    ok.remove(1);
    assert!(doc.apply(&ok).is_ok());
    let mut ok = Edit::new();
    ok.remove(3);
    ok.replace(
        9,
        simple(
            "SHAPE_REPRESENTATION",
            vec![
                Attribute::String(String::new()),
                Attribute::List(vec![]),
                Attribute::EntityRef(8),
            ],
        ),
    );
    assert!(doc.apply(&ok).is_ok());
    // References to nothing.
    assert_eq!(
        refused(&|e| {
            e.add(simple("X", vec![Attribute::EntityRef(99)]));
        }),
        P21Error::Dangling {
            from: 11,
            to: "#99".into()
        }
    );
    let mut other = Edit::new();
    other.add(simple("X", vec![]));
    let foreign = other.add(simple("X", vec![]));
    assert_eq!(foreign.index(), 1);
    assert_eq!(
        refused(&|e| e.replace(7, simple("X", vec![foreign.to_ref()]))),
        P21Error::Dangling {
            from: 7,
            to: "new #1".into()
        }
    );
    // A literal number for an instance the edit adds is not a reference to it: only a
    // provisional reference is.
    assert_eq!(
        refused(&|e| {
            e.add(simple("X", vec![Attribute::EntityRef(11)]));
        }),
        P21Error::Dangling {
            from: 11,
            to: "#11".into()
        }
    );
    // Missing instances, and instances edited twice.
    assert_eq!(
        refused(&|e| e.replace(4, simple("X", vec![]))),
        P21Error::Missing(4)
    );
    assert_eq!(refused(&|e| e.remove(42)), P21Error::Missing(42));
    assert_eq!(
        refused(&|e| {
            e.remove(10);
            e.remove(10);
        }),
        P21Error::EditedTwice(Some(10))
    );
    assert_eq!(
        refused(&|e| {
            e.replace(10, simple("X", vec![]));
            e.remove(10);
        }),
        P21Error::EditedTwice(Some(10))
    );
    assert_eq!(
        refused(&|e| {
            e.set_file_schema(vec!["A".into()]);
            e.set_file_schema(vec!["B".into()]);
        }),
        P21Error::EditedTwice(None)
    );
    // An unsorted raw complex record is refused, not reordered.
    let unsorted = RawEntity::Complex {
        id: 0,
        parts: vec![
            leaf("NAMED_UNIT", vec![Attribute::Derived]),
            leaf("LENGTH_UNIT", vec![]),
        ],
        span: simple("X", vec![]).span(),
    };
    assert!(matches!(
        refused(&|e| e.replace(8, unsorted.clone())),
        P21Error::Encode(_)
    ));
}

#[test]
fn edits_on_a_nist_file_keep_every_other_byte() {
    let Some(path) = nist_file("nist_ctc_01_asme1_ap242-e1.stp") else {
        return;
    };
    let bytes = read(&path);
    let doc = Document::parse(bytes.clone()).unwrap();
    // A point to replace, an unreferenced instance alone on its line to remove.
    let replaced = doc
        .ids()
        .find(|&id| matches!(doc.get(id), Some(RawEntity::Simple { name, .. }) if name == "CARTESIAN_POINT"))
        .unwrap();
    let removed = doc
        .ids()
        .filter(|&id| doc.referrers(id).is_empty() && id != replaced)
        .last()
        .unwrap();
    let mut edit = Edit::new();
    edit.replace(replaced, point("moved", [1.0, 2.0, 3.0]));
    edit.remove(removed);
    let a = edit.add(point("added", [0.1, 0.2, 0.3]));
    let b = edit.add(simple(
        "GEOMETRIC_SET",
        vec![
            Attribute::String("set".into()),
            Attribute::List(vec![a.to_ref(), Attribute::EntityRef(replaced)]),
        ],
    ));
    let out = doc.apply(&edit).unwrap();
    assert_eq!(doc.apply(&edit).unwrap(), out, "deterministic");
    let max = doc.max_id();
    assert_eq!(out.ids, BTreeMap::from([(a, max + 1), (b, max + 2)]));

    let after = Document::parse(out.bytes.clone()).unwrap();
    // Every untouched instance keeps its text, and its record.
    let touched = BTreeSet::from([replaced, removed]);
    for id in doc.ids().filter(|id| !touched.contains(id)) {
        assert_eq!(
            &after.bytes()[after.span(id).unwrap()],
            &bytes[doc.span(id).unwrap()],
            "#{id}"
        );
        assert_eq!(after.get(id).map(unspanned), doc.get(id).map(unspanned));
    }
    assert!(after.get(removed).is_none());
    assert_eq!(
        after.get(replaced).map(unspanned),
        Some(with_id(&point("moved", [1.0, 2.0, 3.0]), replaced))
    );
    // The bytes between instances (comments, line breaks) are the original's, except the
    // removed instance's line; the additions are one CRLF line each before the last ENDSEC.
    let order = |d: &Document| {
        let mut ids: Vec<(usize, u64)> =
            d.ids().map(|id| (d.span(id).unwrap().start, id)).collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
    };
    let (before_ids, after_ids) = (order(&doc), order(&after));
    let gap = |d: &Document, x: u64, y: u64| {
        d.bytes()[d.span(x).unwrap().end..d.span(y).unwrap().start].to_vec()
    };
    for w in before_ids.windows(2) {
        if !w.contains(&removed) {
            assert_eq!(
                gap(&doc, w[0], w[1]),
                gap(&after, w[0], w[1]),
                "#{} #{}",
                w[0],
                w[1]
            );
        }
    }
    let first = before_ids[0];
    assert_eq!(
        &bytes[..doc.span(first).unwrap().start],
        &after.bytes()[..after.span(first).unwrap().start]
    );
    assert_eq!(after_ids[after_ids.len() - 2..], [max + 1, max + 2]);
    let text = |d: &Document, id: u64| {
        String::from_utf8_lossy(&d.bytes()[d.span(id).unwrap()]).to_string()
    };
    assert!(!String::from_utf8_lossy(after.bytes()).contains(&text(&doc, removed)));
    assert_eq!(
        text(&after, max + 1),
        format!("#{}=CARTESIAN_POINT('added',(0.1,0.2,0.3));", max + 1)
    );
    assert_eq!(
        text(&after, max + 2),
        format!(
            "#{}=GEOMETRIC_SET('set',(#{},#{replaced}));",
            max + 2,
            max + 1
        )
    );
    assert_eq!(gap(&after, max + 1, max + 2), b"\r\n");
    let endsec = bytes.windows(9).rposition(|w| w == b"\r\nENDSEC;").unwrap() + 2;
    let start = after.span(max + 1).unwrap().start;
    assert_eq!(&after.bytes()[start - 2..start], b"\r\n");
    assert_eq!(
        &after.bytes()[after.span(max + 2).unwrap().end..],
        [b"\r\n".as_slice(), &bytes[endsec..]].concat()
    );
}

/// `text` as the only attribute of a record, read back by step-io.
fn read_back(text: &str) -> Attribute {
    let g = step_io::parser::parse(file(&format!("#1=X({text});\n")).as_slice()).unwrap();
    match &g.entities[&1] {
        RawEntity::Simple { attributes, .. } => attributes[0].clone(),
        RawEntity::Complex { .. } => unreachable!(),
    }
}

#[test]
fn duplicate_id_columns_ignore_quoted_instance_text() {
    let prefix = "#1=X('#1=literal');   ";
    let bytes = file(&format!("{prefix}#1=Y();\n"));
    let line = HEAD.lines().count() + 1;
    let column = prefix.len() + 1;
    let error = Document::parse(bytes).unwrap_err().to_string();
    assert!(
        error.ends_with(&format!(
            "parse error at line {line}, column {column}: duplicate entity #1"
        )),
        "{error}"
    );
}

#[test]
fn reals_are_the_shortest_part_21_text_that_reads_back() {
    for (v, text) in [
        (0.1, "0.1"),
        (1e-7, "1.E-07"),
        (1e300, "1.E+300"),
        (-0.0, "-0."),
        (0.0, "0."),
        (123456789.0, "123456789."),
        (5e-324, "5.E-324"),
        (-2.5e300, "-2.5E+300"),
        (25.4, "25.4"),
        (0.0001, "0.0001"),
        (1e15, "1000000000000000."),
        (1e16, "1.E+16"),
        (f64::MAX, "1.7976931348623157E+308"),
        (std::f64::consts::PI, "3.141592653589793"),
    ] {
        assert_eq!(encode_real(v).unwrap(), text);
        match read_back(text) {
            Attribute::Real(r) => assert_eq!(r.to_bits(), v.to_bits(), "{text}"),
            other => panic!("{text} read back as {other:?}"),
        }
    }
    assert!(encode_real(f64::NAN).is_err());
    assert!(encode_real(f64::INFINITY).is_err());
    // Stated decimals are written with their own digits.
    for t in ["0.0030", "1.E-07", "-2.5E+300", "+1.", "20.000"] {
        assert_eq!(encode_real_text(t).unwrap(), t);
    }
    for t in [
        "3", ".5", "1e-7", "1.e5", "1.E", "1.E+", "0.1 ", "1.E999", "", "-",
    ] {
        assert!(encode_real_text(t).is_err(), "{t:?}");
    }
    let measure = || {
        simple(
            "MEASURES",
            vec![Attribute::List(vec![
                Attribute::Real(0.003),
                Attribute::Typed {
                    type_name: "LENGTH_MEASURE".into(),
                    value: Box::new(Attribute::Real(1e-7)),
                },
                Attribute::Real(0.003),
                Attribute::Real(0.5),
            ])],
        )
    };
    let stated = Record::new(measure())
        .state("0.0030")
        .unwrap()
        .state("1.E-07")
        .unwrap();
    assert_eq!(
        stated.encode().unwrap(),
        "MEASURES((0.0030,LENGTH_MEASURE(1.E-07),0.0030,0.5))"
    );
    assert!(Record::new(measure()).state("0.0031").is_err());
    assert!(
        Record::new(measure())
            .state("3.E-03")
            .unwrap()
            .state("0.0030")
            .is_err()
    );
    assert!(Record::new(measure()).state("1e-7").is_err());
}

#[test]
fn strings_escape_and_decode() {
    let text = "it's a \\ backslash: \u{00D8}20 \u{2300}5 caf\u{00E9} \u{1F600}!";
    let body = escape(text);
    assert_eq!(
        body,
        "it's a \\\\ backslash: \\X2\\00D8\\X0\\20 \\X2\\2300\\X0\\5 caf\\X2\\00E9\\X0\\ \\X4\\0001F600\\X0\\!"
    );
    assert_eq!(decode(&body).unwrap(), text);
    let written = encode(&simple("X", vec![Attribute::String(text.into())])).unwrap();
    assert!(written.contains("it''s"), "{written}");
    match read_back(&written[2..written.len() - 1]) {
        Attribute::String(s) => {
            assert_eq!(s, text);
        }
        other => panic!("{other:?}"),
    }
    // What files carry: \X\, \S\ under the default page, \PA\, and \\.
    assert_eq!(
        decode("caf\\X\\E9 \\S\\i \\PA\\x \\\\").unwrap(),
        "café é x \\"
    );
    for bad in [
        "\\Q",
        "\\X\\G0",
        "\\X2\\00D",
        "\\X2\\00D8",
        "\\X4\\0001F6\\X0\\",
        "\\PB\\",
        "\\S\\",
    ] {
        assert!(decode(bad).is_err(), "{bad}");
    }
    // Line breaks inside a string, even inside an escape, are print control, not text.
    assert_eq!(
        decode("ab\r\nc \\X2\\00\r\nD8\\X0\\").unwrap(),
        "abc \u{00D8}"
    );
    assert!(encode(&simple("X", vec![Attribute::String("ab\r\nc".into())])).is_ok());
    // Since step-io 0.3, attributes are decoded Unicode. Controls and strings that
    // resemble malformed escapes are data: encode must escape them and preserve them.
    for text in ["a\tb", "Ø", "\\Q", "\\PB\\x", "literal \\X2\\0041\\X0\\"] {
        let written = encode(&simple("X", vec![Attribute::String(text.into())])).unwrap();
        assert_eq!(
            read_back(&written[2..written.len() - 1]),
            Attribute::String(text.into())
        );
        assert!(written.is_ascii(), "{written}");
    }
}

#[test]
fn records_encode_and_read_back() {
    let leaves = [
        leaf(
            "SI_UNIT",
            vec![
                Attribute::Enum("MILLI".into()),
                Attribute::Enum("METRE".into()),
            ],
        ),
        leaf("LENGTH_UNIT", vec![]),
        leaf("NAMED_UNIT", vec![Attribute::Derived]),
    ];
    let want = "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))";
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let c = Complex::new(order.map(|i| leaves[i].clone())).unwrap();
        assert_eq!(Record::from(c.clone()).encode().unwrap(), want);
        assert_eq!(encode(&c.into_record()).unwrap(), want);
    }
    assert!(Complex::new([]).is_err());
    assert!(Complex::new([leaf("A", vec![]), leaf("A", vec![])]).is_err());
    let g = step_io::parser::parse(file(&format!("#1={want};\n")).as_slice()).unwrap();
    let RawEntity::Complex { parts, .. } = &g.entities[&1] else {
        panic!()
    };
    let names: Vec<&str> = parts
        .iter()
        .map(|p: &RawEntityPart| p.name.as_str())
        .collect();
    assert_eq!(names, ["LENGTH_UNIT", "NAMED_UNIT", "SI_UNIT"]);
    // A raw complex record out of order is refused.
    let unsorted = RawEntity::Complex {
        id: 0,
        parts: leaves.to_vec(),
        span: simple("X", vec![]).span(),
    };
    assert!(matches!(encode(&unsorted), Err(P21Error::Encode(_))));

    // Every kind of parameter, nested.
    let record = simple(
        "B_SPLINE_THING",
        vec![
            Attribute::Integer(-3),
            Attribute::List(vec![
                Attribute::List(vec![Attribute::Real(0.0), Attribute::Real(1.5)]),
                Attribute::List(vec![]),
                Attribute::List(vec![Attribute::List(vec![Attribute::EntityRef(7)])]),
            ]),
            Attribute::Typed {
                type_name: "LENGTH_MEASURE".into(),
                value: Box::new(Attribute::Real(2.5)),
            },
            Attribute::Typed {
                type_name: "DESCRIPTIVE_MEASURE".into(),
                value: Box::new(Attribute::String("x".into())),
            },
            Attribute::Unset,
            Attribute::Derived,
            Attribute::Enum("T".into()),
            Attribute::Binary("0FF".into()),
            Attribute::String(String::new()),
        ],
    );
    let text = encode(&record).unwrap();
    assert_eq!(
        text,
        "B_SPLINE_THING(-3,((0.,1.5),(),((#7))),LENGTH_MEASURE(2.5),\
         DESCRIPTIVE_MEASURE('x'),$,*,.T.,\"0FF\",'')"
    );
    let g = step_io::parser::parse(file(&format!("#1={text};\n#7=Y();\n")).as_slice()).unwrap();
    assert_eq!(unspanned(&g.entities[&1]), with_id(&record, 1));
    // Names that are not Part 21 keywords, bad enumerations and binaries are refused.
    for bad in [
        simple("lower", vec![]),
        simple("1X", vec![]),
        simple("X", vec![Attribute::Enum("A.B".into())]),
        simple("X", vec![Attribute::Binary("4F".into())]),
        simple(
            "X",
            vec![Attribute::Typed {
                type_name: "a b".into(),
                value: Box::new(Attribute::Unset),
            }],
        ),
    ] {
        assert!(encode(&bad).is_err(), "{bad:?}");
    }
}

#[test]
fn additions_use_the_files_line_ending() {
    let add = |bytes: Vec<u8>| {
        let doc = Document::parse(bytes).unwrap();
        let mut e = Edit::new();
        e.add(point("a", [0.0; 3]));
        e.add(point("b", [1.0; 3]));
        (
            doc.newline(),
            String::from_utf8(doc.apply(&e).unwrap().bytes).unwrap(),
        )
    };
    let crlf = String::from_utf8(file("#1=X();\n"))
        .unwrap()
        .replace('\n', "\r\n");
    let (nl, out) = add(crlf.clone().into_bytes());
    assert_eq!(nl, "\r\n");
    assert_eq!(
        out,
        crlf.replace(
            "#1=X();\r\nENDSEC;\r\nEND",
            "#1=X();\r\n#2=CARTESIAN_POINT('a',(0.,0.,0.));\r\n\
             #3=CARTESIAN_POINT('b',(1.,1.,1.));\r\nENDSEC;\r\nEND"
        )
    );
    // Mixed: the line ending before the ENDSEC.
    let mixed = String::from_utf8(file("#1=X();\r\n")).unwrap();
    let (nl, out) = add(mixed.clone().into_bytes());
    assert_eq!(nl, "\r\n");
    assert!(out.contains("#1=X();\r\n#2=CARTESIAN_POINT('a',(0.,0.,0.));\r\n#3="));
    // ENDSEC sharing the last instance's line: the additions start on a line of their own.
    let same_line = String::from_utf8(file("#1=X();")).unwrap();
    let (_, out) = add(same_line.into_bytes());
    assert!(out.contains("#1=X();\n#2=CARTESIAN_POINT('a',(0.,0.,0.));\n#3=CARTESIAN_POINT('b',(1.,1.,1.));\nENDSEC;"), "{out}");
}

#[test]
fn latin_1_files_keep_their_bytes_and_extents() {
    let mut bytes = file("#1=X('caf\u{00E9}');\n#2=Y(#1);\n");
    // Re-encode é as the single Latin-1 byte 0xE9, so the file is not UTF-8.
    let at = bytes.windows(2).position(|w| w == [0xC3, 0xA9]).unwrap();
    bytes.splice(at..at + 2, [0xE9]);
    assert!(std::str::from_utf8(&bytes).is_err());
    let doc = Document::parse(bytes.clone()).unwrap();
    assert_eq!(&bytes[doc.span(1).unwrap()], b"#1=X('caf\xE9');");
    assert_eq!(&bytes[doc.span(2).unwrap()], b"#2=Y(#1);");
    assert_eq!(doc.apply(&Edit::new()).unwrap().bytes, bytes);
    let mut e = Edit::new();
    e.replace(
        2,
        simple("Y", vec![Attribute::EntityRef(1), Attribute::Integer(2)]),
    );
    let out = doc.apply(&e).unwrap().bytes;
    let want: &[u8] = b"#1=X('caf\xE9');\n#2=Y(#1,2);\n";
    assert!(out.windows(want.len()).any(|w| w == want));
}

#[test]
fn files_step_io_cannot_read_are_refused() {
    // Several DATA sections (edition 3): the extents are found, but step-io reads one section.
    let two = format!("{HEAD}#1=X();\nENDSEC;\nDATA;\n#2=Y();\n{TAIL}");
    assert!(matches!(
        Document::parse(two.into_bytes()),
        Err(P21Error::Parse(_))
    ));
    assert!(matches!(
        Document::parse(file("#1=X('unterminated);\n")),
        Err(P21Error::Parse(_))
    ));
}

#[test]
fn removals_keep_their_neighbours() {
    let removed = |data: &str, ids: &[u64]| {
        let doc = Document::parse(file(data)).unwrap();
        let mut e = Edit::new();
        for &id in ids {
            e.remove(id);
        }
        let out = String::from_utf8(doc.apply(&e).unwrap().bytes).unwrap();
        let body = out.strip_prefix(HEAD).unwrap().strip_suffix(TAIL);
        body.map_or_else(|| panic!("{out}"), str::to_owned)
    };
    // Two instances sharing a line: the line goes once both are gone, else only the span.
    assert_eq!(removed("#1=X();#2=Y();\n#3=Z();\n", &[1, 2]), "#3=Z();\n");
    assert_eq!(removed("#1=X(); #2=Y();\n#3=Z();\n", &[2, 1]), "#3=Z();\n");
    assert_eq!(
        removed("#1=X(); #2=Y();\n#3=Z();\n", &[1]),
        " #2=Y();\n#3=Z();\n"
    );
    // A trailing comment keeps its line.
    assert_eq!(
        removed("#1=X(); /* one */\n#2=Y();\n", &[1]),
        " /* one */\n#2=Y();\n"
    );
    // A removed instance spread over several lines takes them all.
    assert_eq!(removed("#1=X(\n  1);\n#2=Y();\n", &[1]), "#2=Y();\n");
}

#[test]
fn removing_the_instance_on_the_endsec_line_keeps_the_endsec() {
    let doc = Document::parse(file("#1=X();\n#2=Y(); ")).unwrap();
    let mut e = Edit::new();
    e.remove(2);
    let out = String::from_utf8(doc.apply(&e).unwrap().bytes).unwrap();
    assert_eq!(out, String::from_utf8(file("#1=X();\n ")).unwrap());
    assert!(Document::parse(out.into_bytes()).is_ok());
}

/// An edition 3 file: an ANCHOR naming #1, and #50 an external REFERENCE.
const ED3: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\n\
                   FILE_NAME('t','',(''),(''),'','','');\nFILE_SCHEMA(('T'));\nENDSEC;\n\
                   ANCHOR;\n<A1>=#1;\nENDSEC;\n\
                   REFERENCE;\n#50=<other.stp#X; y>;\nENDSEC;\n\
                   DATA;\n#1=X(#50);\n#2=Y(#1);\n#3=Z();\nENDSEC;\nEND-ISO-10303-21;\n";

#[test]
fn anchor_and_reference_sections_are_kept() {
    let doc = Document::parse(ED3.as_bytes().to_vec()).unwrap();
    assert_eq!(doc.ids().collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(doc.max_id(), 50, "the external reference #50 counts");
    assert_eq!(doc.referrers(50), [1]);
    assert_eq!(doc.apply(&Edit::new()).unwrap().bytes, ED3.as_bytes());
    // An anchored instance cannot be removed, even with its referrer.
    let mut e = Edit::new();
    e.remove(2);
    e.remove(1);
    assert_eq!(
        doc.apply(&e).unwrap_err(),
        P21Error::Anchored {
            id: 1,
            name: "<A1>".into()
        }
    );
    // An external reference is a valid target, and additions are numbered past it.
    let mut e = Edit::new();
    let a = e.add(simple("W", vec![Attribute::EntityRef(50)]));
    let out = doc.apply(&e).unwrap();
    assert_eq!(out.ids, BTreeMap::from([(a, 51)]));
    assert_eq!(
        String::from_utf8(out.bytes).unwrap(),
        ED3.replace("#3=Z();\n", "#3=Z();\n#51=W(#50);\n")
    );
}

#[test]
fn a_malformed_file_schema_is_refused() {
    let doc = |schema: &str| {
        let text = String::from_utf8(file("#1=X();\n")).unwrap();
        Document::parse(text.replace("FILE_SCHEMA(('T'));", schema).into_bytes())
    };
    assert_eq!(
        doc("FILE_SCHEMA(('A','B'));")
            .unwrap()
            .file_schema()
            .unwrap(),
        ["A", "B"]
    );
    // step-io itself refuses a FILE_SCHEMA that is not a list of strings.
    assert!(matches!(
        doc("FILE_SCHEMA(('A',$));"),
        Err(P21Error::Parse(_))
    ));
}

#[test]
fn real_occurrences_keep_distinct_stated_tokens_and_validate_bindings() {
    let record = Record::new(simple(
        "CARTESIAN_POINT",
        vec![
            Attribute::String(String::new()),
            Attribute::List(vec![
                Attribute::Real(0.),
                Attribute::Real(0.),
                Attribute::Real(1.),
            ]),
        ],
    ));
    let stated = record
        .clone()
        .state_reals(&["0.0", "0.E+0", "1.00"])
        .unwrap();
    assert_eq!(
        stated.encode().unwrap(),
        "CARTESIAN_POINT('',(0.0,0.E+0,1.00))"
    );
    assert!(record.clone().state_reals(&["0.0"]).is_err());
    assert!(record.clone().state_reals(&["0.0", "2.0", "1.00"]).is_err());
    assert!(record.state_reals(&["0.0", "0.0", "1e0"]).is_err());
}
