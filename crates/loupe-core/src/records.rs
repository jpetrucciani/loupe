use std::collections::{BTreeSet, VecDeque};
use std::io::{self, BufRead, Read};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::input::{InputFormat, Location, Reader, Unit};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Lines,
    Whole,
    Null,
    Paragraphs,
    Records,
    Jsonl,
    Csv,
    Tsv,
    Files,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Options {
    pub format: Format,
    pub keep_blank: bool,
    pub records: Option<String>,
    pub field: Option<String>,
    pub select: Vec<String>,
    pub with_context: usize,
}

impl Options {
    pub fn from_document(document: &Value, default_format: Format) -> Result<Self> {
        #[derive(Default, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        struct Input {
            mode: Option<Format>,
            keep_blank: bool,
            records: Option<String>,
            field: Option<String>,
            select: Vec<String>,
            with_context: usize,
        }
        let input: Input = document
            .get("input")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| Error::new(ErrorKind::Usage, format!("preset input: {error}")))?
            .unwrap_or_default();
        Ok(Self {
            format: input.mode.unwrap_or(default_format),
            keep_blank: input.keep_blank,
            records: input.records,
            field: input.field,
            select: input.select,
            with_context: input.with_context,
        })
    }

    pub fn new(format: Format) -> Self {
        Self {
            format,
            keep_blank: false,
            records: None,
            field: None,
            select: Vec::new(),
            with_context: 0,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if (self.format == Format::Records) != self.records.is_some() {
            return Err(Error::new(
                ErrorKind::Usage,
                "-I records requires --records RE; --records applies only to records input",
            ));
        }
        if let Some(pattern) = &self.records {
            Regex::new(pattern).map_err(|error| {
                Error::new(ErrorKind::Usage, format!("invalid records regex: {error}"))
            })?;
        }
        if self.field.is_some() && self.format != Format::Jsonl {
            return Err(Error::new(
                ErrorKind::Usage,
                "--field applies only to JSONL input",
            ));
        }
        if !self.select.is_empty() && !matches!(self.format, Format::Csv | Format::Tsv) {
            return Err(Error::new(
                ErrorKind::Usage,
                "--select applies only to CSV/TSV input",
            ));
        }
        if self.with_context != 0 && self.format != Format::Lines {
            return Err(Error::new(
                ErrorKind::Usage,
                "--with-context applies only to lines input",
            ));
        }
        if self.select.iter().any(|key| key.is_empty())
            || self.select.iter().collect::<BTreeSet<_>>().len() != self.select.len()
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "--select requires distinct nonempty column names",
            ));
        }
        if let Some(path) = &self.field {
            field_path(path)?;
        }
        Ok(())
    }
}

pub struct Source<R: BufRead> {
    driver: Driver<R>,
    options: Options,
    consumed: u64,
    previous: VecDeque<Value>,
    fields: Option<Vec<String>>,
}

enum Driver<R: BufRead> {
    Text(Reader<R>),
    Block(BlockReader<R>),
    Table(TableReader<R>),
}

impl<R: BufRead> Source<R> {
    pub fn new(input: R, options: Options, source: Option<PathBuf>) -> Result<Self> {
        options.validate()?;
        let fields = options
            .field
            .as_ref()
            .map(|path| field_path(path))
            .transpose()?;
        let driver = match options.format {
            Format::Csv | Format::Tsv => Driver::Table(TableReader::new(
                input,
                source,
                if options.format == Format::Csv {
                    b','
                } else {
                    b'\t'
                },
                &options.select,
            )?),
            Format::Paragraphs | Format::Records => {
                Driver::Block(BlockReader::new(input, source, options.records.as_deref())?)
            }
            format => Driver::Text(Reader::new(
                input,
                match format {
                    Format::Whole | Format::Files => InputFormat::Whole,
                    Format::Null => InputFormat::Null,
                    _ => InputFormat::Lines,
                },
                source,
                true,
            )),
        };
        Ok(Self {
            driver,
            options,
            consumed: 0,
            previous: VecDeque::new(),
            fields,
        })
    }

    pub fn consumed_units(&self) -> u64 {
        self.consumed
    }

    pub fn header(&self) -> Option<&[u8]> {
        match &self.driver {
            Driver::Table(table) => Some(&table.header),
            _ => None,
        }
    }

    pub fn next_unit(&mut self) -> Result<Option<Unit>> {
        loop {
            let (unit, consumed) = match &mut self.driver {
                Driver::Text(reader) => {
                    let unit = reader.next_unit()?;
                    (unit, reader.consumed_units())
                }
                Driver::Block(reader) => {
                    let unit = reader.next_unit()?;
                    (unit, reader.index)
                }
                Driver::Table(reader) => {
                    let unit = reader.next_unit()?;
                    (unit, reader.index)
                }
            };
            self.consumed = consumed;
            let Some(mut unit) = unit else {
                return Ok(None);
            };
            match self.options.format {
                Format::Lines => {
                    if self.options.with_context != 0 {
                        unit.state = json!({"unit":unit.subject,"previous_lines":self.previous});
                        self.previous.push_back(json!({"id":format!("L{}", unit.location.line_start), "text":unit.subject}));
                        while self.previous.len() > self.options.with_context {
                            self.previous.pop_front();
                        }
                    }
                    if !self.options.keep_blank && unit.text.is_empty() {
                        continue;
                    }
                }
                Format::Jsonl => {
                    let value: Value = serde_json::from_str(&unit.text).map_err(|error| {
                        Error::new(
                            ErrorKind::Input,
                            format!(
                                "JSONL unit {} at byte {}: {error}",
                                unit.index, unit.location.byte_start
                            ),
                        )
                    })?;
                    if !value.is_object() {
                        return Err(Error::new(
                            ErrorKind::Input,
                            format!("JSONL unit {} must be an object", unit.index),
                        ));
                    }
                    unit.state = value.clone();
                    unit.subject = if let Some(fields) = &self.fields {
                        select_path(&value, fields)?.clone()
                    } else {
                        value
                    };
                    if self.fields.is_some() {
                        unit.text = echo_text(&unit.subject)?;
                        unit.raw = unit.text.as_bytes().to_vec();
                        unit.raw.push(b'\n');
                    }
                }
                Format::Files => {
                    let path = unit.location.source.as_ref().ok_or_else(|| {
                        Error::new(
                            ErrorKind::Usage,
                            "--files requires file operands and cannot read stdin",
                        )
                    })?;
                    let path = path
                        .to_str()
                        .ok_or_else(|| Error::new(ErrorKind::Input, "file path is not UTF-8"))?;
                    unit.state = json!({"path":path,"content":unit.text});
                    unit.text = path.to_owned();
                    unit.subject = Value::String(unit.text.clone());
                    unit.raw = unit.text.as_bytes().to_vec();
                    unit.raw.push(b'\n');
                }
                _ => {}
            }
            return Ok(Some(unit));
        }
    }
}

pub fn echo_text(value: &Value) -> Result<String> {
    if let Value::String(text) = value {
        Ok(text.clone())
    } else {
        serde_json::to_string(value)
            .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))
    }
}

pub fn field_path(path: &str) -> Result<Vec<String>> {
    if path.is_empty() {
        return Err(Error::new(ErrorKind::Usage, "field path cannot be empty"));
    }
    if let Some(pointer) = path.strip_prefix('/') {
        let mut result = Vec::new();
        for component in pointer.split('/') {
            let mut decoded = String::new();
            let mut chars = component.chars();
            while let Some(ch) = chars.next() {
                if ch == '~' {
                    decoded.push(match chars.next() {
                        Some('0') => '~',
                        Some('1') => '/',
                        _ => {
                            return Err(Error::new(
                                ErrorKind::Usage,
                                "invalid JSON pointer escape",
                            ));
                        }
                    });
                } else {
                    decoded.push(ch);
                }
            }
            result.push(decoded);
        }
        Ok(result)
    } else {
        let fields: Vec<_> = path.split('.').map(str::to_owned).collect();
        if fields.iter().any(String::is_empty) {
            return Err(Error::new(ErrorKind::Usage, "invalid dotted field path"));
        }
        Ok(fields)
    }
}

pub fn select_path<'a>(value: &'a Value, path: &[String]) -> Result<&'a Value> {
    let mut selected = value;
    for component in path {
        selected = match selected {
            Value::Object(fields) => fields.get(component),
            Value::Array(items) => component
                .parse::<usize>()
                .ok()
                .filter(|_| component == "0" || !component.starts_with('0'))
                .and_then(|index| items.get(index)),
            _ => None,
        }
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Input,
                format!("missing field path component {component}"),
            )
        })?;
    }
    Ok(selected)
}

struct BlockReader<R> {
    input: R,
    source: Option<PathBuf>,
    marker: Option<Regex>,
    pending: Option<(Vec<u8>, u64, u64)>,
    byte: u64,
    line: u64,
    index: u64,
}

impl<R: BufRead> BlockReader<R> {
    fn new(input: R, source: Option<PathBuf>, marker: Option<&str>) -> Result<Self> {
        Ok(Self {
            input,
            source,
            marker: marker.map(Regex::new).transpose().map_err(|error| {
                Error::new(
                    ErrorKind::Usage,
                    format!("invalid --records regex: {error}"),
                )
            })?,
            pending: None,
            byte: 0,
            line: 1,
            index: 0,
        })
    }
    fn line(&mut self) -> Result<Option<(Vec<u8>, u64, u64)>> {
        if let Some(pending) = self.pending.take() {
            return Ok(Some(pending));
        }
        let mut raw = Vec::new();
        if self
            .input
            .read_until(b'\n', &mut raw)
            .map_err(|error| Error::io("reading record", error))?
            == 0
        {
            return Ok(None);
        }
        let start = (self.byte, self.line);
        self.byte = self
            .byte
            .checked_add(raw.len() as u64)
            .ok_or_else(|| Error::new(ErrorKind::Input, "input offset overflow"))?;
        self.line = self
            .line
            .checked_add(u64::from(raw.last() == Some(&b'\n')))
            .ok_or_else(|| Error::new(ErrorKind::Input, "input line overflow"))?;
        Ok(Some((raw, start.0, start.1)))
    }
    fn next_unit(&mut self) -> Result<Option<Unit>> {
        let mut raw = Vec::new();
        let mut byte_start = 0;
        let mut line_start = 1;
        let mut byte_end = 0;
        let mut line_end = 1;
        let mut content_len = 0;
        while let Some((line, byte, line_number)) = self.line()? {
            let text = std::str::from_utf8(&line).map_err(|error| {
                Error::new(
                    ErrorKind::Input,
                    format!("invalid UTF-8 at byte {byte}: {error}"),
                )
            })?;
            let blank = text.trim().is_empty();
            if self.marker.is_none() && raw.is_empty() && blank {
                continue;
            }
            if !raw.is_empty()
                && self
                    .marker
                    .as_ref()
                    .is_some_and(|marker| marker.is_match(text))
            {
                self.pending = Some((line, byte, line_number));
                break;
            }
            if raw.is_empty() {
                byte_start = byte;
                line_start = line_number;
            }
            byte_end = byte + line.len() as u64;
            line_end = line_number;
            raw.extend(&line);
            if self.marker.is_none() && blank {
                break;
            }
            content_len = raw.len();
        }
        if raw.is_empty() {
            return Ok(None);
        }
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::Input, "record index overflow"))?;
        let text = std::str::from_utf8(&raw[..content_len])
            .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?
            .to_owned();
        Ok(Some(Unit {
            index: self.index,
            subject: Value::String(text.clone()),
            state: Value::String(text.clone()),
            header: None,
            text,
            raw,
            location: Location {
                source: self.source.clone(),
                byte_start,
                byte_end,
                line_start,
                line_end,
            },
        }))
    }
}

#[derive(Default)]
struct Capture {
    bytes: VecDeque<u8>,
    start: u64,
}

struct Capturing<R> {
    input: R,
    capture: Arc<Mutex<Capture>>,
}
impl<R: BufRead> Read for Capturing<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let requested = buffer.len().saturating_sub(1).max(1);
        let mut count = self.input.read(&mut buffer[..requested])?;
        // Keep CRLF available to the byte capture even when a pipe splits the pair.
        if count != 0
            && count < buffer.len()
            && buffer.get(count - 1) == Some(&b'\r')
            && self.input.fill_buf()?.first() == Some(&b'\n')
        {
            buffer[count] = b'\n';
            self.input.consume(1);
            count += 1;
        }
        self.capture
            .lock()
            .map_err(|_| io::Error::other("CSV capture lock poisoned"))?
            .bytes
            .extend(&buffer[..count]);
        Ok(count)
    }
}

struct TableReader<R> {
    reader: csv::Reader<Capturing<R>>,
    capture: Arc<Mutex<Capture>>,
    columns: Vec<String>,
    header: Vec<u8>,
    selected: Vec<String>,
    source: Option<PathBuf>,
    index: u64,
    line: u64,
}

impl<R: BufRead> TableReader<R> {
    fn new(input: R, source: Option<PathBuf>, delimiter: u8, selected: &[String]) -> Result<Self> {
        let capture = Arc::new(Mutex::new(Capture::default()));
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .from_reader(Capturing {
                input,
                capture: capture.clone(),
            });
        let columns: Vec<_> = reader
            .headers()
            .map_err(table_error)?
            .iter()
            .map(str::to_owned)
            .collect();
        if columns.is_empty()
            || columns.iter().any(String::is_empty)
            || columns.iter().collect::<BTreeSet<_>>().len() != columns.len()
        {
            return Err(Error::new(
                ErrorKind::Input,
                "CSV/TSV requires distinct, nonempty column headers",
            ));
        }
        for column in selected {
            if !columns.contains(column) {
                return Err(Error::new(
                    ErrorKind::Input,
                    format!("unknown selected column {column}"),
                ));
            }
        }
        let (header, _, _) = take_bytes(&capture, 0, reader.position().byte())?;
        let line = 1 + line_breaks(&header);
        Ok(Self {
            reader,
            capture,
            columns,
            header,
            selected: selected.to_vec(),
            source,
            index: 0,
            line,
        })
    }
    fn next_unit(&mut self) -> Result<Option<Unit>> {
        let mut record = csv::StringRecord::new();
        if !self.reader.read_record(&mut record).map_err(table_error)? {
            return Ok(None);
        }
        let position = record
            .position()
            .ok_or_else(|| Error::new(ErrorKind::Input, "CSV record has no source position"))?;
        let start = position.byte();
        let end = self.reader.position().byte();
        let (raw, start, end) = take_bytes(&self.capture, start, end)?;
        let subject = Value::Object(
            self.columns
                .iter()
                .cloned()
                .zip(record.iter().map(|field| Value::String(field.to_owned())))
                .collect(),
        );
        let state = if self.selected.is_empty() {
            subject.clone()
        } else {
            Value::Object(
                self.selected
                    .iter()
                    .map(|key| {
                        Ok((
                            key.clone(),
                            subject
                                .get(key)
                                .ok_or_else(|| {
                                    Error::new(ErrorKind::Input, "selected CSV column disappeared")
                                })?
                                .clone(),
                        ))
                    })
                    .collect::<Result<_>>()?,
            )
        };
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::Input, "CSV record index overflow"))?;
        let newlines = line_breaks(&raw);
        let line_start = self.line;
        let line_end = line_start
            + newlines.saturating_sub(u64::from(matches!(raw.last(), Some(b'\r' | b'\n'))));
        self.line = self
            .line
            .checked_add(newlines)
            .ok_or_else(|| Error::new(ErrorKind::Input, "CSV line count overflow"))?;
        let text = std::str::from_utf8(&raw)
            .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        Ok(Some(Unit {
            index: self.index,
            text,
            subject,
            state,
            raw,
            header: Some(self.header.clone()),
            location: Location {
                source: self.source.clone(),
                byte_start: start,
                byte_end: end,
                line_start,
                line_end,
            },
        }))
    }
}

fn line_breaks(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .enumerate()
        .filter(|(index, byte)| {
            **byte == b'\r' || (**byte == b'\n' && (*index == 0 || bytes[*index - 1] != b'\r'))
        })
        .count() as u64
}

fn take_bytes(
    capture: &Arc<Mutex<Capture>>,
    start: u64,
    mut end: u64,
) -> Result<(Vec<u8>, u64, u64)> {
    let mut capture = capture
        .lock()
        .map_err(|_| Error::new(ErrorKind::Io, "CSV capture lock poisoned"))?;
    let start = if start < capture.start && capture.start - start == 1 {
        capture.start
    } else {
        start
    };
    let end_offset = usize::try_from(
        end.checked_sub(capture.start)
            .ok_or_else(|| Error::new(ErrorKind::Input, "invalid CSV source end"))?,
    )
    .map_err(|_| Error::new(ErrorKind::Input, "CSV source end overflow"))?;
    if end_offset != 0
        && capture.bytes.get(end_offset - 1) == Some(&b'\r')
        && capture.bytes.get(end_offset) == Some(&b'\n')
    {
        end = end
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::Input, "CSV source offset overflow"))?;
    }
    let offset =
        usize::try_from(start.checked_sub(capture.start).ok_or_else(|| {
            Error::new(ErrorKind::Input, "CSV source range precedes retained bytes")
        })?)
        .map_err(|_| Error::new(ErrorKind::Input, "CSV source offset overflow"))?;
    let length = usize::try_from(
        end.checked_sub(start)
            .ok_or_else(|| Error::new(ErrorKind::Input, "invalid CSV source range"))?,
    )
    .map_err(|_| Error::new(ErrorKind::Input, "CSV source length overflow"))?;
    if offset
        .checked_add(length)
        .is_none_or(|end| end > capture.bytes.len())
    {
        return Err(Error::new(
            ErrorKind::Input,
            "CSV source range exceeds captured input",
        ));
    }
    let bytes = capture
        .bytes
        .iter()
        .skip(offset)
        .take(length)
        .copied()
        .collect();
    capture.bytes.drain(..offset + length);
    capture.start = end;
    Ok((bytes, start, end))
}

fn table_error(error: csv::Error) -> Error {
    if let csv::ErrorKind::Io(_) = error.kind() {
        Error::new(ErrorKind::Io, format!("CSV/TSV input: {error}"))
    } else {
        Error::new(ErrorKind::Input, format!("CSV/TSV input: {error}"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::records::{Format, Options, Source, field_path};
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn paragraphs_preserve_multiline_bytes_locations_and_skip_leading_blanks() {
        let mut source = Source::new(
            Cursor::new(b"\nalpha\r\n stack\r\n\r\nbeta"),
            Options::new(Format::Paragraphs),
            None,
        )
        .unwrap();
        let first = source.next_unit().unwrap().unwrap();
        let second = source.next_unit().unwrap().unwrap();
        assert_eq!(first.raw, b"alpha\r\n stack\r\n\r\n");
        assert_eq!(first.text, "alpha\r\n stack\r\n");
        assert_eq!(first.location.byte_start, 1);
        assert_eq!(first.location.line_start, 2);
        assert_eq!(second.index, 2);
        assert_eq!(second.location.line_start, 5);
        assert_eq!(second.raw, b"beta");
        assert!(source.next_unit().unwrap().is_none());
    }

    #[test]
    fn regex_record_headers_stay_with_their_original_multiline_record() {
        let options = Options {
            records: Some(r"^\d{4}-".to_owned()),
            ..Options::new(Format::Records)
        };
        let mut source = Source::new(
            Cursor::new(b"2026-10-01 first\n stack 2026-10-02\n2026-10-03 next\n"),
            options,
            None,
        )
        .unwrap();
        let first = source.next_unit().unwrap().unwrap();
        let second = source.next_unit().unwrap().unwrap();
        assert_eq!(first.raw, b"2026-10-01 first\n stack 2026-10-02\n");
        assert_eq!(second.location.byte_start, first.location.byte_end);
        assert_eq!(second.location.line_start, 3);
        assert_eq!(second.raw, b"2026-10-03 next\n");
        assert!(source.next_unit().unwrap().is_none());
        assert!(
            Source::new(
                Cursor::new(b""),
                Options {
                    records: Some("[".to_owned()),
                    ..Options::new(Format::Records)
                },
                None
            )
            .is_err()
        );
    }

    #[test]
    fn jsonl_preserves_typed_state_large_integers_and_selects_escaped_paths() {
        let input = b"{\"n\":1267650600228229401496703205376,\"a/b\":{\"~\":[\"echo\"]}}\r\n";
        let mut source = Source::new(
            Cursor::new(input),
            Options {
                field: Some("/a~1b/~0/0".to_owned()),
                ..Options::new(Format::Jsonl)
            },
            None,
        )
        .unwrap();
        let unit = source.next_unit().unwrap().unwrap();
        assert_eq!(unit.subject, "echo");
        assert_eq!(unit.raw, b"echo\n");
        assert_eq!(
            unit.state["n"].to_string(),
            "1267650600228229401496703205376"
        );
        let mut source =
            Source::new(Cursor::new(input), Options::new(Format::Jsonl), None).unwrap();
        assert_eq!(source.next_unit().unwrap().unwrap().raw, input);
        assert!(field_path("/a~2").is_err());
        for input in [b"7\n".as_slice(), b"\n", b"{broken}\n"] {
            let mut source =
                Source::new(Cursor::new(input), Options::new(Format::Jsonl), None).unwrap();
            assert_eq!(source.next_unit().unwrap_err().exit_code(), 65);
        }
        let mut source = Source::new(
            Cursor::new(b"{\"x\":1}\n"),
            Options {
                field: Some("missing".to_owned()),
                ..Options::new(Format::Jsonl)
            },
            None,
        )
        .unwrap();
        assert_eq!(source.next_unit().unwrap_err().exit_code(), 65);
    }

    #[test]
    fn csv_and_tsv_preserve_header_quotes_multiline_rows_and_selected_state() {
        let input = b"name,note\r\n\"A, Inc\",\"first\nsecond\"\r\nB,ok\r\n";
        let mut source = Source::new(
            Cursor::new(input),
            Options {
                select: vec!["note".to_owned()],
                ..Options::new(Format::Csv)
            },
            None,
        )
        .unwrap();
        let first = source.next_unit().unwrap().unwrap();
        let second = source.next_unit().unwrap().unwrap();
        assert_eq!(
            first.subject,
            json!({"name":"A, Inc","note":"first\nsecond"})
        );
        assert_eq!(first.state, json!({"note":"first\nsecond"}));
        assert_eq!(first.header.as_deref(), Some(b"name,note\r\n".as_slice()));
        assert_eq!(first.raw, b"\"A, Inc\",\"first\nsecond\"\r\n");
        assert_eq!(second.raw, b"B,ok\r\n");
        assert_eq!(first.location.line_start, 2);
        assert_eq!(first.location.line_end, 3);
        assert_eq!(second.location.line_start, 4);
        assert!(source.next_unit().unwrap().is_none());
        let mut source = Source::new(
            Cursor::new(b"name\tnote\nx\t\"one\ttwo\"\n"),
            Options::new(Format::Tsv),
            None,
        )
        .unwrap();
        assert_eq!(
            source.next_unit().unwrap().unwrap().state,
            json!({"name":"x","note":"one\ttwo"})
        );
    }

    #[test]
    fn csv_headers_width_and_selected_columns_are_validated() {
        for input in [b"a,a\n1,2\n".as_slice(), b",b\n1,2\n", b""] {
            assert!(Source::new(Cursor::new(input), Options::new(Format::Csv), None).is_err());
        }
        let mut source = Source::new(
            Cursor::new(b"a,b\n1,2,3\n"),
            Options::new(Format::Csv),
            None,
        )
        .unwrap();
        assert_eq!(source.next_unit().unwrap_err().exit_code(), 65);
        assert!(
            Source::new(
                Cursor::new(b"a,b\n"),
                Options {
                    select: vec!["missing".to_owned()],
                    ..Options::new(Format::Csv)
                },
                None
            )
            .is_err()
        );
    }

    #[test]
    fn csv_cr_terminators_preserve_original_byte_and_line_provenance() {
        let mut source = Source::new(
            Cursor::new(b"name,note\ra,\"one\rtwo\"\rb,last\r"),
            Options::new(Format::Csv),
            None,
        )
        .unwrap();
        assert_eq!(source.header().unwrap(), b"name,note\r");
        let first = source.next_unit().unwrap().unwrap();
        let second = source.next_unit().unwrap().unwrap();
        assert_eq!(first.raw, b"a,\"one\rtwo\"\r");
        assert_eq!((first.location.line_start, first.location.line_end), (2, 3));
        assert_eq!(
            (second.location.line_start, second.location.line_end),
            (4, 4)
        );
        assert_eq!(second.location.byte_start, first.location.byte_end);
        assert!(source.next_unit().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn csv_real_stream_handles_crlf_pairs_delivered_in_separate_writes() {
        use std::io::{BufReader, Write};
        use std::os::unix::net::UnixStream;
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let writing = std::thread::spawn(move || {
            for bytes in [
                b"name,note\r".as_slice(),
                b"\nA,first\r",
                b"\nB,last\r",
                b"\n",
            ] {
                writer.write_all(bytes).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        let mut source = Source::new(
            BufReader::with_capacity(1, reader),
            Options::new(Format::Csv),
            None,
        )
        .unwrap();
        assert_eq!(source.header().unwrap(), b"name,note\r\n");
        assert_eq!(source.next_unit().unwrap().unwrap().raw, b"A,first\r\n");
        assert_eq!(source.next_unit().unwrap().unwrap().raw, b"B,last\r\n");
        assert!(source.next_unit().unwrap().is_none());
        writing.join().unwrap();
    }

    #[test]
    fn preceding_context_includes_skipped_blanks_but_never_becomes_echoed_subject() {
        let mut source = Source::new(
            Cursor::new(b"earlier\n\ncurrent\nlast\n"),
            Options {
                with_context: 2,
                ..Options::new(Format::Lines)
            },
            None,
        )
        .unwrap();
        source.next_unit().unwrap().unwrap();
        let current = source.next_unit().unwrap().unwrap();
        assert_eq!(current.index, 3);
        assert_eq!(current.raw, b"current\n");
        assert_eq!(current.subject, "current");
        assert_eq!(
            current.state,
            json!({"unit":"current","previous_lines":[{"id":"L1","text":"earlier"},{"id":"L2","text":""}]})
        );
        let last = source.next_unit().unwrap().unwrap();
        assert_eq!(last.state["previous_lines"][0]["id"], "L2");
    }

    #[test]
    fn csv_capture_memory_is_bounded_by_read_ahead_and_one_record() {
        let input = format!("a,b\n{}", "left,right\n".repeat(20_000));
        let mut source = Source::new(Cursor::new(input), Options::new(Format::Csv), None).unwrap();
        let mut count = 0;
        while source.next_unit().unwrap().is_some() {
            count += 1;
            if let crate::records::Driver::Table(table) = &source.driver {
                assert!(table.capture.lock().unwrap().bytes.len() <= 8192);
            }
        }
        assert_eq!(count, 20_000);
    }
}
