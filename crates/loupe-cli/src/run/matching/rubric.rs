use crate::args::Join;
use crate::run::json_error;
use indexmap::IndexMap;
use loupe_core::config::Profile;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::matching::{GATE_ID, SCORE_ID, Template};
use loupe_core::policy::{Policy, Probability};
use loupe_core::preset_store::{Origin, Preset, Store};
use loupe_core::wire::{Question, Request};
use loupe_core::{signals, templates};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(crate) fn prepare(
    options: &Join,
    store: &Store,
    profile: &Profile,
    fields: &[String],
) -> Result<(Template, Value)> {
    compile(
        Settings {
            preset: options.preset.as_deref(),
            question_file: options.question_file.as_deref(),
            arg: &options.arg,
            context: options.context.as_deref(),
            threshold: options.threshold,
            margin: options.margin,
            min_confidence: options.min_confidence,
            min_gap: options.min_gap,
            ensemble: options.ensemble,
            min_agreement: options.min_agreement,
        },
        store,
        profile,
        fields,
    )
}

pub(crate) fn prepare_eval(
    options: &crate::args::Eval,
    preset: &str,
    store: &Store,
    profile: &Profile,
    fields: &[String],
) -> Result<(Template, Value)> {
    compile(
        Settings {
            preset: (preset != "join-default").then_some(preset),
            question_file: None,
            arg: &options.arg,
            context: None,
            threshold: options.threshold,
            margin: options.margin,
            min_confidence: options.min_confidence,
            min_gap: options.min_gap,
            ensemble: options.ensemble,
            min_agreement: options.min_agreement,
        },
        store,
        profile,
        fields,
    )
}

struct Settings<'a> {
    preset: Option<&'a str>,
    question_file: Option<&'a std::path::Path>,
    arg: &'a [String],
    context: Option<&'a std::path::Path>,
    threshold: Option<f64>,
    margin: Option<f64>,
    min_confidence: Option<f64>,
    min_gap: Option<f64>,
    ensemble: bool,
    min_agreement: Option<f64>,
}

fn compile(
    options: Settings<'_>,
    store: &Store,
    profile: &Profile,
    fields: &[String],
) -> Result<(Template, Value)> {
    let loaded = if let Some(name) = &options.preset {
        Some(store.lookup(name)?)
    } else if let Some(path) = &options.question_file {
        Some(store.explicit(path)?)
    } else {
        None
    };
    if let Some(preset) = &loaded {
        preset.require_trusted()?;
        preset.validate(&profile.limits, &profile.model)?;
    }
    let mut questions = IndexMap::from([
        (
            SCORE_ID.to_owned(),
            Question::Score {
                instructions: Some(
                    json!({"task":"How strong is the evidence that left.record and right.record identify the same real-world entity? Compare the configured identity fields; shared words alone are insufficient.","identity_fields":fields}),
                ),
                criteria: vec![
                    json!("Different entities or incompatible identity fields"),
                    json!("Ambiguous possible match"),
                    json!("Clear match, allowing spelling, case and punctuation variations"),
                ],
            },
        ),
        (
            GATE_ID.to_owned(),
            Question::Noul {
                instructions: Some(
                    json!({"task":"Do left.record and right.record identify the same real-world entity using the configured identity fields? A conflict in an identity field means different entities. Shared words alone are insufficient.","identity_fields":fields}),
                ),
                criteria: None,
            },
        ),
    ]);
    for (position, field) in fields.iter().enumerate() {
        questions.insert(format!("field_{}",position+1),Question::Noul{instructions:Some(json!({"task":"Do these identity field values agree, allowing equivalent spellings and formatting? Missing information is not agreement.","field":field,"left_record":"left.record","right_record":"right.record"})),criteria:None});
    }
    let preset = loaded.unwrap_or(Preset {
        name: "join-default".into(),
        document: json!({"questions":questions}),
        origin: Origin::Builtin,
        trusted: true,
        content_hash: String::new(),
    });
    let args = options
        .arg
        .iter()
        .map(|arg| {
            arg.split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .ok_or_else(|| Error::new(ErrorKind::Usage, "--arg requires NAME=VALUE"))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    let document = templates::expand(&preset, store, &args)?;
    if !loupe_core::factors::Set::from_document(&document)?.is_empty() {
        return Err(Error::new(
            ErrorKind::Usage,
            "join pair rubrics do not apply explain composite factors",
        ));
    }
    if let Some(policy) = document.get("policy")
        && (policy.get("splittable") == Some(&Value::Bool(true))
            || policy.get("combine").is_some()
            || policy
                .get("pack")
                .is_some_and(|pack| pack.as_str() != Some("unit")))
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "join evaluates whole pair records in unit mode; splitting/combine/window preset policies are inapplicable",
        ));
    }
    if loupe_core::calibration_pin::validate(&document)?.is_some() {
        return Err(Error::new(
            ErrorKind::Usage,
            "join requires a pair-calibrated policy; classification calibration pins are inapplicable",
        ));
    }
    let mut questions: IndexMap<String, Question> = serde_json::from_value(
        document
            .get("questions")
            .ok_or_else(|| Error::new(ErrorKind::Usage, "pair rubric needs a questions map"))?
            .clone(),
    )
    .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
    let signals = signals::Set::from_document(&document)?;
    signals.append(&mut questions)?;
    let cli_context = options
        .context
        .map(|path| {
            std::fs::read_to_string(path)
                .map(Value::String)
                .map_err(|error| Error::io("reading join context", error))
        })
        .transpose()?;
    let context = match (document.get("context"), cli_context) {
        (Some(preset), Some(cli)) => Some(json!({"preset":preset,"cli":cli})),
        (Some(preset), None) => Some(preset.clone()),
        (None, cli) => cli,
    };
    let mut policies = IndexMap::new();
    for (id, question) in &questions {
        let mut policy = if let Some(signal) = signals.get(id) {
            signal.policy()
        } else {
            Policy::from_field(&document, id, question.kind())?
        };
        if id == GATE_ID {
            if let Some(threshold) = options.threshold {
                policy.threshold = Probability::new(threshold)
                    .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
            }
            if let Some(margin) = options.margin {
                policy.margin = margin;
            }
        }
        if id == SCORE_ID {
            if let Some(confidence) = options.min_confidence {
                policy.min_confidence = Probability::new(confidence)
                    .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
            }
            if let Some(gap) = options.min_gap {
                policy.min_gap = gap;
            }
        }
        policy.validate()?;
        policies.insert(id.clone(), policy);
    }
    let ensemble = if options.ensemble {
        Some(loupe_core::ensemble::Set::from_document(
            &document, &questions,
        )?)
    } else {
        None
    };
    let mut template = Template::new(profile.model.clone(), questions, policies)?;
    if let Some(ensemble) = ensemble {
        template = template.with_ensemble(
            ensemble,
            loupe_core::ensemble::minimum(&document, options.min_agreement)?,
        )?;
    }
    if let Some(context) = context {
        template.add_context(context)?;
    }
    profile.limits.validate_schema(&Request {
        model: profile.model.clone(),
        state: Value::Null,
        questions: template.questions().clone(),
    })?;
    let mut effective = document.clone();
    effective["compiled_questions"] =
        serde_json::to_value(template.questions().iter().collect::<Vec<_>>())
            .map_err(json_error)?;
    Ok((template, effective))
}
