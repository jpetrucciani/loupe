//! Stable numeric columns derived from validated, jointly evaluated answers.

use std::collections::BTreeSet;
use std::io::Write;

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::input::Unit;
use crate::pack::SourceKind;
use crate::policy::{Policy, Probability};
use crate::wire::{Answer, Question, Request, Response};

#[derive(Clone, Debug)]
pub struct Schema {
    questions: IndexMap<String, Question>,
    columns: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Values<'a> {
    pub features: IndexMap<String, f64>,
    pub answers: IndexMap<String, &'a Value>,
    pub bindings: IndexMap<String, String>,
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub ensembles: IndexMap<String, crate::ensemble::Report<'a>>,
}

#[derive(Clone, Copy)]
pub struct Ensemble<'a> {
    pub set: &'a crate::ensemble::Set,
    pub document: &'a Value,
    pub min_agreement: Probability,
}

impl Schema {
    pub fn new(questions: IndexMap<String, Question>) -> Result<Self> {
        if questions.is_empty() {
            return Err(Error::new(
                ErrorKind::Usage,
                "featurize needs at least one question",
            ));
        }
        let mut columns = Vec::new();
        for (id, question) in &questions {
            question.validate()?;
            match question {
                Question::Noul { .. } => columns.push(format!("p_{id}")),
                Question::Score { criteria, .. } => {
                    columns.push(format!("s_{id}"));
                    columns.extend((0..criteria.len()).map(|level| format!("p_{id}_{level}")));
                }
                Question::Choice { .. } => {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "featurize accepts only noul and score questions",
                    ));
                }
            }
        }
        if columns.iter().collect::<BTreeSet<_>>().len() != columns.len() {
            return Err(Error::new(
                ErrorKind::Usage,
                "feature question IDs produce duplicate column names",
            ));
        }
        Ok(Self { questions, columns })
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The first primary field is the pipeline's anchor for a row. Window
    /// packing either renames that sole field or prefixes every logical ID.
    pub fn values<'a>(
        &self,
        request: &Request,
        response: &'a Response,
        anchor: &str,
    ) -> Result<Values<'a>> {
        self.project(request, response, anchor, None)
    }

    pub fn project<'a>(
        &self,
        request: &Request,
        response: &'a Response,
        anchor: &str,
        ensemble: Option<Ensemble<'_>>,
    ) -> Result<Values<'a>> {
        let validated = response.validate(request)?;
        let primary = self
            .questions
            .first()
            .map(|(id, _)| id)
            .ok_or_else(|| Error::new(ErrorKind::Usage, "feature primary question missing"))?;
        let prefix = if anchor == primary {
            None
        } else if let Some(prefix) = anchor.strip_suffix(&format!("/{primary}")) {
            Some(prefix)
        } else if self.questions.len() == 1 && request.questions.contains_key(anchor) {
            None
        } else {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "invalid feature row binding",
            ));
        };
        let mut features = IndexMap::new();
        let mut answers = IndexMap::new();
        let mut bindings = IndexMap::new();
        let mut ensembles = IndexMap::new();
        for (id, question) in &self.questions {
            let field = if id == primary {
                anchor.to_owned()
            } else {
                prefix.map_or_else(|| id.clone(), |prefix| format!("{prefix}/{id}"))
            };
            let raw = validated.get(&field).ok_or_else(|| {
                Error::new(
                    ErrorKind::Unavailable,
                    format!("missing feature answer {field}"),
                )
            })?;
            let report = ensemble
                .filter(|ensemble| ensemble.set.has_primary(id))
                .map(|ensemble| {
                    ensemble.set.report(
                        id,
                        &field,
                        request,
                        response,
                        Policy::from_field(ensemble.document, id, question.kind())?,
                        ensemble.min_agreement,
                    )
                })
                .transpose()?;
            let derived = report
                .as_ref()
                .map(|report| report.mean_answer(question))
                .transpose()?;
            let answer = derived.as_ref().unwrap_or(raw);
            match (question, answer) {
                (Question::Noul { .. }, Answer::Noul { noul }) => {
                    features.insert(format!("p_{id}"), *noul);
                }
                (
                    Question::Score { criteria, .. },
                    Answer::Score {
                        score,
                        probabilities,
                        ..
                    },
                ) => {
                    features.insert(format!("s_{id}"), *score);
                    for level in 0..criteria.len() {
                        let probability =
                            probabilities.get(&level.to_string()).ok_or_else(|| {
                                Error::new(ErrorKind::Unavailable, "missing feature score level")
                            })?;
                        features.insert(format!("p_{id}_{level}"), *probability);
                    }
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::Unavailable,
                        "feature answer type does not match schema",
                    ));
                }
            }
            let raw = response
                .answers
                .get(&field)
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing raw feature answer"))?;
            answers.insert(id.clone(), raw);
            bindings.insert(id.clone(), field);
            if let Some(report) = report {
                ensembles.insert(id.clone(), report);
            }
        }
        Ok(Values {
            features,
            answers,
            bindings,
            ensembles,
        })
    }

    pub fn csv_header(&self, writer: &mut impl Write) -> Result<()> {
        csv_record(
            writer,
            std::iter::once("index")
                .chain(std::iter::once("row_id"))
                .chain(self.columns.iter().map(String::as_str))
                .chain(std::iter::once("error")),
        )
    }

    pub fn csv_row(
        &self,
        writer: &mut impl Write,
        index: u64,
        id: &Value,
        features: Option<&IndexMap<String, f64>>,
        error: Option<&str>,
    ) -> Result<()> {
        let mut record = vec![
            index.to_string(),
            id.as_str().map_or_else(|| id.to_string(), str::to_owned),
        ];
        for column in &self.columns {
            record.push(match features {
                Some(features) => features
                    .get(column)
                    .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing feature column"))?
                    .to_string(),
                None => String::new(),
            });
        }
        record.push(error.unwrap_or_default().to_owned());
        csv_record(writer, record)
    }
}

pub fn row_id(unit: &Unit, selected: bool, source: SourceKind) -> Result<Value> {
    if !selected {
        return Ok(Value::String(source.id(unit.index)));
    }
    match &unit.subject {
        Value::String(id) if !id.is_empty() => Ok(unit.subject.clone()),
        Value::Number(_) => Ok(unit.subject.clone()),
        _ => Err(Error::new(
            ErrorKind::Input,
            format!(
                "unit {}: featurize --field must select a nonempty string or number row ID",
                unit.index
            ),
        )),
    }
}

fn csv_record(
    writer: &mut impl Write,
    record: impl IntoIterator<Item = impl AsRef<[u8]>>,
) -> Result<()> {
    let mut csv = csv::WriterBuilder::new()
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    csv.write_record(record)
        .map_err(|error| Error::new(ErrorKind::Io, format!("encoding feature CSV: {error}")))?;
    let bytes = csv
        .into_inner()
        .map_err(|error| Error::new(ErrorKind::Io, format!("finishing feature CSV: {error}")))?;
    writer
        .write_all(&bytes)
        .map_err(|error| Error::io("writing feature CSV", error))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::features::{Schema, row_id};
    use crate::input::{InputFormat, Reader};
    use crate::pack::SourceKind;
    use crate::wire::{Question, Request, Response};
    use indexmap::IndexMap;
    use serde_json::{Value, json};
    use std::io::Cursor;

    #[test]
    fn ensemble_means_keep_feature_columns_raw_answers_and_window_binding() {
        // Probability arithmetic only, no inference backend.
        let questions: IndexMap<String, Question> = IndexMap::from([
            (
                "outage".into(),
                serde_json::from_value(json!({"type":"noul","instructions":"Is checkout down?"}))
                    .unwrap(),
            ),
            (
                "impact".into(),
                serde_json::from_value(
                    json!({"type":"score","criteria":["None","Partial","Complete"]}),
                )
                .unwrap(),
            ),
        ]);
        let document = json!({"variants":[
            {"id":"direct","field":"outage"},
            {"id":"direct","field":"impact"}
        ]});
        let set = crate::ensemble::Set::from_document(&document, &questions).unwrap();
        let schema = Schema::new(questions.clone()).unwrap();
        let mut expanded = questions;
        set.append(&mut expanded).unwrap();
        let request = Request {
            model: "arithmetic-only".into(),
            state: json!("numeric projection"),
            questions: expanded
                .into_iter()
                .map(|(id, question)| (format!("L7/{id}"), question))
                .collect(),
        };
        let legend = json!({"0":"None","1":"Partial","2":"Complete"});
        let response: Response = serde_json::from_value(json!({
            "model":"arithmetic-only","usage":{"input_tokens":0,"output_tokens":0},
            "answers":{
                "L7/outage":{"type":"noul","noul":0.8},
                "L7/impact":{"type":"score","score":1.5,"confidence":0.5,"legend":legend,"probabilities":{"0":0.0,"1":0.5,"2":0.5}},
                "L7/ensemble:outage/direct":{"type":"noul","noul":0.2},
                "L7/ensemble:impact/direct":{"type":"score","score":0.5,"confidence":0.5,"legend":legend,"probabilities":{"0":0.5,"1":0.5,"2":0.0}}
            }
        }))
        .unwrap();
        let original = serde_json::to_value(&response).unwrap();
        let settings = crate::features::Ensemble {
            set: &set,
            document: &document,
            min_agreement: crate::policy::Probability::new(1.0).unwrap(),
        };
        let values = schema
            .project(&request, &response, "L7/outage", Some(settings))
            .unwrap();
        assert_eq!(values.features["p_outage"], 0.5);
        assert_eq!(values.features["s_impact"], 1.0);
        assert_eq!(values.features["p_impact_0"], 0.25);
        assert_eq!(values.features["p_impact_1"], 0.5);
        assert_eq!(values.features["p_impact_2"], 0.25);
        assert!(values.ensembles["outage"].assessment.uncertain);
        assert!(values.ensembles["impact"].assessment.uncertain);
        assert_eq!(values.ensembles["outage"].agreement.get(), 0.5);
        assert_eq!(values.bindings["impact"], "L7/impact");
        assert_eq!(values.answers["impact"]["score"], 1.5);
        assert_eq!(
            values.features.keys().collect::<Vec<_>>(),
            schema.columns().iter().collect::<Vec<_>>()
        );
        assert_eq!(serde_json::to_value(&response).unwrap(), original);
        assert!(
            schema
                .project(&request, &response, "L8/outage", Some(settings))
                .is_err()
        );
        let raw = schema.values(&request, &response, "L7/outage").unwrap();
        assert_eq!(raw.features["p_outage"], 0.8);
        assert_eq!(raw.features["s_impact"], 1.5);
        assert!(raw.ensembles.is_empty());
    }

    #[test]
    fn saved_actual_distribution_projects_without_changing_score_or_probabilities() {
        let evidence: Value =
            serde_json::from_str(include_str!("../testdata/captured-response.json")).unwrap();
        let request: Request = serde_json::from_value(evidence["request"].clone()).unwrap();
        let mut response: Response = serde_json::from_value(evidence["response"].clone()).unwrap();
        let schema = Schema::new(
            request
                .questions
                .iter()
                .filter(|(id, _)| id.as_str() != "severity")
                .map(|(id, q)| (id.clone(), q.clone()))
                .collect(),
        )
        .unwrap();
        assert_eq!(
            schema.columns(),
            &[
                "p_outage",
                "s_urgency",
                "p_urgency_0",
                "p_urgency_1",
                "p_urgency_2"
            ]
        );
        let values = schema.values(&request, &response, "outage").unwrap();
        assert_eq!(values.features["p_outage"], 0.9654);
        assert_eq!(values.features["s_urgency"], 1.8972);
        assert_eq!(values.features["p_urgency_2"], 0.9226);
        assert_eq!(
            *values.answers["urgency"],
            evidence["response"]["answers"]["urgency"]
        );
        let mut bytes = Vec::new();
        schema.csv_header(&mut bytes).unwrap();
        schema
            .csv_row(
                &mut bytes,
                7,
                &json!("é,\"id\"\nnext"),
                Some(&values.features),
                None,
            )
            .unwrap();
        schema
            .csv_row(
                &mut bytes,
                8,
                &json!(19),
                None,
                Some("error, details\nnext"),
            )
            .unwrap();
        let mut csv = csv::Reader::from_reader(bytes.as_slice());
        assert_eq!(
            csv.headers().unwrap().iter().collect::<Vec<_>>(),
            vec![
                "index",
                "row_id",
                "p_outage",
                "s_urgency",
                "p_urgency_0",
                "p_urgency_1",
                "p_urgency_2",
                "error"
            ]
        );
        let records = csv
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(&records[0][1], "é,\"id\"\nnext");
        for (offset, value) in values.features.values().enumerate() {
            assert_eq!(records[0][offset + 2].parse::<f64>().unwrap(), *value);
        }
        assert_eq!(&records[1][1], "19");
        assert!(
            records[1]
                .iter()
                .skip(2)
                .take(schema.columns().len())
                .all(str::is_empty)
        );
        assert_eq!(&records[1][7], "error, details\nnext");
        response.answers.shift_remove("severity");
        assert_eq!(
            schema
                .values(&request, &response, "outage")
                .unwrap_err()
                .exit_code(),
            69
        );
    }

    #[test]
    fn choices_and_colliding_columns_are_rejected_before_any_rows() {
        let score = Question::Score {
            instructions: None,
            criteria: vec![json!("low"), json!("high")],
        };
        let noul = Question::Noul {
            instructions: None,
            criteria: None,
        };
        assert_eq!(
            Schema::new(IndexMap::from([
                ("x".into(), score.clone()),
                ("x_0".into(), noul.clone())
            ]))
            .unwrap_err()
            .exit_code(),
            64
        );
        assert!(Schema::new(IndexMap::new()).is_err());
        assert!(
            Schema::new(IndexMap::from([(
                "x".into(),
                Question::Choice {
                    instructions: None,
                    criteria: IndexMap::from([("a".into(), json!("A"))])
                }
            )]))
            .is_err()
        );
        let schema =
            Schema::new(IndexMap::from([("z,\"".into(), noul), ("a".into(), score)])).unwrap();
        let mut bytes = Vec::new();
        schema.csv_header(&mut bytes).unwrap();
        let mut reader = csv::Reader::from_reader(bytes.as_slice());
        assert_eq!(&reader.headers().unwrap()[2], "p_z,\"");
    }

    #[test]
    fn row_ids_preserve_scalar_type_and_source_index_without_global_state() {
        let mut reader = Reader::new(Cursor::new(b"x\n"), InputFormat::Lines, None, true);
        let mut unit = reader.next_unit().unwrap().unwrap();
        unit.index = 5;
        assert_eq!(
            row_id(&unit, false, SourceKind::Records).unwrap(),
            json!("R5")
        );
        assert_eq!(
            row_id(&unit, false, SourceKind::Lines).unwrap(),
            json!("L5")
        );
        for id in [json!("é"), json!(0), json!(18446744073709551615u64)] {
            unit.subject = id.clone();
            assert_eq!(row_id(&unit, true, SourceKind::Records).unwrap(), id);
        }
        for id in [Value::Null, json!(true), json!([]), json!({}), json!("")] {
            unit.subject = id;
            assert_eq!(
                row_id(&unit, true, SourceKind::Records)
                    .unwrap_err()
                    .exit_code(),
                65
            );
        }
    }
}
