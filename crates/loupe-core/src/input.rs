use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputFormat {
    Lines,
    Whole,
    Null,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub source: Option<PathBuf>,
    pub byte_start: u64,
    pub byte_end: u64,
    pub line_start: u64,
    pub line_end: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub index: u64,
    pub text: String,
    pub raw: Vec<u8>,
    pub subject: Value,
    pub state: Value,
    pub header: Option<Vec<u8>>,
    pub location: Location,
}

pub struct Reader<R> {
    input: R,
    format: InputFormat,
    source: Option<PathBuf>,
    keep_blank: bool,
    index: u64,
    byte: u64,
    line: u64,
    complete: bool,
}

impl<R: BufRead> Reader<R> {
    pub fn new(input: R, format: InputFormat, source: Option<PathBuf>, keep_blank: bool) -> Self {
        Self {
            input,
            format,
            source,
            keep_blank,
            index: 0,
            byte: 0,
            line: 1,
            complete: false,
        }
    }

    pub fn consumed_units(&self) -> u64 {
        self.index
    }

    pub fn next_unit(&mut self) -> Result<Option<Unit>> {
        if self.complete {
            return Ok(None);
        }
        loop {
            let mut raw = Vec::new();
            let length = match self.format {
                InputFormat::Whole => {
                    self.complete = true;
                    self.input.read_to_end(&mut raw)
                }
                InputFormat::Lines => self.input.read_until(b'\n', &mut raw),
                InputFormat::Null => self.input.read_until(0, &mut raw),
            }
            .map_err(|error| Error::io("reading input", error))?;
            if length == 0 && self.format != InputFormat::Whole {
                self.complete = true;
                return Ok(None);
            }
            let byte_start = self.byte;
            let line_start = self.line;
            self.byte = self
                .byte
                .checked_add(
                    u64::try_from(length)
                        .map_err(|_| Error::new(ErrorKind::Input, "input offset overflow"))?,
                )
                .ok_or_else(|| Error::new(ErrorKind::Input, "input offset overflow"))?;
            let newlines = u64::try_from(raw.iter().filter(|byte| **byte == b'\n').count())
                .map_err(|_| Error::new(ErrorKind::Input, "line count overflow"))?;
            self.line = self
                .line
                .checked_add(newlines)
                .ok_or_else(|| Error::new(ErrorKind::Input, "line count overflow"))?;
            self.index = self
                .index
                .checked_add(1)
                .ok_or_else(|| Error::new(ErrorKind::Input, "unit index overflow"))?;
            let content_length = match self.format {
                InputFormat::Lines => raw
                    .len()
                    .saturating_sub(usize::from(raw.last() == Some(&b'\n'))),
                InputFormat::Null => raw
                    .len()
                    .saturating_sub(usize::from(raw.last() == Some(&0))),
                InputFormat::Whole => raw.len(),
            };
            let mut content = &raw[..content_length];
            if self.format == InputFormat::Lines && content.last() == Some(&b'\r') {
                content = &content[..content.len().saturating_sub(1)];
            }
            let text = std::str::from_utf8(content)
                .map_err(|error| {
                    Error::new(
                        ErrorKind::Input,
                        format!(
                            "invalid UTF-8 in unit {} at byte {}: {error}",
                            self.index, byte_start
                        ),
                    )
                })?
                .to_owned();
            if self.format == InputFormat::Lines && !self.keep_blank && text.is_empty() {
                continue;
            }
            let line_end = self
                .line
                .saturating_sub(u64::from(raw.last() == Some(&b'\n')));
            return Ok(Some(Unit {
                index: self.index,
                subject: Value::String(text.clone()),
                state: Value::String(text.clone()),
                header: None,
                text,
                raw,
                location: Location {
                    source: self.source.clone(),
                    byte_start,
                    byte_end: self.byte,
                    line_start,
                    line_end,
                },
            }));
        }
    }
}

impl Reader<BufReader<File>> {
    pub fn from_file(path: PathBuf, format: InputFormat, keep_blank: bool) -> Result<Self> {
        let file = File::open(&path)
            .map_err(|error| Error::io(&format!("opening {}", path.display()), error))?;
        Ok(Self::new(
            BufReader::new(file),
            format,
            Some(path),
            keep_blank,
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::input::{InputFormat, Reader};
    use std::io::Cursor;

    #[test]
    fn skipped_blank_lines_keep_source_ids_offsets_and_exact_bytes() {
        let mut reader = Reader::new(
            Cursor::new(b"first\r\n\nthird"),
            InputFormat::Lines,
            None,
            false,
        );
        let first = reader.next_unit().unwrap().unwrap();
        let third = reader.next_unit().unwrap().unwrap();
        assert_eq!(first.text, "first");
        assert_eq!(first.raw, b"first\r\n");
        assert_eq!(third.index, 3);
        assert_eq!(third.location.line_start, 3);
        assert_eq!(third.location.byte_start, 8);
        assert_eq!(third.raw, b"third");
        assert!(reader.next_unit().unwrap().is_none());
    }

    #[test]
    fn nul_records_preserve_delimiters_and_unterminated_final_record() {
        let mut reader = Reader::new(
            Cursor::new(b"one\0two\0last"),
            InputFormat::Null,
            None,
            false,
        );
        assert_eq!(reader.next_unit().unwrap().unwrap().raw, b"one\0");
        assert_eq!(reader.next_unit().unwrap().unwrap().text, "two");
        assert_eq!(reader.next_unit().unwrap().unwrap().raw, b"last");
        assert!(reader.next_unit().unwrap().is_none());
    }

    #[test]
    fn invalid_utf8_is_input_failure_and_whole_is_one_unit() {
        let mut invalid = Reader::new(Cursor::new([0xff, b'\n']), InputFormat::Lines, None, false);
        assert!(invalid.next_unit().is_err());
        let mut whole = Reader::new(
            Cursor::new(b"first\nsecond\n"),
            InputFormat::Whole,
            None,
            false,
        );
        let unit = whole.next_unit().unwrap().unwrap();
        assert_eq!(unit.text, "first\nsecond\n");
        assert_eq!(unit.location.line_end, 2);
        assert!(whole.next_unit().unwrap().is_none());
    }

    #[test]
    fn empty_whole_input_is_a_real_state_but_empty_line_input_has_no_units() {
        let mut whole = Reader::new(Cursor::new(b""), InputFormat::Whole, None, false);
        let unit = whole.next_unit().unwrap().unwrap();
        assert_eq!(unit.text, "");
        assert_eq!(unit.location.byte_start, 0);
        assert_eq!(unit.location.byte_end, 0);
        assert!(whole.next_unit().unwrap().is_none());
        let mut lines = Reader::new(Cursor::new(b""), InputFormat::Lines, None, false);
        assert!(lines.next_unit().unwrap().is_none());
    }

    #[test]
    fn keep_blank_preserves_empty_lines_and_whole_keeps_crlf() {
        let mut lines = Reader::new(Cursor::new(b"\n\r\n"), InputFormat::Lines, None, true);
        let first = lines.next_unit().unwrap().unwrap();
        let second = lines.next_unit().unwrap().unwrap();
        assert_eq!(first.text, "");
        assert_eq!(first.raw, b"\n");
        assert_eq!(second.text, "");
        assert_eq!(second.raw, b"\r\n");
        let mut whole = Reader::new(Cursor::new(b"x\r\ny"), InputFormat::Whole, None, false);
        assert_eq!(whole.next_unit().unwrap().unwrap().text, "x\r\ny");
    }

    #[test]
    fn actual_file_reader_preserves_bytes_and_source_path() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../LICENSE");
        let expected = std::fs::read(&path).unwrap();
        let mut reader = Reader::from_file(path.clone(), InputFormat::Whole, false).unwrap();
        let unit = reader.next_unit().unwrap().unwrap();
        assert_eq!(unit.raw, expected);
        assert_eq!(unit.location.source, Some(path));
        assert_eq!(
            unit.location.byte_end,
            u64::try_from(expected.len()).unwrap()
        );
        assert!(reader.next_unit().unwrap().is_none());
    }

    #[test]
    fn real_file_open_failure_has_io_exit_code() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../LICENSE/child");
        let failure = Reader::from_file(path, InputFormat::Lines, false)
            .err()
            .unwrap();
        assert_eq!(failure.exit_code(), 74);
    }
}
