//! Resumable Part 21 statement lexer. `scan` captures extents only; `scan_structure` also
//! captures borrowed type keywords and references. One statement and reusable token scratch
//! are buffered, with no allocation for each ordinary instance.
use super::{P21Error, Section, Statement};
use std::io::{BufRead, BufReader, Read};
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
pub struct Types<'a> {
    bytes: &'a [u8],
    spans: &'a [Range<usize>],
}
impl Types<'_> {
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.spans.iter().map(|r| {
            // Keyword spans contain ASCII letters, digits and underscores only.
            std::str::from_utf8(&self.bytes[r.clone()]).unwrap_or_default()
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Instance<'a> {
    pub id: u64,
    pub offset: u64,
    pub length: u32,
    pub gap_start: u64,
    pub types: Types<'a>,
    pub references: &'a [u64],
}
pub enum Event<'a> {
    Instance(Instance<'a>, &'a [u8]),
    Header(&'a str, u64, &'a [u8]),
    HeaderEnd(u64),
    Data(u64, u64),
}
fn error(line: u64, message: &str) -> P21Error {
    P21Error::Parse(format!("line {line}: {message}"))
}
struct Input<R> {
    reader: BufReader<R>,
    at: u64,
    line: u64,
}
impl<R: Read> Input<R> {
    fn peek(&mut self) -> Result<Option<u8>, P21Error> {
        self.reader
            .fill_buf()
            .map(|b| b.first().copied())
            .map_err(|e| error(self.line, &e.to_string()))
    }
    fn next(&mut self) -> Result<Option<u8>, P21Error> {
        let b = self.peek()?;
        if let Some(b) = b {
            self.reader.consume(1);
            self.at += 1;
            self.line += u64::from(b == b'\n');
        }
        Ok(b)
    }
    fn comment(
        &mut self,
        mut text: Option<&mut Vec<u8>>,
        line: u64,
        message: &str,
    ) -> Result<(), P21Error> {
        let mut star = false;
        loop {
            let b = self.next()?.ok_or_else(|| error(line, message))?;
            if let Some(text) = &mut text {
                text.push(b);
            }
            if star && b == b'/' {
                return Ok(());
            }
            star = b == b'*';
        }
    }
}
/// Capture extents without parsing instance parameters (including without requiring `=`).
pub fn scan<R: Read>(
    reader: R,
    chunk_size: usize,
    visit: impl FnMut(Event<'_>) -> Result<(), P21Error>,
) -> Result<(), P21Error> {
    scan_inner(reader, chunk_size, false, visit)
}
/// Capture types and references outside strings, binaries and comments. Borrowed tokens are
/// valid only during `visit`; the next statement reuses their buffers.
pub fn scan_structure<R: Read>(
    reader: R,
    chunk_size: usize,
    visit: impl FnMut(Event<'_>) -> Result<(), P21Error>,
) -> Result<(), P21Error> {
    scan_inner(reader, chunk_size, true, visit)
}
fn scan_inner<R: Read>(
    reader: R,
    chunk_size: usize,
    capture: bool,
    mut visit: impl FnMut(Event<'_>) -> Result<(), P21Error>,
) -> Result<(), P21Error> {
    if chunk_size == 0 {
        return Err(error(1, "zero scanner chunk size"));
    }
    let mut input = Input {
        reader: BufReader::with_capacity(chunk_size, reader),
        at: 0,
        line: 1,
    };
    let mut section = Section::Start;
    let mut data_start = 0;
    let mut previous_end = 0;
    let mut text = Vec::with_capacity(1024);
    let mut type_spans = Vec::new();
    let mut refs = Vec::new();
    loop {
        text.clear();
        let blank_line = input.line;
        let (start, first) = loop {
            let at = input.at;
            let Some(b) = input.next()? else {
                return if section == Section::End {
                    Ok(())
                } else {
                    Err(error(input.line, "missing END-ISO-10303-21;"))
                };
            };
            if b.is_ascii_whitespace() {
                continue;
            }
            if b == b'/' && input.peek()? == Some(b'*') {
                input.next()?;
                input.comment(None, blank_line, "unterminated comment")?;
                continue;
            }
            break (at, b);
        };
        let line = input.line;
        if section == Section::End {
            return Err(error(line, "text after END-ISO-10303-21"));
        }
        text.push(first);
        let first = if first == b'#' {
            while input.peek()?.is_some_and(|b| b.is_ascii_digit()) {
                if let Some(b) = input.next()? {
                    text.push(b);
                }
            }
            let id = std::str::from_utf8(&text[1..])
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| error(line, "malformed instance id"))?;
            Statement::Instance(id)
        } else if first == b'<' && section == Section::Ed3 {
            Statement::Anchor
        } else {
            if !first.is_ascii_alphanumeric() && !b"_-".contains(&first) {
                return Err(error(line, "expected a keyword or #N"));
            }
            while input
                .peek()?
                .is_some_and(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                if let Some(b) = input.next()? {
                    text.push(b);
                }
            }
            Statement::Word(String::from_utf8_lossy(&text).to_ascii_uppercase())
        };
        let parameters = text.len();
        let unfinished = "statement without its terminating ';'";
        if matches!(first, Statement::Anchor) {
            loop {
                let b = input.next()?.ok_or_else(|| error(line, unfinished))?;
                text.push(b);
                if b == b'>' {
                    break;
                }
            }
        }
        loop {
            let b = input.next()?.ok_or_else(|| error(line, unfinished))?;
            text.push(b);
            match b {
                b';' => break,
                b'\'' | b'"' => loop {
                    let c = input.next()?.ok_or_else(|| error(line, unfinished))?;
                    text.push(c);
                    if c == b {
                        if b == b'\'' && input.peek()? == Some(b'\'') {
                            input.next()?;
                            text.push(b'\'');
                        } else {
                            break;
                        }
                    }
                },
                b'<' if section == Section::Ed3 => loop {
                    let c = input.next()?.ok_or_else(|| error(line, unfinished))?;
                    text.push(c);
                    if c == b'>' {
                        break;
                    }
                },
                b'/' if input.peek()? == Some(b'*') => {
                    input.next()?;
                    text.push(b'*');
                    input.comment(Some(&mut text), line, unfinished)?;
                }
                _ => {}
            }
            if text.len() > u32::MAX as usize {
                return Err(error(line, "statement exceeds u32 byte length"));
            }
        }
        let word = match &first {
            Statement::Word(w) => w.as_str(),
            _ => "",
        };
        match (section, &first) {
            (Section::Start, _) if word == "ISO-10303-21" => section = Section::Top,
            (Section::Start, _) => return Err(error(line, "expected ISO-10303-21;")),
            (Section::Top, _) => match word {
                "HEADER" => section = Section::Header,
                "DATA" => {
                    section = Section::Data;
                    data_start = start;
                }
                "ANCHOR" | "REFERENCE" => section = Section::Ed3,
                "END-ISO-10303-21" => section = Section::End,
                "SIGNATURE" => return Err(error(line, "SIGNATURE sections are not supported")),
                _ => return Err(error(line, "expected a section")),
            },
            (Section::Header | Section::Data | Section::Ed3, _) if word == "ENDSEC" => {
                if section == Section::Data {
                    visit(Event::Data(data_start, start))?;
                }
                if section == Section::Header {
                    visit(Event::HeaderEnd(input.at))?;
                }
                section = Section::Top;
            }
            (Section::Header, Statement::Word(w)) => visit(Event::Header(w, start, &text))?,
            (Section::Data, Statement::Instance(id)) => {
                type_spans.clear();
                refs.clear();
                if capture {
                    structure(&text, parameters, &mut type_spans, &mut refs)
                        .map_err(|e| error(line, &e.to_string()))?;
                }
                visit(Event::Instance(
                    Instance {
                        id: *id,
                        offset: start,
                        length: text.len() as u32,
                        gap_start: previous_end,
                        types: Types {
                            bytes: &text,
                            spans: &type_spans,
                        },
                        references: &refs,
                    },
                    &text,
                ))?;
            }
            (Section::Ed3, _) => {}
            _ => return Err(error(line, "unexpected statement")),
        }
        previous_end = input.at;
    }
}
fn structure(
    b: &[u8],
    from: usize,
    types: &mut Vec<Range<usize>>,
    refs: &mut Vec<u64>,
) -> Result<(), P21Error> {
    let mut pos = super::skip_blank(b, from).ok_or_else(|| error(1, "comment"))?;
    if b.get(pos) != Some(&b'=') {
        return Err(P21Error::Parse("instance without '='".into()));
    }
    pos = super::skip_blank(b, pos + 1).ok_or_else(|| error(1, "comment"))?;
    let complex = b.get(pos) == Some(&b'(');
    let mut depth = 0usize;
    while pos < b.len() {
        match b[pos] {
            b'\'' | b'"' => {
                let quote = b[pos];
                pos += 1;
                while pos < b.len() {
                    if b[pos] == quote {
                        pos += 1;
                        if quote == b'\'' && b.get(pos) == Some(&quote) {
                            pos += 1;
                        } else {
                            break;
                        }
                    } else {
                        pos += 1;
                    }
                }
            }
            b'/' if b.get(pos + 1) == Some(&b'*') => {
                pos = super::comment_end(b, pos + 2).ok_or_else(|| error(1, "comment"))?
            }
            b'(' => {
                depth += 1;
                pos += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                pos += 1;
            }
            b'#' => {
                pos += 1;
                let start = pos;
                while b.get(pos).is_some_and(u8::is_ascii_digit) {
                    pos += 1;
                }
                refs.push(
                    std::str::from_utf8(&b[start..pos])
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(|| P21Error::Parse("malformed reference".into()))?,
                );
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = pos;
                pos += 1;
                while b
                    .get(pos)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                {
                    pos += 1;
                }
                if depth == usize::from(complex) {
                    types.push(start..pos);
                }
            }
            _ => pos += 1,
        }
    }
    Ok(())
}
