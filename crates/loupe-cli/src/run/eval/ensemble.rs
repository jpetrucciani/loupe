use std::collections::BTreeSet;

use indexmap::IndexMap;
use loupe_core::ensemble::Set;
use loupe_core::error::Result;
use loupe_core::wire::{Answer, Question};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::args::Eval;
use crate::run::eval::{Record, json_error, usage};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    min_agreement: f64,
    primaries: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Saved {
    pub(crate) settings: Settings,
    pub(crate) reports: IndexMap<String, Value>,
}

pub(crate) fn prepare(
    options: &Eval,
    document: &Value,
    questions: &mut IndexMap<String, Question>,
) -> Result<(Option<Settings>, Set)> {
    if !options.ensemble {
        return Ok((None, Set::default()));
    }
    let set = Set::from_document(document, questions)?;
    if set.is_empty() {
        return Err(usage("--ensemble requires named preset variants"));
    }
    let settings = Settings {
        min_agreement: loupe_core::ensemble::minimum(document, options.min_agreement)?.get(),
        primaries: questions.keys().cloned().collect(),
    };
    set.append(questions)?;
    Ok((Some(settings), set))
}

impl Settings {
    pub(crate) fn set(
        &self,
        document: &Value,
        questions: &IndexMap<String, Question>,
    ) -> Result<Set> {
        loupe_core::policy::Probability::new(self.min_agreement)?;
        let mut primaries = IndexMap::new();
        for id in &self.primaries {
            let question = questions
                .get(id)
                .ok_or_else(|| usage("ensemble primary is missing"))?;
            if primaries.insert(id.clone(), question.clone()).is_some() {
                return Err(usage("ensemble primary IDs are duplicated"));
            }
        }
        let set = Set::from_document(document, &primaries)?;
        if set.is_empty() {
            return Err(usage("saved ensemble has no variants"));
        }
        set.append(&mut primaries)?;
        for (id, question) in primaries {
            let actual = questions
                .get(&id)
                .ok_or_else(|| usage("saved ensemble question is missing"))?;
            if question.kind() != actual.kind()
                || question
                    .options()
                    .iter()
                    .map(|(key, _)| key)
                    .ne(actual.options().iter().map(|(key, _)| key))
            {
                return Err(usage(
                    "saved ensemble options differ from its declared schema",
                ));
            }
        }
        Ok(set)
    }

    pub(crate) fn saved(&self) -> Saved {
        Saved {
            settings: self.clone(),
            reports: IndexMap::new(),
        }
    }
}

pub(crate) fn set(record: &Record) -> Result<Set> {
    record
        .ensemble
        .as_ref()
        .map(|saved| {
            saved
                .settings
                .set(&record.effective_document, &record.questions)
        })
        .transpose()
        .map(|set| set.unwrap_or_default())
}

/// Compute summaries from raw answers. Never mutate the retained API response.
fn summarize(record: &Record) -> Result<(IndexMap<String, Value>, IndexMap<String, Answer>)> {
    let evaluation = record
        .evaluation
        .as_ref()
        .ok_or_else(|| usage("ensemble report has no actual answer"))?;
    let mut answers = evaluation.response.validate(&record.request)?;
    let Some(saved) = &record.ensemble else {
        return Ok((IndexMap::new(), answers));
    };
    let set = set(record)?;
    let minimum = loupe_core::policy::Probability::new(saved.settings.min_agreement)?;
    let mut reports = IndexMap::new();
    for primary in &saved.settings.primaries {
        if !set.has_primary(primary) {
            continue;
        }
        let bound = record
            .binding
            .get(primary)
            .ok_or_else(|| usage("ensemble primary binding is missing"))?;
        for field in set.fields(primary, primary)? {
            let binding = record
                .binding
                .get(&field)
                .ok_or_else(|| usage("ensemble member binding is missing"))?;
            if !set.fields(primary, bound)?.contains(binding) {
                return Err(usage("ensemble member is bound to a different window"));
            }
        }
        let policy = record
            .policies
            .get(primary)
            .ok_or_else(|| usage("ensemble primary policy is missing"))?
            .policy()?;
        let report = set.report(
            primary,
            bound,
            &record.request,
            &evaluation.response,
            policy,
            minimum,
        )?;
        let question = record
            .request
            .questions
            .get(bound)
            .ok_or_else(|| usage("ensemble bound question is missing"))?;
        answers.insert(bound.clone(), report.mean_answer(question)?);
        reports.insert(
            primary.clone(),
            serde_json::to_value(report).map_err(json_error)?,
        );
    }
    Ok((reports, answers))
}

pub(crate) fn capture(record: &mut Record) -> Result<()> {
    if record.ensemble.is_some() && record.evaluation.is_some() {
        let (reports, _) = summarize(record)?;
        if let Some(saved) = &mut record.ensemble {
            saved.reports = reports;
        }
    }
    Ok(())
}

pub(crate) fn answers(record: &Record) -> Result<IndexMap<String, Answer>> {
    let (reports, answers) = summarize(record)?;
    if let Some(saved) = &record.ensemble
        && saved.reports != reports
    {
        return Err(usage(
            "saved ensemble summaries differ from the raw answers or policy",
        ));
    }
    Ok(answers)
}

pub(crate) fn uncertain(record: &Record, primary: &str) -> Result<bool> {
    record
        .ensemble
        .as_ref()
        .and_then(|saved| saved.reports.get(primary))
        .map(|report| {
            report
                .get("agreement_uncertain")
                .and_then(Value::as_bool)
                .ok_or_else(|| usage("ensemble uncertainty metadata is missing"))
        })
        .transpose()
        .map(|value| value.unwrap_or(false))
}

pub(crate) fn calibration(
    record: &Record,
) -> Result<Option<loupe_core::calibration_pin::Ensemble>> {
    record
        .ensemble
        .as_ref()
        .map(|saved| {
            Ok(loupe_core::calibration_pin::Ensemble {
                min_agreement: loupe_core::policy::Probability::new(saved.settings.min_agreement)?,
            })
        })
        .transpose()
}

pub(crate) fn uncertainty_cases<'a>(
    records: &'a [Record],
    field: &str,
    selected: &[usize],
) -> Result<BTreeSet<&'a str>> {
    let mut cases = BTreeSet::new();
    for index in selected {
        let record = records
            .get(*index)
            .ok_or_else(|| usage("ensemble metric case is missing"))?;
        if uncertain(record, field)? {
            cases.insert(record.case_id.as_str());
        }
    }
    Ok(cases)
}

pub(crate) fn primary_questions(record: &Record) -> Result<IndexMap<String, Question>> {
    let signals = loupe_core::signals::Set::from_document(&record.effective_document)?;
    Ok(set(record)?.primary(&signals.primary(&record.questions)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
pub(crate) mod tests {
    use clap::Parser;
    use indexmap::IndexMap;
    use loupe_core::backend::Evaluation;
    use loupe_core::policy::{Policy, Selected};
    use loupe_core::wire::{Answer, Question, Request, Response};
    use serde_json::json;

    use crate::args::{Cli, Command};
    use crate::run::eval::ensemble::{answers, capture, prepare, primary_questions, uncertain};
    use crate::run::eval::{PolicyData, Record, RequestRole};

    pub(crate) fn arithmetic_record() -> Record {
        // Pure math/schema/codec fixture. No backend and no fresh inference claim.
        let document = json!({"questions":{
            "present":{"type":"noul","instructions":"Arithmetic Noul fixture"},
            "topic":{"type":"choice","criteria":{"a":"A","b":"B"}},
            "impact":{"type":"score","criteria":["Low","High"]}
        },"variants":[{"field":"present","id":"direct"},{"field":"topic","id":"direct"},{"field":"impact","id":"direct"}]});
        let mut questions: IndexMap<String, Question> =
            serde_json::from_value(document["questions"].clone()).unwrap();
        let Command::Eval(options) =
            Cli::try_parse_from(["loupe", "eval", "--ensemble", "--dry-run"])
                .unwrap()
                .command
        else {
            panic!("eval grammar")
        };
        let (settings, _) = prepare(&options, &document, &mut questions).unwrap();
        let request = Request {
            model: "mathematical-fixture".into(),
            state: json!("arithmetic codec fixture; no backend"),
            questions: questions.clone(),
        };
        let raw = json!({
            "present":{"type":"noul","noul":0.99},
            "ensemble:present/direct":{"type":"noul","noul":0.4},
            "topic":{"type":"choice","choice":"a","confidence":0.7,"probabilities":{"a":0.7,"b":0.3}},
            "ensemble:topic/direct":{"type":"choice","choice":"b","confidence":1.0,"probabilities":{"a":0.0,"b":1.0}},
            "impact":{"type":"score","score":0.0,"confidence":1.0,"legend":{"0":"Low","1":"High"},"probabilities":{"0":1.0,"1":0.0}},
            "ensemble:impact/direct":{"type":"score","score":1.0,"confidence":1.0,"legend":{"0":"Low","1":"High"},"probabilities":{"0":0.0,"1":1.0}}
        });
        let response = Response {
            model: request.model.clone(),
            answers: serde_json::from_value(raw).unwrap(),
            usage: loupe_core::wire::Usage {
                input_tokens: 0,
                output_tokens: 0,
            },
        };
        let mut record = Record {
            ensemble: settings.map(|settings| settings.saved()),
            case_id: "arithmetic".into(),
            index: 7,
            label: serde_json::from_value(json!({"present":true,"topic":"b","impact":1})).unwrap(),
            source: "mathematical codec fixture".into(),
            reviewed: false,
            sampling: None,
            preset: json!({"document":document}),
            effective_document: document,
            policies: questions
                .keys()
                .map(|id| (id.clone(), PolicyData::from(Policy::default())))
                .collect(),
            base_question_order: questions.keys().cloned().collect(),
            binding: questions
                .keys()
                .map(|id| (id.clone(), id.clone()))
                .collect(),
            questions,
            request_question_order: request.questions.keys().cloned().collect(),
            counts: loupe_core::encoder::estimate(&request).unwrap(),
            request,
            request_role: RequestRole::Evaluated,
            evaluation: Some(Evaluation {
                response,
                warning: None,
                cached: false,
            }),
            packing: json!({"declared":"unit"}),
            provenance: json!({"fixture":"pure arithmetic"}),
            reference: None,
        };
        capture(&mut record).unwrap();
        record
    }

    #[test]
    fn means_drive_metrics_without_changing_raw_answers_and_agreement_limits_coverage() {
        let record = arithmetic_record();
        let before = serde_json::to_value(&record.evaluation).unwrap();
        let derived = answers(&record).unwrap();
        let Answer::Noul { noul } = derived["present"] else {
            panic!("Noul")
        };
        assert!((noul - 0.695).abs() < 1e-12);
        assert_eq!(
            derived["topic"]
                .assessment(&Policy::default())
                .unwrap()
                .selected,
            Selected::Choice("b".into())
        );
        assert_eq!(
            derived["impact"]
                .assessment(&Policy::default())
                .unwrap()
                .selected,
            Selected::Score(0.5)
        );
        assert!(uncertain(&record, "present").unwrap());
        assert!(uncertain(&record, "topic").unwrap());
        let reports = crate::run::eval::question_reports(&[record]).unwrap();
        assert_eq!(reports.len(), 3);
        for report in &reports {
            assert_eq!(report.metrics.coverage.value, Some(0.0));
            assert!(
                report
                    .metrics
                    .confidence_gates
                    .iter()
                    .all(|gate| gate.coverage.value == Some(0.0))
            );
        }
        let record = arithmetic_record();
        assert_eq!(serde_json::to_value(&record.evaluation).unwrap(), before);
        let mut saved: Record =
            serde_json::from_value(serde_json::to_value(&record).unwrap()).unwrap();
        answers(&saved).unwrap();
        saved.ensemble.as_mut().unwrap().settings.min_agreement = 0.5;
        assert!(answers(&saved).is_err());
        capture(&mut saved).unwrap();
        assert!(!uncertain(&saved, "present").unwrap());
        assert!(!uncertain(&saved, "topic").unwrap());
        assert!(uncertain(&saved, "impact").unwrap());
        let reports = crate::run::eval::question_reports(&[saved]).unwrap();
        let present = reports
            .iter()
            .find(|report| report.field == "present")
            .unwrap();
        assert_eq!(present.metrics.accuracy.value, Some(1.0));
        assert_eq!(present.metrics.coverage.value, Some(1.0));
        assert!((present.metrics.brier.value.unwrap() - 0.305f64.powi(2)).abs() < 1e-12);
        let impact = reports
            .iter()
            .find(|report| report.field == "impact")
            .unwrap();
        assert_eq!(impact.metrics.mae.value, Some(0.5));
    }

    #[test]
    fn summaries_and_window_bindings_are_verified_and_primary_fields_exclude_variants() {
        let mut record = arithmetic_record();
        record.binding = record
            .questions
            .keys()
            .map(|id| (id.clone(), format!("R7/{id}")))
            .collect();
        record.request.questions = record
            .questions
            .iter()
            .map(|(id, question)| (format!("R7/{id}"), question.clone()))
            .collect();
        record.request_question_order = record.request.questions.keys().cloned().collect();
        let evaluation = record.evaluation.as_mut().unwrap();
        evaluation.response.answers = evaluation
            .response
            .answers
            .iter()
            .map(|(id, answer)| (format!("R7/{id}"), answer.clone()))
            .collect();
        capture(&mut record).unwrap();
        answers(&record).unwrap();
        assert_eq!(
            primary_questions(&record)
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["present", "topic", "impact"]
        );
        record.ensemble.as_mut().unwrap().reports["present"]["agreement"] = json!(1.0);
        assert!(answers(&record).is_err());
        capture(&mut record).unwrap();
        record.binding.insert(
            "ensemble:present/direct".into(),
            "R8/ensemble:present/direct".into(),
        );
        assert!(answers(&record).is_err());
        record.binding.insert(
            "ensemble:present/direct".into(),
            "R7/ensemble:present/direct".into(),
        );
        record
            .ensemble
            .as_mut()
            .unwrap()
            .settings
            .primaries
            .push("present".into());
        assert!(answers(&record).is_err());
    }
}
