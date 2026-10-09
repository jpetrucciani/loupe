//! Finite right-side blocking with reusable, bounded left-query scratch space.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::io::BufRead;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use crate::records::{Options, Source, field_path, select_path};

const PERMUTATIONS: usize = 64;
const ROWS_PER_BAND: usize = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Tokens,
    Lsh,
    #[default]
    Hybrid,
}
impl std::str::FromStr for Mode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "tokens" => Ok(Self::Tokens),
            "lsh" => Ok(Self::Lsh),
            "hybrid" => Ok(Self::Hybrid),
            _ => Err(Error::new(
                ErrorKind::Usage,
                "blocking mode must be tokens, lsh or hybrid",
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Fields {
    paths: Vec<Vec<String>>,
    names: Vec<String>,
}
impl Fields {
    pub fn parse(value: &str) -> Result<Self> {
        let names = value
            .split(',')
            .map(str::trim)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if names.is_empty() || names.iter().any(String::is_empty) {
            return Err(Error::new(
                ErrorKind::Usage,
                "--on needs nonempty field paths",
            ));
        }
        let paths = names
            .iter()
            .map(|name| field_path(name))
            .collect::<Result<Vec<_>>>()?;
        if paths.iter().collect::<BTreeSet<_>>().len() != paths.len() {
            return Err(Error::new(
                ErrorKind::Usage,
                "--on field paths must be distinct",
            ));
        }
        Ok(Self { paths, names })
    }
    pub fn names(&self) -> &[String] {
        &self.names
    }
    fn normalize(&self, unit: &Unit) -> Result<Normalized> {
        let mut tokens = BTreeSet::new();
        let mut fields = Vec::new();
        for (position, path) in self.paths.iter().enumerate() {
            let value = select_path(&unit.state, path).map_err(|error| {
                Error::new(error.kind, format!("record {}: {error}", unit.index))
            })?;
            let text = match value {
                Value::Null => String::new(),
                Value::String(text) => text.clone(),
                Value::Number(_) | Value::Bool(_) => value.to_string(),
                _ => {
                    return Err(Error::new(
                        ErrorKind::Input,
                        format!(
                            "record {}: blocking fields must be scalar values",
                            unit.index
                        ),
                    ));
                }
            };
            let text = text
                .chars()
                .flat_map(char::to_lowercase)
                .map(|character| {
                    if character.is_alphanumeric() {
                        character
                    } else {
                        ' '
                    }
                })
                .collect::<String>();
            let words = text.split_whitespace().collect::<BTreeSet<_>>();
            fields.push(words.iter().copied().collect::<Vec<_>>().join(" "));
            tokens.extend(words.into_iter().map(|word| format!("{position}:{word}")));
        }
        let bands = bands(&tokens);
        Ok(Normalized {
            fields,
            tokens,
            bands,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct Band {
    position: usize,
    digest: [u8; 32],
}
struct Normalized {
    fields: Vec<String>,
    tokens: BTreeSet<String>,
    bands: Vec<Band>,
}
struct Right {
    unit: Unit,
    normalized: Normalized,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub records: usize,
    pub raw_bytes: u64,
    pub distinct_tokens: usize,
    pub token_postings: usize,
    pub distinct_bands: usize,
    pub band_postings: usize,
    pub scratch_slots: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub right_index: u64,
    pub exact_fields: usize,
    pub intersection: usize,
    pub union: usize,
    pub token_jaccard: f64,
    pub matching_lsh_bands: usize,
    #[serde(skip)]
    position: usize,
    #[serde(skip)]
    owner: [u8; 32],
}
impl Candidate {
    fn compare(&self, other: &Self) -> Ordering {
        other
            .exact_fields
            .cmp(&self.exact_fields)
            .then_with(|| {
                ((other.intersection as u128) * (self.union as u128))
                    .cmp(&((self.intersection as u128) * (other.union as u128)))
            })
            .then_with(|| self.right_index.cmp(&other.right_index))
    }
}
#[derive(Debug, Serialize)]
pub struct Selection {
    pub eligible_records: usize,
    pub unavailable_records: usize,
    pub candidates: Vec<Candidate>,
}

pub struct Index {
    fields: Fields,
    records: Vec<Right>,
    tokens: HashMap<String, Vec<usize>>,
    bands: HashMap<Band, Vec<usize>>,
    marks: Vec<u64>,
    generation: u64,
    raw_bytes: u64,
    identity: [u8; 32],
}
impl Index {
    pub fn load(
        input: impl BufRead,
        options: Options,
        source: Option<PathBuf>,
        fields: Fields,
    ) -> Result<Self> {
        let mut reader = Source::new(input, options, source)?;
        let mut hash = blake3::Hasher::new_derive_key("loupe blocking right index v1");
        let names = serde_json::to_vec(&fields.paths)
            .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?;
        hash.update(&names);
        let mut index = Self {
            fields,
            records: Vec::new(),
            tokens: HashMap::new(),
            bands: HashMap::new(),
            marks: Vec::new(),
            generation: 0,
            raw_bytes: 0,
            identity: [0; 32],
        };
        while let Some(unit) = reader.next_unit()? {
            let normalized = index.fields.normalize(&unit)?;
            let encoded = serde_json::to_vec(&unit)
                .map_err(|error| Error::new(ErrorKind::Input, error.to_string()))?;
            hash.update(
                &u64::try_from(encoded.len())
                    .map_err(|_| overflow())?
                    .to_le_bytes(),
            );
            hash.update(&encoded);
            let position = index.records.len();
            index.raw_bytes = index
                .raw_bytes
                .checked_add(u64::try_from(unit.raw.len()).map_err(|_| overflow())?)
                .ok_or_else(overflow)?;
            for token in &normalized.tokens {
                index
                    .tokens
                    .entry(token.clone())
                    .or_default()
                    .push(position);
            }
            for band in &normalized.bands {
                index.bands.entry(band.clone()).or_default().push(position);
            }
            index.records.push(Right { unit, normalized });
            index.marks.push(0);
        }
        index.identity = *hash.finalize().as_bytes();
        Ok(index)
    }
    pub fn stats(&self) -> Result<Stats> {
        Ok(Stats {
            records: self.records.len(),
            raw_bytes: self.raw_bytes,
            distinct_tokens: self.tokens.len(),
            token_postings: total(self.tokens.values().map(Vec::len))?,
            distinct_bands: self.bands.len(),
            band_postings: total(self.bands.values().map(Vec::len))?,
            scratch_slots: self.marks.len(),
        })
    }
    pub fn unit(&self, candidate: &Candidate) -> Result<&Unit> {
        if candidate.owner != self.identity {
            return Err(Error::new(
                ErrorKind::Input,
                "blocking candidate belongs to a different right index",
            ));
        }
        self.records
            .get(candidate.position)
            .map(|record| &record.unit)
            .filter(|unit| unit.index == candidate.right_index)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Input,
                    "blocking candidate is not in this right index",
                )
            })
    }
    pub fn select(&mut self, left: &Unit, limit: NonZeroUsize, mode: Mode) -> Result<Selection> {
        self.select_available(left, limit, mode, None)
    }
    pub fn identity(&self) -> String {
        blake3::Hash::from(self.identity).to_hex().to_string()
    }
    pub fn units(&self) -> impl Iterator<Item = &Unit> {
        self.records.iter().map(|record| &record.unit)
    }
    pub fn select_available(
        &mut self,
        left: &Unit,
        limit: NonZeroUsize,
        mode: Mode,
        excluded: Option<&BTreeSet<u64>>,
    ) -> Result<Selection> {
        let normalized = self.fields.normalize(left)?;
        self.generation = match self.generation.checked_add(1) {
            Some(generation) => generation,
            None => {
                self.marks.fill(0);
                1
            }
        };
        for token in normalized.tokens.iter().filter(|_| mode != Mode::Lsh) {
            if let Some(postings) = self.tokens.get(token) {
                mark(&mut self.marks, postings, self.generation)?;
            }
        }
        for band in normalized.bands.iter().filter(|_| mode != Mode::Tokens) {
            if let Some(postings) = self.bands.get(band) {
                mark(&mut self.marks, postings, self.generation)?;
            }
        }
        let mut candidates = Vec::<Candidate>::new();
        let mut eligible_records = 0usize;
        let mut unavailable_records = 0usize;
        for (position, right) in self.records.iter().enumerate() {
            if self.marks.get(position) != Some(&self.generation) {
                continue;
            }
            let intersection = normalized
                .tokens
                .intersection(&right.normalized.tokens)
                .count();
            let union = normalized
                .tokens
                .len()
                .checked_add(right.normalized.tokens.len())
                .and_then(|sum| sum.checked_sub(intersection))
                .ok_or_else(overflow)?;
            if union == 0 {
                continue;
            }
            eligible_records = eligible_records.checked_add(1).ok_or_else(overflow)?;
            if excluded.is_some_and(|indices| indices.contains(&right.unit.index)) {
                unavailable_records = unavailable_records.checked_add(1).ok_or_else(overflow)?;
                continue;
            }
            let candidate = Candidate {
                right_index: right.unit.index,
                exact_fields: normalized
                    .fields
                    .iter()
                    .zip(&right.normalized.fields)
                    .filter(|(left, right)| !left.is_empty() && left == right)
                    .count(),
                intersection,
                union,
                token_jaccard: intersection as f64 / union as f64,
                matching_lsh_bands: normalized
                    .bands
                    .iter()
                    .zip(&right.normalized.bands)
                    .filter(|(left, right)| left == right)
                    .count(),
                position,
                owner: self.identity,
            };
            let insertion = candidates
                .partition_point(|existing| existing.compare(&candidate) != Ordering::Greater);
            if insertion < limit.get() {
                candidates.insert(insertion, candidate);
                candidates.truncate(limit.get());
            }
        }
        Ok(Selection {
            eligible_records,
            unavailable_records,
            candidates,
        })
    }
}
fn mark(marks: &mut [u64], postings: &[usize], generation: u64) -> Result<()> {
    for position in postings {
        *marks
            .get_mut(*position)
            .ok_or_else(|| Error::new(ErrorKind::Input, "invalid blocking index posting"))? =
            generation;
    }
    Ok(())
}
pub(crate) fn bands(tokens: &BTreeSet<String>) -> Vec<Band> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut minima = [u64::MAX; PERMUTATIONS];
    for token in tokens {
        for (permutation, minimum) in minima.iter_mut().enumerate() {
            let mut hash = blake3::Hasher::new_derive_key("loupe blocking minhash v1");
            hash.update(&(permutation as u64).to_le_bytes());
            hash.update(token.as_bytes());
            let digest = hash.finalize();
            let value = digest
                .as_bytes()
                .iter()
                .take(8)
                .enumerate()
                .fold(0u64, |value, (shift, byte)| {
                    value | u64::from(*byte) << (shift * 8)
                });
            *minimum = (*minimum).min(value);
        }
    }
    minima
        .as_chunks::<ROWS_PER_BAND>()
        .0
        .iter()
        .enumerate()
        .map(|(position, rows)| {
            let mut hash = blake3::Hasher::new_derive_key("loupe blocking lsh band v1");
            for row in rows {
                hash.update(&row.to_le_bytes());
            }
            Band {
                position,
                digest: *hash.finalize().as_bytes(),
            }
        })
        .collect()
}
fn overflow() -> Error {
    Error::new(ErrorKind::Input, "blocking size overflow")
}
fn total(mut lengths: impl Iterator<Item = usize>) -> Result<usize> {
    lengths.try_fold(0usize, |sum, length| {
        sum.checked_add(length).ok_or_else(overflow)
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::blocking::{Fields, Index, Mode, bands};
    use crate::input::Unit;
    use crate::records::{Format, Options, Source};
    use serde_json::{Value, json};
    use std::io::Cursor;
    use std::num::NonZeroUsize;

    fn records(values: &[Value]) -> Vec<u8> {
        values
            .iter()
            .map(|value| format!("{value}\n"))
            .collect::<String>()
            .into_bytes()
    }
    fn left(value: Value) -> Unit {
        Source::new(
            Cursor::new(records(&[value])),
            Options::new(Format::Jsonl),
            None,
        )
        .unwrap()
        .next_unit()
        .unwrap()
        .unwrap()
    }
    fn make_index(values: &[Value], fields: &str) -> Index {
        Index::load(
            Cursor::new(records(values)),
            Options::new(Format::Jsonl),
            None,
            Fields::parse(fields).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn field_qualified_tokens_normalize_case_punctuation_order_and_unicode() {
        let mut index = make_index(
            &[
                json!({"name":"ÉCOLE, Alpha!", "location":"New-York"}),
                json!({"name":"New York", "location":"École Alpha"}),
                json!({"name":"École Beta", "location":"Berlin"}),
                json!({"name":"different", "location":null}),
            ],
            "name,location",
        );
        let input = left(json!({"name":"alpha école", "location":"new york"}));
        let selection = index
            .select(&input, NonZeroUsize::new(20).unwrap(), Mode::Hybrid)
            .unwrap();
        assert_eq!(selection.eligible_records, 2);
        assert_eq!(
            selection
                .candidates
                .iter()
                .map(|candidate| candidate.right_index)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(selection.candidates[0].token_jaccard, 1.0);
        assert_eq!(selection.candidates[0].exact_fields, 2);
        assert_eq!(selection.candidates[0].matching_lsh_bands, 8);
        assert_eq!(
            index.unit(&selection.candidates[0]).unwrap().state["name"],
            json!("ÉCOLE, Alpha!")
        );
        assert_eq!(index.stats().unwrap().band_postings, 32);
    }

    #[test]
    fn cap_and_ties_are_stable_and_queries_reuse_only_right_sized_scratch() {
        let values = (0..100)
            .map(|id| json!({"name":"same customer", "id":id}))
            .collect::<Vec<_>>();
        let mut index = make_index(&values, "name");
        let input = left(json!({"name":"same customer"}));
        let before = index.stats().unwrap();
        for _ in 0..1000 {
            let selection = index
                .select(&input, NonZeroUsize::new(20).unwrap(), Mode::Tokens)
                .unwrap();
            assert_eq!(selection.eligible_records, 100);
            assert_eq!(selection.candidates.len(), 20);
            assert_eq!(
                selection
                    .candidates
                    .iter()
                    .map(|candidate| candidate.right_index)
                    .collect::<Vec<_>>(),
                (1..=20).collect::<Vec<_>>()
            );
        }
        let after = index.stats().unwrap();
        assert_eq!(after.scratch_slots, before.records);
        assert_eq!(after.token_postings, before.token_postings);
        assert_eq!(after.band_postings, before.band_postings);
        index.generation = u64::MAX;
        assert_eq!(
            index
                .select(&input, NonZeroUsize::MIN, Mode::Hybrid)
                .unwrap()
                .candidates[0]
                .right_index,
            1
        );
        let none = index
            .select(
                &left(json!({"name":"unrelated"})),
                NonZeroUsize::MIN,
                Mode::Hybrid,
            )
            .unwrap();
        assert!(none.candidates.is_empty());
    }

    #[test]
    fn exact_fields_precede_jaccard_and_candidate_ownership_is_checked() {
        let values = [
            json!({"a":"x", "b":"one two three four five six"}),
            json!({"a":"x extra", "b":"one two extra"}),
            json!({"a":"x extra", "b":"one two extra"}),
        ];
        let mut index = make_index(&values, "a,b");
        let selection = index
            .select(
                &left(json!({"a":"x", "b":"one two"})),
                NonZeroUsize::new(3).unwrap(),
                Mode::Hybrid,
            )
            .unwrap();
        assert_eq!(
            selection
                .candidates
                .iter()
                .map(|candidate| candidate.right_index)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        let unrelated = make_index(&[json!({"a":"other", "b":"record"})], "a,b");
        assert_eq!(
            unrelated
                .unit(&selection.candidates[0])
                .unwrap_err()
                .exit_code(),
            65
        );
    }

    #[test]
    fn missing_nested_paths_empty_values_invalid_fields_and_modes_are_explicit() {
        assert!(Fields::parse("").is_err());
        assert!(Fields::parse("x,/x").is_err());
        assert!(Fields::parse("x,x").is_err());
        assert!("bad".parse::<Mode>().is_err());
        let fields = Fields::parse("/meta/0/name").unwrap();
        assert!(
            Index::load(
                Cursor::new(records(&[json!({"meta":[{"name":"A"}]})])),
                Options::new(Format::Jsonl),
                None,
                fields
            )
            .is_ok()
        );
        assert!(
            Index::load(
                Cursor::new(records(&[json!({"name":[]})])),
                Options::new(Format::Jsonl),
                None,
                Fields::parse("name").unwrap()
            )
            .is_err()
        );
        assert!(
            Index::load(
                Cursor::new(records(&[json!({"missing":"A"})])),
                Options::new(Format::Jsonl),
                None,
                Fields::parse("name").unwrap()
            )
            .is_err()
        );
        let mut index = make_index(
            &[
                json!({"name":null}),
                json!({"name":"---"}),
                json!({"name":""}),
            ],
            "name",
        );
        assert!(
            index
                .select(&left(json!({"name":null})), NonZeroUsize::MIN, Mode::Hybrid)
                .unwrap()
                .candidates
                .is_empty()
        );
        assert_eq!(index.stats().unwrap().band_postings, 0);
    }

    #[test]
    fn lsh_bands_are_order_independent_and_can_be_selected_without_token_fallback() {
        let mut index = make_index(
            &[
                json!({"name":"a b c"}),
                json!({"name":"a b c d"}),
                json!({"name":"unrelated"}),
            ],
            "name",
        );
        let input = left(json!({"name":"c a b"}));
        let exact = index
            .select(&input, NonZeroUsize::new(20).unwrap(), Mode::Lsh)
            .unwrap();
        assert_eq!(exact.candidates[0].right_index, 1);
        assert_eq!(exact.candidates[0].matching_lsh_bands, 8);
        let normalized = Fields::parse("name").unwrap().normalize(&input).unwrap();
        assert_eq!(bands(&normalized.tokens), normalized.bands);
        assert_eq!(normalized.bands.len(), 8);
        assert!(
            exact
                .candidates
                .iter()
                .all(|candidate| candidate.matching_lsh_bands > 0)
        );
    }

    #[test]
    fn quoted_multiline_table_rows_keep_source_locations_and_full_state() {
        let table =
            b"name,location,note\n\"Alpha, Inc\",London,\"first\nsecond\"\nBeta,Paris,normal\n";
        let mut index = Index::load(
            Cursor::new(table),
            Options::new(Format::Csv),
            Some("right.csv".into()),
            Fields::parse("name,location").unwrap(),
        )
        .unwrap();
        let input = left(json!({"name":"Alpha Inc", "location":"London"}));
        let selection = index
            .select(&input, NonZeroUsize::MIN, Mode::Hybrid)
            .unwrap();
        let unit = index.unit(&selection.candidates[0]).unwrap();
        assert_eq!(unit.state["note"], json!("first\nsecond"));
        assert_eq!(unit.location.line_start, 2);
        assert_eq!(unit.location.line_end, 3);
        assert_eq!(
            unit.location.source.as_ref().unwrap().to_str().unwrap(),
            "right.csv"
        );
    }
    #[test]
    fn claimed_records_are_excluded_before_the_candidate_cap() {
        let values = (0..25)
            .map(|id| json!({"id":id,"name":"same"}))
            .collect::<Vec<_>>();
        let mut index = make_index(&values, "name");
        let excluded = (1..=20).collect();
        let selection = index
            .select_available(
                &left(json!({"name":"same"})),
                NonZeroUsize::new(2).unwrap(),
                Mode::Hybrid,
                Some(&excluded),
            )
            .unwrap();
        assert_eq!(selection.eligible_records, 25);
        assert_eq!(selection.unavailable_records, 20);
        assert_eq!(
            selection
                .candidates
                .iter()
                .map(|candidate| candidate.right_index)
                .collect::<Vec<_>>(),
            vec![21, 22]
        );
    }
}
