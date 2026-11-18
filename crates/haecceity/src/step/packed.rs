//! Packed DATA records for geometry provenance. Only the private writer can create
//! the byte blocks and offsets read below. Source spans, numeric bits, record shape
//! and attribute ordering survive a round trip. Names share a symbol table.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

use step_io::Error;
use step_io::parser::{Attribute, RawEntity, RawEntityPart, Span, Stream};

use super::RecordGraph;

const BLOCK: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub(crate) struct PackedGraph {
    pub(super) header: Vec<RawEntity>,
    records: BTreeMap<u64, usize>,
    symbols: Vec<String>,
    data: Blocks,
}

impl PackedGraph {
    /// Consume the stream through its trailer. Keep the first duplicate in file
    /// order ahead of any subsequent syntax or I/O error, as ordinary parse does.
    pub(super) fn from_stream(stream: &mut Stream<impl Read>) -> Result<Self, Error> {
        let mut graph = Self {
            header: stream.header().to_vec(),
            ..Self::default()
        };
        let mut symbol_ids = HashMap::new();
        for entity in stream {
            let entity = entity?;
            let id = entity.id();
            if let Some(offset) = graph.records.insert(id, graph.data.len) {
                let mut first = RecordReader {
                    data: &graph.data,
                    position: offset,
                    symbols: &[],
                };
                return Err(Error::DuplicateEntityId {
                    id,
                    line: entity.span().line,
                    first_line: first.span().line,
                });
            }
            let mut writer = RecordWriter {
                data: &mut graph.data,
                symbol_ids: &mut symbol_ids,
            };
            writer.span(entity.span());
            match entity {
                RawEntity::Simple {
                    name, attributes, ..
                } => {
                    writer.byte(0);
                    writer.symbol(&name);
                    writer.attributes(&attributes);
                }
                RawEntity::Complex { parts, .. } => {
                    writer.byte(1);
                    writer.length(parts.len());
                    for part in parts {
                        writer.symbol(&part.name);
                        writer.attributes(&part.attributes);
                    }
                }
            }
        }
        graph.symbols.resize(symbol_ids.len(), String::new());
        for (name, index) in symbol_ids {
            graph.symbols[index] = name;
        }
        Ok(graph)
    }

    #[cfg(test)]
    pub(crate) fn parse(input: impl Read) -> Result<Self, Error> {
        Self::from_stream(&mut step_io::parser::stream(input)?)
    }

    fn reader(&self, id: u64) -> Option<RecordReader<'_>> {
        Some(RecordReader {
            data: &self.data,
            position: *self.records.get(&id)?,
            symbols: &self.symbols,
        })
    }
}

impl RecordGraph for PackedGraph {
    fn ids(&self) -> Box<dyn Iterator<Item = u64> + '_> {
        Box::new(self.records.keys().copied())
    }

    fn record(&self, id: u64) -> Option<Cow<'_, RawEntity>> {
        let mut reader = self.reader(id)?;
        let span = reader.span();
        let record = if reader.byte() == 0 {
            RawEntity::Simple {
                id,
                span,
                name: reader.symbol(),
                attributes: reader.attributes(),
            }
        } else {
            RawEntity::Complex {
                id,
                span,
                parts: (0..reader.length())
                    .map(|_| RawEntityPart {
                        name: reader.symbol(),
                        attributes: reader.attributes(),
                    })
                    .collect(),
            }
        };
        Some(Cow::Owned(record))
    }

    fn kind(&self, id: u64) -> Option<&str> {
        let mut reader = self.reader(id)?;
        reader.span();
        Some(if reader.byte() == 0 {
            reader.symbol_ref()
        } else {
            "COMPLEX"
        })
    }

    fn is_complex(&self, id: u64) -> bool {
        self.reader(id).is_some_and(|mut reader| {
            reader.span();
            reader.byte() == 1
        })
    }

    fn attribute_at(&self, id: u64, part: Option<&str>, path: &[usize]) -> Option<Attribute> {
        let mut reader = self.reader(id)?;
        reader.span();
        match (reader.byte(), part) {
            (0, None) => {
                reader.unsigned();
                reader.select(path)
            }
            (1, Some(part)) => {
                for _ in 0..reader.length() {
                    if reader.symbol_ref() == part {
                        return reader.select(path);
                    }
                    reader.skip_attributes();
                }
                None
            }
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct Blocks {
    chunks: Vec<Vec<u8>>,
    len: usize,
}

impl Blocks {
    #[allow(
        clippy::expect_used,
        reason = "the branch above last_mut ensures a writable block"
    )]
    fn write(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.chunks.last().is_none_or(|last| last.len() == BLOCK) {
                self.chunks.push(Vec::new());
            }
            let chunk = self.chunks.last_mut().expect("block allocated");
            let count = bytes.len().min(BLOCK - chunk.len());
            chunk.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            self.len += count;
        }
    }

    fn byte(&self, position: usize) -> u8 {
        self.chunks[position / BLOCK][position % BLOCK]
    }

    fn read(&self, position: &mut usize, bytes: &mut [u8]) {
        let mut out = bytes;
        while !out.is_empty() {
            let chunk = &self.chunks[*position / BLOCK];
            let start = *position % BLOCK;
            let count = out.len().min(chunk.len() - start);
            out[..count].copy_from_slice(&chunk[start..start + count]);
            *position += count;
            out = &mut out[count..];
        }
    }
}

struct RecordWriter<'a> {
    data: &'a mut Blocks,
    symbol_ids: &'a mut HashMap<String, usize>,
}

impl RecordWriter<'_> {
    fn byte(&mut self, value: u8) {
        self.data.write(&[value]);
    }

    fn unsigned(&mut self, mut value: u64) {
        while value >= 128 {
            self.byte((value as u8 & 127) | 128);
            value >>= 7;
        }
        self.byte(value as u8);
    }

    fn length(&mut self, value: usize) {
        self.unsigned(value as u64);
    }

    fn symbol(&mut self, value: &str) {
        let id = if let Some(&id) = self.symbol_ids.get(value) {
            id
        } else {
            let id = self.symbol_ids.len();
            self.symbol_ids.insert(value.to_owned(), id);
            id
        };
        self.length(id);
    }

    fn text(&mut self, value: &str) {
        self.length(value.len());
        self.data.write(value.as_bytes());
    }

    fn span(&mut self, span: Span) {
        self.unsigned(span.start);
        self.unsigned(span.end);
        self.unsigned(u64::from(span.line));
        self.unsigned(u64::from(span.column));
    }

    fn attributes(&mut self, values: &[Attribute]) {
        self.length(values.len());
        for value in values {
            self.attribute(value);
        }
    }

    fn attribute(&mut self, value: &Attribute) {
        match value {
            Attribute::Integer(value) => {
                self.byte(0);
                self.data.write(&value.to_le_bytes());
            }
            Attribute::Real(value) => {
                self.byte(1);
                self.data.write(&value.to_bits().to_le_bytes());
            }
            Attribute::String(value) => {
                self.byte(2);
                self.text(value);
            }
            Attribute::Enum(value) => {
                self.byte(3);
                self.symbol(value);
            }
            Attribute::Binary(value) => {
                self.byte(4);
                self.text(value);
            }
            Attribute::EntityRef(value) => {
                self.byte(5);
                self.unsigned(*value);
            }
            Attribute::Unset => self.byte(6),
            Attribute::Derived => self.byte(7),
            Attribute::List(values) => {
                self.byte(8);
                self.attributes(values);
            }
            Attribute::Typed { type_name, value } => {
                self.byte(9);
                self.symbol(type_name);
                self.attribute(value);
            }
        }
    }
}

struct RecordReader<'a> {
    data: &'a Blocks,
    position: usize,
    symbols: &'a [String],
}

impl<'a> RecordReader<'a> {
    fn byte(&mut self) -> u8 {
        let value = self.data.byte(self.position);
        self.position += 1;
        value
    }

    #[allow(
        clippy::unreachable,
        reason = "RecordWriter::unsigned emits at most ten bytes per u64"
    )]
    fn unsigned(&mut self) -> u64 {
        let mut value = 0;
        for shift in (0..70).step_by(7) {
            let byte = self.byte();
            value |= u64::from(byte & 127) << shift;
            if byte < 128 {
                return value;
            }
        }
        unreachable!("private varint written from u64")
    }

    #[allow(
        clippy::expect_used,
        reason = "lengths originate from usize in the same process"
    )]
    fn length(&mut self) -> usize {
        usize::try_from(self.unsigned()).expect("addressable stored length")
    }

    fn symbol(&mut self) -> String {
        self.symbol_ref().to_owned()
    }

    fn symbol_ref(&mut self) -> &'a str {
        let id = self.length();
        &self.symbols[id]
    }

    fn select(&mut self, path: &[usize]) -> Option<Attribute> {
        let (&index, rest) = path.split_first()?;
        if index >= self.length() {
            return None;
        }
        for _ in 0..index {
            self.skip_attribute();
        }
        if rest.is_empty() {
            Some(self.attribute())
        } else if self.byte() == 8 {
            self.select(rest)
        } else {
            None
        }
    }

    fn skip_attributes(&mut self) {
        for _ in 0..self.length() {
            self.skip_attribute();
        }
    }

    #[allow(
        clippy::unreachable,
        reason = "RecordWriter::attribute emits only tags 0 through 9"
    )]
    fn skip_attribute(&mut self) {
        match self.byte() {
            0 | 1 => self.position += 8,
            2 | 4 => self.position += self.length(),
            3 | 5 => {
                self.unsigned();
            }
            6 | 7 => {}
            8 => self.skip_attributes(),
            9 => {
                self.unsigned();
                self.skip_attribute();
            }
            _ => unreachable!("private attribute tag"),
        }
    }

    #[allow(
        clippy::expect_used,
        reason = "RecordWriter::text stores the bytes of a str"
    )]
    fn text(&mut self) -> String {
        let mut bytes = vec![0; self.length()];
        self.data.read(&mut self.position, &mut bytes);
        String::from_utf8(bytes).expect("stored UTF-8")
    }

    #[allow(
        clippy::expect_used,
        reason = "RecordWriter::span writes u32 line and column values"
    )]
    fn span(&mut self) -> Span {
        Span {
            start: self.unsigned(),
            end: self.unsigned(),
            line: u32::try_from(self.unsigned()).expect("stored line"),
            column: u32::try_from(self.unsigned()).expect("stored column"),
        }
    }

    fn attributes(&mut self) -> Vec<Attribute> {
        (0..self.length()).map(|_| self.attribute()).collect()
    }

    #[allow(
        clippy::unreachable,
        reason = "RecordWriter::attribute emits only tags 0 through 9"
    )]
    fn attribute(&mut self) -> Attribute {
        match self.byte() {
            0 => {
                let mut bytes = [0; 8];
                self.data.read(&mut self.position, &mut bytes);
                Attribute::Integer(i64::from_le_bytes(bytes))
            }
            1 => {
                let mut bytes = [0; 8];
                self.data.read(&mut self.position, &mut bytes);
                Attribute::Real(f64::from_bits(u64::from_le_bytes(bytes)))
            }
            2 => Attribute::String(self.text()),
            3 => Attribute::Enum(self.symbol()),
            4 => Attribute::Binary(self.text()),
            5 => Attribute::EntityRef(self.unsigned()),
            6 => Attribute::Unset,
            7 => Attribute::Derived,
            8 => Attribute::List(self.attributes()),
            9 => Attribute::Typed {
                type_name: self.symbol(),
                value: Box::new(self.attribute()),
            },
            _ => unreachable!("private attribute tag"),
        }
    }
}
