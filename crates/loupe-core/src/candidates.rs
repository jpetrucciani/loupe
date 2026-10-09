use std::fmt;
use std::io::Cursor;
use std::num::NonZeroU64;
use std::ops::Range;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorKind, Result};
use crate::input::{InputFormat, Location, Reader};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Family {
    Line,
    Parsed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id {
    family: Family,
    number: NonZeroU64,
}

impl Id {
    fn new(family: Family, number: u64) -> Result<Self> {
        Ok(Self {
            family,
            number: NonZeroU64::new(number).ok_or_else(|| invalid("candidate IDs start at one"))?,
        })
    }
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.family {
            Family::Line => 'L',
            Family::Parsed => 'C',
        };
        write!(formatter, "{prefix}{}", self.number)
    }
}

impl FromStr for Id {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let (family, number) = if let Some(number) = value.strip_prefix('L') {
            (Family::Line, number)
        } else if let Some(number) = value.strip_prefix('C') {
            (Family::Parsed, number)
        } else {
            return Err(invalid("candidate ID must be L<n> or C<n>"));
        };
        if number.starts_with('0') || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid("candidate ID needs a canonical positive integer"));
        }
        Self::new(
            family,
            number
                .parse()
                .map_err(|_| invalid("candidate ID overflow or missing number"))?,
        )
    }
}

impl TryFrom<String> for Id {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<Id> for String {
    fn from(value: Id) -> Self {
        value.to_string()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Candidate<'a> {
    pub id: Id,
    pub verbatim: &'a str,
    pub location: Location,
    #[serde(skip)]
    pub raw: &'a str,
    #[serde(skip)]
    range: Range<usize>,
}

/// A finite UTF-8 source. Candidate text borrows the original bytes; only the
/// physical line index and candidate metadata allocate additional storage.
pub struct Document<'a> {
    text: &'a str,
    source: Option<PathBuf>,
    line_starts: Vec<usize>,
}

impl<'a> Document<'a> {
    pub fn new(text: &'a str, source: Option<PathBuf>) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(position, _)| position + 1));
        Self {
            text,
            source,
            line_starts,
        }
    }

    pub fn slice(&self, range: Range<usize>) -> Result<&'a str> {
        self.text.get(range).ok_or_else(|| {
            invalid("candidate/chunk range is outside the source or splits a UTF-8 character")
        })
    }

    pub fn text(&self) -> &'a str {
        self.text
    }

    pub fn lines(&self, keep_blank: bool) -> Result<Vec<Candidate<'a>>> {
        let mut reader = Reader::new(
            Cursor::new(self.text.as_bytes()),
            InputFormat::Lines,
            self.source.clone(),
            keep_blank,
        );
        let mut candidates = Vec::new();
        while let Some(unit) = reader.next_unit()? {
            let start = usize::try_from(unit.location.byte_start)
                .map_err(|_| invalid("candidate offset overflow"))?;
            let end = usize::try_from(unit.location.byte_end)
                .map_err(|_| invalid("candidate offset overflow"))?;
            let content_end = start
                .checked_add(unit.text.len())
                .ok_or_else(|| invalid("candidate offset overflow"))?;
            let mut location = unit.location;
            location.byte_end = offset(content_end)?;
            candidates.push(Candidate {
                id: Id::new(Family::Line, unit.index)?,
                verbatim: self.slice(start..content_end)?,
                raw: self.slice(start..end)?,
                location,
                range: start..end,
            });
        }
        Ok(candidates)
    }

    /// Assign once after all parser/chunk spans have been mapped into the full
    /// source. Equal ranges collapse; equal text at different positions remains.
    pub fn parsed(&self, mut spans: Vec<Range<usize>>) -> Result<Vec<Candidate<'a>>> {
        for span in &spans {
            if self.slice(span.clone())?.is_empty() {
                return Err(invalid(
                    "parsed candidates must have nonempty verbatim spans",
                ));
            }
        }
        spans.sort_unstable_by_key(|span| (span.start, span.end));
        spans.dedup();
        let mut candidates = Vec::with_capacity(spans.len());
        for (position, range) in spans.into_iter().enumerate() {
            let number = offset(position)?
                .checked_add(1)
                .ok_or_else(|| invalid("candidate ID overflow"))?;
            let verbatim = self.slice(range.clone())?;
            candidates.push(Candidate {
                id: Id::new(Family::Parsed, number)?,
                verbatim,
                raw: verbatim,
                location: Location {
                    source: self.source.clone(),
                    byte_start: offset(range.start)?,
                    byte_end: offset(range.end)?,
                    line_start: self.line(range.start)?,
                    line_end: self.line(range.end - 1)?,
                },
                range,
            });
        }
        Ok(candidates)
    }

    pub fn map_chunk(
        &self,
        chunk: Range<usize>,
        local: &[Range<usize>],
    ) -> Result<Vec<Range<usize>>> {
        let text = self.slice(chunk.clone())?;
        local
            .iter()
            .map(|span| {
                if text.get(span.clone()).is_none_or(str::is_empty) {
                    return Err(invalid(
                        "chunk candidate must be a nonempty UTF-8 span within that chunk",
                    ));
                }
                let start = chunk
                    .start
                    .checked_add(span.start)
                    .ok_or_else(|| invalid("candidate offset overflow"))?;
                let end = chunk
                    .start
                    .checked_add(span.end)
                    .ok_or_else(|| invalid("candidate offset overflow"))?;
                Ok(start..end)
            })
            .collect()
    }

    /// Selecting a chunk keeps global IDs and original ranges. Candidates that
    /// cross its boundary are retained globally but cannot become partial spans.
    pub fn in_chunk<'b>(
        &self,
        candidates: &'b [Candidate<'a>],
        chunk: Range<usize>,
    ) -> Result<Vec<&'b Candidate<'a>>> {
        self.slice(chunk.clone())?;
        for candidate in candidates {
            if candidate.location.source != self.source
                || !std::ptr::eq(self.slice(candidate.range.clone())?, candidate.raw)
            {
                return Err(invalid(
                    "candidate belongs to a different document snapshot",
                ));
            }
        }
        Ok(candidates
            .iter()
            .filter(|candidate| {
                candidate.range.start >= chunk.start && candidate.range.end <= chunk.end
            })
            .collect())
    }

    fn line(&self, byte: usize) -> Result<u64> {
        offset(self.line_starts.partition_point(|start| *start <= byte))
    }
}

fn offset(value: usize) -> Result<u64> {
    u64::try_from(value).map_err(|_| invalid("candidate offset overflow"))
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Input, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::ops::Range;
    use std::path::PathBuf;

    use serde_json::json;

    use crate::candidates::{Document, Id};

    #[test]
    fn lines_preserve_physical_ids_crlf_blanks_final_bytes_and_verbatim_locations() {
        let document = Document::new("α\r\n\n終", Some(PathBuf::from("source.txt")));
        let candidates = document.lines(false).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.id.to_string())
                .collect::<Vec<_>>(),
            ["L1", "L3"]
        );
        assert_eq!(candidates[0].raw, "α\r\n");
        assert_eq!(candidates[0].verbatim, "α");
        assert_eq!(
            (
                candidates[0].location.byte_start,
                candidates[0].location.byte_end
            ),
            (0, 2)
        );
        assert_eq!(
            (
                candidates[1].location.byte_start,
                candidates[1].location.byte_end
            ),
            (5, 8)
        );
        assert_eq!(candidates[1].location.line_start, 3);
        let all = document.lines(true).unwrap();
        assert_eq!(all[1].id.to_string(), "L2");
        assert_eq!(all[1].verbatim, "");
        assert_eq!(all[1].raw, "\n");
        assert_eq!(
            all.iter()
                .map(|candidate| candidate.raw)
                .collect::<String>(),
            "α\r\n\n終"
        );
        assert!(Document::new("", None).lines(true).unwrap().is_empty());
        assert_eq!(Document::new("last\n", None).lines(true).unwrap().len(), 1);
        let json = serde_json::to_value(&candidates[1]).unwrap();
        assert_eq!(json["id"], "L3");
        assert_eq!(json["verbatim"], "終");
        assert_eq!(json["location"]["source"], "source.txt");
        assert!(json.get("raw").is_none());
    }

    #[test]
    fn parsed_spans_sort_by_original_position_and_collapse_only_identical_ranges() {
        let text = "東京 v1.2\nagain v1.2";
        let document = Document::new(text, None);
        let spans = text
            .match_indices("v1.2")
            .map(|(start, value)| start..start + value.len())
            .collect::<Vec<_>>();
        let candidates = document
            .parsed(vec![
                spans[1].clone(),
                spans[0].clone(),
                0..6,
                spans[0].clone(),
            ])
            .unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.id.to_string())
                .collect::<Vec<_>>(),
            ["C1", "C2", "C3"]
        );
        assert_eq!(candidates[0].verbatim, "東京");
        assert_eq!(candidates[1].verbatim, candidates[2].verbatim);
        assert_ne!(
            candidates[1].location.byte_start,
            candidates[2].location.byte_start
        );
        assert_eq!(candidates[2].location.line_start, 2);
        let crossing = document
            .parsed(std::iter::once(0..text.len()).collect())
            .unwrap();
        assert_eq!(crossing[0].location.line_end, 2);
        let prefix = document.parsed(std::iter::once(0..12).collect()).unwrap();
        assert_eq!(prefix[0].location.line_end, 1);
    }

    #[test]
    fn every_unicode_chunk_retains_global_ids_and_overlap_maps_back_without_duplicates() {
        let text = "α x 東京 x\n終 x.";
        let document = Document::new(text, None);
        let spans = text
            .match_indices('x')
            .map(|(start, _)| start..start + 1)
            .collect::<Vec<_>>();
        let original = document.parsed(spans.clone()).unwrap();
        let mut boundaries = text
            .char_indices()
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        boundaries.push(text.len());
        let mut mapped = Vec::new();
        for start in &boundaries {
            for end in boundaries.iter().filter(|end| **end >= *start) {
                let chunk = *start..*end;
                let visible = document.in_chunk(&original, chunk.clone()).unwrap();
                let local = visible
                    .iter()
                    .map(|candidate| {
                        let range = &candidate.range;
                        range.start - *start..range.end - *start
                    })
                    .collect::<Vec<_>>();
                let global = document.map_chunk(chunk, &local).unwrap();
                for (candidate, span) in visible.iter().zip(&global) {
                    assert_eq!(candidate.range, *span);
                    assert_eq!(candidate.verbatim, document.slice(span.clone()).unwrap());
                }
                mapped.extend(global);
            }
        }
        mapped.reverse();
        let combined = document.parsed(mapped).unwrap();
        assert_eq!(
            serde_json::to_value(&combined).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        let line = document.lines(false).unwrap();
        assert_eq!(
            document
                .in_chunk(&line, 0..text.len() - 2)
                .unwrap()
                .iter()
                .map(|candidate| candidate.id.to_string())
                .collect::<Vec<_>>(),
            ["L1"]
        );
    }

    #[test]
    fn invalid_boundaries_ranges_and_empty_parser_matches_are_explicit_errors() {
        let document = Document::new("東京", None);
        for span in [1..3, 0..1, 0..7, Range { start: 4, end: 2 }, 0..0] {
            assert_eq!(document.parsed(vec![span]).unwrap_err().exit_code(), 65);
        }
        for span in [0..4, 1..3, 0..0] {
            assert!(
                document
                    .map_chunk(0..3, std::slice::from_ref(&span))
                    .is_err()
            );
        }
        assert!(document.in_chunk(&[], 1..3).is_err());
    }

    #[test]
    fn candidate_ids_are_canonical_typed_positive_numbers_and_reserve_none() {
        for id in ["L1", "C2", "C18446744073709551615"] {
            let parsed: Id = id.parse().unwrap();
            assert_eq!(parsed.to_string(), id);
            assert_eq!(serde_json::to_value(parsed).unwrap(), json!(id));
            assert_eq!(serde_json::from_value::<Id>(json!(id)).unwrap(), parsed);
        }
        for id in [
            "none",
            "uncertain",
            "C0",
            "L01",
            "L",
            "C+1",
            "C-1",
            "C١",
            "c1",
            "C18446744073709551616",
        ] {
            assert!(id.parse::<Id>().is_err(), "{id}");
        }
    }

    #[test]
    fn chunk_views_refuse_candidates_from_other_document_snapshots() {
        let text = String::from("same bytes");
        let original = Document::new(&text, Some(PathBuf::from("first.txt")));
        let candidates = original.lines(false).unwrap();
        let other_source = Document::new(&text, Some(PathBuf::from("second.txt")));
        assert!(other_source.in_chunk(&candidates, 0..text.len()).is_err());
        let other_text = text.clone();
        let other_snapshot = Document::new(&other_text, Some(PathBuf::from("first.txt")));
        assert!(other_snapshot.in_chunk(&candidates, 0..text.len()).is_err());
    }
}
