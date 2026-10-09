use std::io::Write;

use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::factors::Set;
use serde_json::json;

use crate::args::Output;
use crate::run::{Completed, Why, json_error, output_error, write_json};

pub(crate) fn emit(
    writer: &mut impl Write,
    completed: &Completed,
    factors: &Set,
    primary: &str,
    output: Output,
    why: Option<Why<'_>>,
    ensemble: Option<loupe_core::factors::Ensemble<'_>>,
) -> Result<()> {
    let evaluation = completed
        .evaluation
        .as_ref()
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing factor evaluation"))?;
    let report = factors.report(
        &completed.request,
        &evaluation.response,
        primary,
        &completed.answer_id,
        ensemble.as_ref(),
    )?;
    let answer = evaluation
        .response
        .answers
        .get(&completed.answer_id)
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing primary factor response"))?;
    match output {
        Output::Json => {
            let mut value = json!({"index":completed.unit.index,"unit":completed.unit.subject,"answer":answer,"model":evaluation.response.model,"cached":evaluation.cached,"composite":report,"request":completed.request.as_ref(),"counts":completed.counts.as_ref(),"response":evaluation.response,"location":completed.unit.location});
            if let Some(ensemble) = &ensemble
                && ensemble.set.has_primary(primary)
            {
                let primary_report = ensemble.set.report(
                    primary,
                    &completed.answer_id,
                    &completed.request,
                    &evaluation.response,
                    loupe_core::policy::Policy::default(),
                    ensemble.min_agreement,
                )?;
                value["ensemble"] = serde_json::to_value(primary_report).map_err(json_error)?;
            }
            if let Some(why) = why {
                value["signals"] =
                    serde_json::to_value(why.reports(completed)?).map_err(json_error)?;
            }
            for (name, provenance) in [
                ("split", &completed.split_plan),
                ("truncate", &completed.view),
                ("focus", &completed.focus_plan),
            ] {
                if let Some(provenance) = provenance {
                    value[name] = provenance.clone();
                }
            }
            if let Some(indices) = &completed.packed_indices {
                value["indices"] = json!(indices);
            }
            write_json(writer, &value)
        }
        Output::Tsv => {
            let factors = serde_json::to_string(&report.factors).map_err(json_error)?;
            writeln!(
                writer,
                "{}\t{}\t{}",
                completed.unit.index, report.score, factors
            )
            .map_err(output_error)
        }
        Output::Plain | Output::Annotate => {
            writeln!(writer, "composite\t{}", report.score).map_err(output_error)?;
            for (id, factor) in &report.factors {
                writeln!(
                    writer,
                    "{id}\tvalue={}\tweight={}\tcontribution={}\tuncertain={}",
                    factor.value, factor.weight, factor.contribution, factor.assessment.uncertain
                )
                .map_err(output_error)?;
            }
            if let Some(why) = why {
                for (id, signal) in why.reports(completed)? {
                    writeln!(
                        writer,
                        "signal:{id}\tp={}\tuncertain={}",
                        signal.probability.get(),
                        signal.assessment.uncertain
                    )
                    .map_err(output_error)?;
                }
            }
            Ok(())
        }
        Output::Csv | Output::Parquet => Err(Error::new(
            ErrorKind::Usage,
            "explain accepts plain, annotate, TSV or JSON",
        )),
    }
}
