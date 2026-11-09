//! Bounded-chunk statement lexer shared by the lossless and manifest readers.
//!
//! Only the current statement is buffered. The callback borrows its text; the scanner retains
//! no entity text or index. Chunk boundaries may occur anywhere, including doubled quotes,
//! comment delimiters and instance numbers.
use super::{P21Error, Section, Statement};
use std::io::{BufRead, BufReader, Read};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub id: u64,
    pub offset: u64,
    pub length: u32,
    /// End of the preceding statement; the intervening bytes are whitespace/comments only.
    pub gap_start: u64,
    pub types: Vec<String>,
    pub references: Vec<u64>,
}

pub enum Event<'a> {
    Instance(Instance, &'a [u8]),
    Header(&'a str, u64, &'a [u8]),
    HeaderEnd(u64),
    Data(u64, u64),
}

struct Input<R> {
    reader: BufReader<R>,
    at: u64,
    line: u64,
}
impl<R: Read> Input<R> {
    fn error(&self, message: &str) -> P21Error {
        P21Error::Parse(format!("line {}: {message}", self.line))
    }
    fn peek(&mut self) -> Result<Option<u8>, P21Error> {
        self.reader
            .fill_buf()
            .map(|b| b.first().copied())
            .map_err(|e| self.error(&e.to_string()))
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
    fn comment(&mut self, mut text: Option<&mut Vec<u8>>) -> Result<(), P21Error> {
        let mut star = false;
        loop {
            let b = self
                .next()?
                .ok_or_else(|| self.error("unterminated comment"))?;
            if let Some(t) = &mut text {
                t.push(b);
            }
            if star && b == b'/' {
                return Ok(());
            }
            star = b == b'*';
        }
    }
}

/// Scan `reader`, requesting at most `chunk_size` bytes per buffer refill. Zero is refused.
/// Refusal rules for sections and statement termination are also those of `p21::Document`.
pub fn scan<R: Read>(
    reader: R,
    chunk_size: usize,
    mut visit: impl FnMut(Event<'_>) -> Result<(), P21Error>,
) -> Result<(), P21Error> {
    if chunk_size == 0 {
        return Err(P21Error::Parse("zero scanner chunk size".into()));
    }
    let mut input = Input {
        reader: BufReader::with_capacity(chunk_size, reader),
        at: 0,
        line: 1,
    };
    let mut section = Section::Start;
    let mut data_start = 0;
    let mut previous_end = 0;
    loop {
        let start;
        let first;
        loop {
            let at = input.at;
            let Some(b) = input.next()? else {
                return if section == Section::End {
                    Ok(())
                } else {
                    Err(input.error("missing END-ISO-10303-21;"))
                };
            };
            if b.is_ascii_whitespace() {
                continue;
            }
            if b == b'/' && input.peek()? == Some(b'*') {
                input.next()?;
                input.comment(None)?;
                continue;
            }
            start = at;
            first = b;
            break;
        }
        if section == Section::End {
            return Err(input.error("text after END-ISO-10303-21"));
        }
        let mut text = vec![first];
        // An ANCHOR statement starts with '<'; include that delimiter in the resumable
        // literal handling, just as for an anchor encountered later in a statement.
        if first == b'<' && section == Section::Ed3 {
            loop {
                let c = input
                    .next()?
                    .ok_or_else(|| input.error("unterminated anchor"))?;
                text.push(c);
                if c == b'>' {
                    break;
                }
            }
        }
        loop {
            let b = input
                .next()?
                .ok_or_else(|| input.error("statement without its terminating ';'"))?;
            text.push(b);
            match b {
                b';' => break,
                b'\'' | b'"' => loop {
                    let c = input
                        .next()?
                        .ok_or_else(|| input.error("statement without its terminating ';'"))?;
                    text.push(c);
                    if c == b {
                        if b == b'\'' && input.peek()? == Some(b'\'') {
                            text.push(b'\'');
                            input.next()?;
                        } else {
                            break;
                        }
                    }
                },
                b'<' if section == Section::Ed3 => loop {
                    let c = input
                        .next()?
                        .ok_or_else(|| input.error("unterminated anchor"))?;
                    text.push(c);
                    if c == b'>' {
                        break;
                    }
                },
                b'/' if input.peek()? == Some(b'*') => {
                    text.push(b'*');
                    input.next()?;
                    input.comment(Some(&mut text))?;
                }
                _ => {}
            }
            if text.len() > u32::MAX as usize {
                return Err(input.error("statement exceeds u32 byte length"));
            }
        }
        let mut pos = 0;
        let first = if text[0] == b'#' {
            pos = 1;
            while text[pos].is_ascii_digit() {
                pos += 1;
            }
            let id = std::str::from_utf8(&text[1..pos])
                .ok()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| input.error("malformed instance id"))?;
            Statement::Instance(id)
        } else if text[0] == b'<' && section == Section::Ed3 {
            Statement::Anchor
        } else {
            while text[pos].is_ascii_alphanumeric() || b"_-".contains(&text[pos]) {
                pos += 1;
            }
            if pos == 0 {
                return Err(input.error("expected a keyword or #N"));
            }
            Statement::Word(String::from_utf8_lossy(&text[..pos]).to_ascii_uppercase())
        };
        let word = match &first {
            Statement::Word(w) => w.as_str(),
            _ => "",
        };
        match (section, &first) {
            (Section::Start, _) if word == "ISO-10303-21" => section = Section::Top,
            (Section::Start, _) => return Err(input.error("expected ISO-10303-21;")),
            (Section::Top, _) => match word {
                "HEADER" => section = Section::Header,
                "DATA" => {
                    section = Section::Data;
                    data_start = start;
                }
                "ANCHOR" | "REFERENCE" => section = Section::Ed3,
                "END-ISO-10303-21" => section = Section::End,
                "SIGNATURE" => return Err(input.error("SIGNATURE sections are not supported")),
                _ => return Err(input.error("expected a section")),
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
                let (types, references) = structure(&text[pos..])?;
                visit(Event::Instance(
                    Instance {
                        id: *id,
                        offset: start,
                        length: text.len() as u32,
                        gap_start: previous_end,
                        types,
                        references,
                    },
                    &text,
                ))?;
            }
            (Section::Ed3, _) => {}
            _ => return Err(input.error("unexpected statement")),
        }
        previous_end = input.at;
    }
}

/// Tokens outside literals/comments, with type names only at the external mapping's depth.
fn structure(b: &[u8]) -> Result<(Vec<String>, Vec<u64>), P21Error> {
    let mut types = Vec::new();
    let mut references = Vec::new();
    let mut pos = super::skip_blank(b, 0).ok_or_else(|| P21Error::Parse("comment".into()))?;
    if b.get(pos) != Some(&b'=') {
        return Err(P21Error::Parse("instance without '='".into()));
    }
    pos = super::skip_blank(b, pos + 1).ok_or_else(|| P21Error::Parse("comment".into()))?;
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
                pos = super::comment_end(b, pos + 2)
                    .ok_or_else(|| P21Error::Parse("comment".into()))?
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
                let id = std::str::from_utf8(&b[start..pos])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| P21Error::Parse("malformed reference".into()))?;
                references.push(id);
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
                    types.push(String::from_utf8_lossy(&b[start..pos]).to_ascii_uppercase());
                }
            }
            _ => pos += 1,
        }
    }
    Ok((types, references))
}
