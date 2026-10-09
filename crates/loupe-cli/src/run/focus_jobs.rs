use std::sync::Arc;

use futures::{StreamExt, TryStreamExt};
use serde_json::json;

use loupe_core::backend::Evaluation;
use loupe_core::encoder::Counts;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::focus;
use loupe_core::input::Unit;
use loupe_core::pack::{Mode as PackMode, Window};
use loupe_core::split;
use loupe_core::wire::Request;

use crate::run::failure::{WorkError, WorkResult};
use crate::run::{Completed, Execution, Job, json_error, unit_request};

pub(crate) struct FocusSettings {
    pub(crate) k: std::num::NonZeroUsize,
    pub(crate) options: split::Options,
    pub(crate) path: String,
}

struct FocusReady {
    unit: Unit,
    plan: focus::Plan,
    selection: focus::Selection,
    responses: Vec<Evaluation>,
    answer_id: String,
}

pub(crate) struct FocusFinal {
    units: Vec<FocusReady>,
    request: Arc<Request>,
    counts: Arc<Counts>,
    window: bool,
}

enum FocusPrepared {
    Plan(Box<Completed>),
    Ready(Box<FocusReady>),
}

async fn prepare_focus(unit: Unit, execution: &Execution) -> Result<FocusPrepared> {
    let settings = execution
        .focus
        .as_ref()
        .ok_or_else(|| Error::new(ErrorKind::Usage, "focus settings missing"))?;
    let encoder = execution
        .encoder
        .as_deref()
        .ok_or_else(|| Error::new(ErrorKind::Usage, "focus requires a verified tokenizer"))?;
    let window = matches!(execution.pack, PackMode::Window(_));
    let (request, path) = if window {
        let packed = Window {
            encoder: Some(encoder),
            limits: &execution.limits,
            questions: &execution.questions,
            model: &execution.model,
            contextual: execution.contextual,
            source: execution.source,
        }
        .single(&unit)?;
        (
            packed.request,
            format!(
                "/units/{}{}",
                execution.source.id(unit.index),
                settings.path
            ),
        )
    } else {
        (unit_request(&unit, execution), settings.path.clone())
    };
    let answer_id = request
        .questions
        .first()
        .map(|(id, _)| id.clone())
        .ok_or_else(|| Error::new(ErrorKind::Usage, "focus question missing"))?;
    let plan = focus::plan(
        encoder,
        &request,
        &path,
        settings.k,
        &execution.limits,
        &settings.options,
    )
    .map_err(|error| {
        Error::new(
            error.kind,
            format!("unit {} focus plan: {error}", unit.index),
        )
    })?;
    let Some(backend) = &execution.backend else {
        let focus_plan = serde_json::to_value(&plan).map_err(json_error)?;
        let index = unit.index;
        return Ok(FocusPrepared::Plan(Box::new(Completed {
            unit,
            request: Arc::new(request),
            counts: Arc::new(plan.original_counts),
            evaluation: None,
            answer_id,
            packed_indices: window.then_some(vec![index]),
            split_plan: None,
            view: None,
            focus_plan: Some(focus_plan),
            fallback: None,
            ensemble: None,
            review: None,
        })));
    };
    execution.record_plan(
        vec![unit.index],
        &request,
        &plan.original_counts,
        json!({"operation":"focus_relevance","plan":plan}),
    )?;
    let responses = futures::stream::iter(&plan.relevance)
        .map(|chunk| async move {
            backend
                .evaluate(
                    &chunk.request,
                    &chunk.counts,
                    &execution.budget,
                    execution.strict,
                    execution.deadline,
                )
                .await
        })
        .buffered(execution.concurrency)
        .try_collect::<Vec<_>>()
        .await
        .map_err(|error| {
            Error::new(
                error.kind,
                format!("unit {} focus relevance: {error}", unit.index),
            )
        })?;
    let raw = responses
        .iter()
        .map(|evaluation| evaluation.response.clone())
        .collect::<Vec<_>>();
    let selection = focus::select(encoder, &plan, &raw, &execution.limits)
        .map_err(|error| Error::new(error.kind, format!("unit {}: {error}", unit.index)))?;
    execution.record_plan(vec![unit.index],&selection.request,&selection.counts,json!({"operation":"focus_selected","plan":plan,"selection":selection,"relevance_responses":raw}))?;
    Ok(FocusPrepared::Ready(Box::new(FocusReady {
        unit,
        plan,
        selection,
        responses,
        answer_id,
    })))
}

pub(crate) async fn plan_focus_jobs(
    records: Vec<Result<Unit>>,
    execution: &Execution,
) -> Vec<WorkResult<Job>> {
    let prepared = futures::stream::iter(records)
        .map(|record| async move {
            let unit = record?;
            prepare_focus(unit.clone(), execution)
                .await
                .map_err(|error| WorkError::at(error, &unit))
        })
        .buffered(execution.concurrency)
        .collect::<Vec<_>>()
        .await;
    let mut jobs = Vec::new();
    let mut ready = Vec::new();
    for record in prepared {
        match record {
            Ok(FocusPrepared::Plan(record)) => jobs.push(Ok(Job::FocusPlan(record))),
            Ok(FocusPrepared::Ready(record)) => ready.push(*record),
            Err(error) => {
                if let Err(error) =
                    pack_focus_jobs(std::mem::take(&mut ready), execution, &mut jobs)
                {
                    jobs.push(Err(error));
                    return jobs;
                }
                let continue_run = execution.keep_going && error.recoverable();
                jobs.push(Err(error));
                if !continue_run {
                    return jobs;
                }
            }
        }
    }
    if let Err(error) = pack_focus_jobs(ready, execution, &mut jobs) {
        jobs.push(Err(error));
    }
    jobs
}

fn pack_focus_jobs(
    mut units: Vec<FocusReady>,
    execution: &Execution,
    jobs: &mut Vec<WorkResult<Job>>,
) -> WorkResult<()> {
    let encoder = execution
        .encoder
        .as_deref()
        .ok_or_else(|| Error::new(ErrorKind::Usage, "focus encoder missing"))?;
    let window = matches!(execution.pack, PackMode::Window(_));
    'packing: while !units.is_empty() {
        let mut length = if window { units.len() } else { 1 };
        let (request, counts) = loop {
            let members = units
                .get(..length)
                .ok_or_else(|| Error::new(ErrorKind::Input, "focus window out of range"))?;
            let request = if window {
                focus::merge(
                    &members
                        .iter()
                        .map(|unit| &unit.selection.request)
                        .collect::<Vec<_>>(),
                )?
            } else {
                members
                    .first()
                    .ok_or_else(|| Error::new(ErrorKind::Input, "focus unit missing"))?
                    .selection
                    .request
                    .clone()
            };
            let counts = encoder.encode(&request)?.1;
            match execution
                .limits
                .validate_request(&request)
                .and_then(|_| counts.preflight(&execution.limits))
            {
                Ok(()) => break (request, counts),
                Err(error) if error.kind == ErrorKind::Input && length > 1 => {
                    length = (length / 2).max(1)
                }
                Err(error) => {
                    let error = Error::new(
                        error.kind,
                        format!("focus final window reached size 1: {error}"),
                    );
                    let unit = units.remove(0);
                    let error = WorkError::at(error, &unit.unit);
                    if execution.keep_going && error.recoverable() {
                        jobs.push(Err(error));
                        continue 'packing;
                    }
                    return Err(error);
                }
            }
        };
        let members = units.drain(..length).collect();
        jobs.push(Ok(Job::FocusFinal(Box::new(FocusFinal {
            units: members,
            request: Arc::new(request),
            counts: Arc::new(counts),
            window,
        }))));
    }
    Ok(())
}

async fn evaluate_phases(job: &FocusFinal, execution: &Execution) -> Result<Arc<Evaluation>> {
    execution.record_plan(job.units.iter().map(|unit|unit.unit.index).collect(),&job.request,&job.counts,json!({"operation":"focus_final","units":job.units.iter().map(|unit|json!({"index":unit.unit.index,"plan":unit.plan,"selection":unit.selection,"relevance_responses":unit.responses})).collect::<Vec<_>>() }))?;
    let backend = execution
        .backend
        .as_ref()
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "focus final backend missing"))?;
    let model = job
        .units
        .first()
        .and_then(|unit| unit.responses.first())
        .map(|evaluation| evaluation.response.model.as_str())
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "focus relevance model missing"))?;
    if job
        .units
        .iter()
        .flat_map(|unit| &unit.responses)
        .any(|evaluation| evaluation.response.model != model)
    {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "model identity changed before focus final window",
        ));
    }
    let mut evaluation = backend
        .evaluate(
            &job.request,
            &job.counts,
            &execution.budget,
            execution.strict,
            execution.deadline,
        )
        .await
        .map_err(|error| Error::new(error.kind, format!("focus final request: {error}")))?;
    if evaluation.response.model != model {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "model identity changed in focus final response",
        ));
    }
    let final_response = evaluation.response.clone();
    evaluation.cached &= job
        .units
        .iter()
        .flat_map(|unit| &unit.responses)
        .all(|evaluation| evaluation.cached);
    let mut warnings = job
        .units
        .iter()
        .flat_map(|unit| &unit.responses)
        .filter_map(|evaluation| evaluation.warning.as_deref())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(warning) = evaluation.warning {
        warnings.push(warning);
    }
    for unit in &job.units {
        for response in &unit.responses {
            evaluation.response.usage.input_tokens = evaluation
                .response
                .usage
                .input_tokens
                .checked_add(response.response.usage.input_tokens)
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "focus input usage overflow"))?;
            evaluation.response.usage.output_tokens = evaluation
                .response
                .usage
                .output_tokens
                .checked_add(response.response.usage.output_tokens)
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "focus output usage overflow"))?;
        }
        let provenance = json!({"operation":"focus","path":unit.plan.path,"coordinate_space":unit.plan.coordinate_space,"k":unit.plan.k,"selected_chunks":unit.selection.selected_chunks,"ranges":unit.selection.ranges,"separator":unit.selection.separator,"source":unit.unit.location,"original_counts":unit.plan.original_counts,"relevance":unit.plan.relevance.iter().zip(&unit.responses).map(|(chunk,evaluation)|json!({"range":chunk.range,"counts":chunk.counts,"response":evaluation.response})).collect::<Vec<_>>(),"final":{"request":job.request.as_ref(),"counts":job.counts.as_ref(),"response":final_response}});
        for id in unit.selection.request.questions.keys() {
            let answer = evaluation.response.answers.get_mut(id).ok_or_else(|| {
                Error::new(ErrorKind::Unavailable, "focus final answer binding missing")
            })?;
            answer["focused"] = json!(true);
            answer["lossy"] = json!(unit.selection.lossy);
            answer["provenance"] = provenance.clone();
        }
    }
    evaluation.warning = if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("; "))
    };
    Ok(Arc::new(evaluation))
}

pub(crate) async fn evaluate_focus_final(
    job: FocusFinal,
    execution: &Execution,
) -> Vec<WorkResult<Completed>> {
    let result = evaluate_phases(&job, execution).await;
    let evaluation = match result {
        Ok(evaluation) => evaluation,
        Err(error) => {
            return job
                .units
                .iter()
                .map(|unit| {
                    Err(WorkError::at(
                        Error::new(error.kind, error.to_string()),
                        &unit.unit,
                    ))
                })
                .collect();
        }
    };
    let indices = job
        .units
        .iter()
        .map(|unit| unit.unit.index)
        .collect::<Vec<_>>();
    job.units
        .into_iter()
        .map(|unit| {
            Ok(Completed {
                unit: unit.unit,
                request: job.request.clone(),
                counts: job.counts.clone(),
                evaluation: Some(evaluation.clone()),
                answer_id: unit.answer_id,
                packed_indices: job.window.then(|| indices.clone()),
                split_plan: None,
                view: None,
                focus_plan: None,
                fallback: None,
                ensemble: None,
                review: None,
            })
        })
        .collect()
}
