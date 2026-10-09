use std::collections::BTreeSet;
use std::io::{self, Write};

use loupe_core::budget::{Budget, Usage};
use loupe_core::calibration::{Fixed, Options};
use loupe_core::config::Profile;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::preset_store::Store;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::args::{Calibrate, Cli, Eval};
use crate::run::eval::calibrate::{Fitted, fit_suite, preflight, write_fitted};
use crate::run::eval::{Failure, RunReport, RunScope, output, run, usage, variants};
use crate::run::{json_error, write_json};

#[derive(Serialize)]
struct Variant {
    name: String,
    evaluation: RunReport,
    fitted: Vec<Fitted>,
}
#[derive(Serialize)]
struct Winner {
    variant: String,
    suite: String,
    field: String,
    accepted: usize,
    correct: usize,
    cases: usize,
    uncached_units_per_second: f64,
    proposed_preset: Option<Value>,
    written: Option<std::path::PathBuf>,
}

struct Chosen {
    variant_index: usize,
    fitted: Fitted,
    throughput: f64,
}

#[derive(Clone, Copy)]
struct Rank {
    accepted: usize,
    correct: usize,
    throughput: f64,
}
impl Rank {
    fn beats(self, previous: Self) -> bool {
        (self.accepted, self.correct) > (previous.accepted, previous.correct)
            || (self.accepted, self.correct) == (previous.accepted, previous.correct)
                && self.throughput > previous.throughput
    }
}
#[derive(Serialize)]
struct Report {
    schema_version: u32,
    kind: &'static str,
    status: String,
    targets: Vec<loupe_core::calibration::Target>,
    max_uncertain: Option<f64>,
    minimum_support: usize,
    token_usage: Usage,
    variants: Vec<Variant>,
    selected: Vec<Winner>,
    error: Option<Failure>,
    notes: Vec<String>,
}

pub(crate) async fn execute(
    cli: &Cli,
    options: &Calibrate,
    store: &Store,
    profile: Profile,
    settings: &Eval,
    budget: &Budget,
    deadline: Option<Instant>,
) -> Result<u8> {
    let candidates = variants::sweep(settings)?;
    let search = Options {
        targets: options.target.clone(),
        max_uncertain: options.max_uncertain,
        minimum_support: options.min_support.get(),
        fixed: Fixed {
            threshold: settings.threshold,
            margin: settings.margin,
            min_confidence: settings.min_confidence,
            min_gap: settings.min_gap,
        },
    };
    // Validate the complete grid and fitting domains before network or split writes.
    for (_, candidate) in &candidates {
        let mut preview = candidate.clone();
        preview.dry_run = true;
        preview.allow_unverified = false;
        let planned = run(
            cli,
            &preview,
            store,
            profile.clone(),
            RunScope {
                budget,
                deadline,
                initialize_splits: false,
                recalibrate: true,
                baseline: None,
            },
        )
        .await?;
        if let Some(error) = planned.error {
            return Err(Error::new(
                if error.code == 64 {
                    ErrorKind::Usage
                } else {
                    ErrorKind::Input
                },
                error.message,
            ));
        }
        preflight(&planned, options.field.as_deref(), &search)?;
    }
    let mut report = Report {
        schema_version:1,kind:"calibration-sweep",status:if settings.dry_run {"plan"} else {"complete"}.into(),
        targets:options.target.clone(),max_uncertain:options.max_uncertain,minimum_support:options.min_support.get(),
        token_usage:Usage::default(),variants:Vec::new(),selected:Vec::new(),error:None,
        notes:vec![
            "Only fixed calibration cases are measured; held-out test and captured review cases never enter this grid.".into(),
            "Feasible views are ranked by accepted support, then correct decisions, then uncached throughput, then declaration order.".into(),
            "The grid shares one budget/deadline and requires stable model/local/server encoder identities and case provenance.".into(),
            "Calibration selection does not ratify a held-out baseline or establish population precision. Unverified diagnostics cannot publish preset policies.".into(),
        ],
    };
    for (name, candidate) in candidates {
        let result = run(
            cli,
            &candidate,
            store,
            profile.clone(),
            RunScope {
                budget,
                deadline,
                initialize_splits: true,
                recalibrate: true,
                baseline: None,
            },
        )
        .await;
        let evaluation = match result {
            Ok(value) => value,
            Err(error) => {
                report.error = Some(Failure {
                    code: error.exit_code(),
                    message: error.to_string(),
                });
                break;
            }
        };
        let mut fitted = Vec::new();
        let mut fitting_error = None;
        if evaluation.error.is_none() && !settings.dry_run {
            for suite in &evaluation.suites {
                match fit_suite(
                    suite,
                    options.field.as_deref(),
                    &search,
                    &evaluation,
                    store,
                    &profile,
                ) {
                    Ok(fit) => fitted.push(fit),
                    Err(error) => {
                        fitting_error = Some(Failure {
                            code: error.exit_code(),
                            message: error.to_string(),
                        });
                        break;
                    }
                }
            }
        }
        if evaluation.status == "provisional" {
            report.status = "provisional".into();
        }
        report.error = evaluation.error.clone().or(fitting_error);
        report.variants.push(Variant {
            name,
            evaluation,
            fitted,
        });
        if report.error.is_some() {
            break;
        }
        if !settings.dry_run && report.variants.len() > 1 {
            let first = report
                .variants
                .first()
                .ok_or_else(|| usage("first calibration view missing"))?;
            let current = report
                .variants
                .last()
                .ok_or_else(|| usage("current calibration view missing"))?;
            if let Err(error) = variants::same_backend(&first.evaluation, &current.evaluation)
                .and_then(|_| variants::same_cases(&first.evaluation, &current.evaluation))
            {
                report.error = Some(Failure {
                    code: error.exit_code(),
                    message: error.to_string(),
                });
                break;
            }
        }
    }
    let mut code = report.error.as_ref().map_or(0, |error| error.code);
    if report.error.is_some() {
        report.status = "incomplete".into();
    } else if !settings.dry_run {
        match select(&mut report, options, store, &profile, deadline).await {
            Ok(selected_code) => code = selected_code,
            Err(error) => {
                report.status = "incomplete".into();
                code = error.exit_code();
                report.error = Some(Failure {
                    code,
                    message: error.to_string(),
                });
            }
        }
    }
    report.token_usage = budget.usage()?;
    let markdown = markdown(&report);
    if let Some(root) = &settings.report {
        let destination = loupe_core::report_store::Store::open(root)?;
        let mut bytes = serde_json::to_vec_pretty(&report).map_err(json_error)?;
        bytes.push(b'\n');
        let path = destination.publish(&bytes, markdown.as_bytes())?;
        if !cli.quiet {
            eprintln!(
                "loupe: calibration reports published under {}",
                path.display()
            );
        }
    }
    if settings.json {
        write_json(&mut io::stdout().lock(), &report)?;
    } else {
        io::stdout()
            .lock()
            .write_all(markdown.as_bytes())
            .map_err(|error| Error::io("writing calibration sweep report", error))?;
    }
    Ok(code)
}

async fn select(
    report: &mut Report,
    options: &Calibrate,
    store: &Store,
    profile: &Profile,
    deadline: Option<Instant>,
) -> Result<u8> {
    let chosen = choose(&report.variants)?;
    let mut fitted = chosen
        .iter()
        .map(|chosen| chosen.fitted.clone())
        .collect::<Vec<_>>();
    let suites = report
        .variants
        .first()
        .ok_or_else(|| usage("calibration grid is empty"))?
        .evaluation
        .suites
        .len();
    let mut code = 0;
    if chosen.len() != suites {
        report.status = "unmet".into();
        code = 1;
    } else if options.write
        && let Err(error) = write_fitted(&mut fitted, store, profile, deadline).await
    {
        report.status = "incomplete".into();
        code = error.exit_code();
        report.error = Some(Failure {
            code,
            message: error.to_string(),
        });
        report.notes.push("Each preset is committed atomically; previously committed presets remain recorded when a later write fails.".into());
    }
    for (chosen, fit) in chosen.into_iter().zip(fitted) {
        let variant = report
            .variants
            .get(chosen.variant_index)
            .ok_or_else(|| usage("chosen calibration view missing"))?;
        let selected = fit
            .fit
            .selected
            .as_ref()
            .ok_or_else(|| usage("chosen view has no feasible fit"))?;
        report.selected.push(Winner {
            variant: variant.name.clone(),
            suite: fit.suite.clone(),
            field: fit.field.clone(),
            accepted: selected.accepted,
            correct: selected.correct,
            cases: selected.cases,
            uncached_units_per_second: chosen.throughput,
            proposed_preset: fit.proposed_preset,
            written: fit.written,
        });
    }
    Ok(code)
}

fn rate(variant: &Variant, suite: &str) -> Result<f64> {
    let suite = variant
        .evaluation
        .suites
        .iter()
        .find(|item| item.manifest.id == suite)
        .ok_or_else(|| usage("calibration throughput suite missing"))?;
    let phase = suite
        .phases
        .iter()
        .find(|phase| phase.name == "latency")
        .ok_or_else(|| usage("uncached calibration throughput phase missing"))?;
    if phase.cache_requested
        || phase.cache_hits != 0
        || phase.error.is_some()
        || phase.timing.completed_units != suite.selected_case_ids.len()
        || !phase.timing.elapsed_seconds.is_finite()
        || phase.timing.elapsed_seconds <= 0.0
    {
        return Err(usage(
            "calibration winner requires a complete positive uncached throughput measurement",
        ));
    }
    let value = phase.timing.completed_units as f64 / phase.timing.elapsed_seconds;
    if !value.is_finite() || value <= 0.0 {
        return Err(usage("invalid measured calibration throughput"));
    }
    Ok(value)
}

fn choose(variants: &[Variant]) -> Result<Vec<Chosen>> {
    let first = variants
        .first()
        .ok_or_else(|| usage("calibration grid is empty"))?;
    let mut chosen = Vec::new();
    let mut destinations = BTreeSet::new();
    for suite in &first.evaluation.suites {
        let mut winner: Option<(usize, &Fitted, f64)> = None;
        for (index, variant) in variants.iter().enumerate() {
            let fit = variant
                .fitted
                .iter()
                .find(|fit| fit.suite == suite.manifest.id)
                .ok_or_else(|| usage("calibration view fitting result missing"))?;
            let Some(selected) = &fit.fit.selected else {
                continue;
            };
            let throughput = rate(variant, &fit.suite)?;
            let rank = Rank {
                accepted: selected.accepted,
                correct: selected.correct,
                throughput,
            };
            let better = winner.is_none_or(|(_, previous, previous_rate)| {
                previous.fit.selected.as_ref().is_some_and(|old| {
                    rank.beats(Rank {
                        accepted: old.accepted,
                        correct: old.correct,
                        throughput: previous_rate,
                    })
                })
            });
            if better {
                winner = Some((index, fit, throughput));
            }
        }
        if let Some((index, fit, throughput)) = winner {
            let variant = variants
                .get(index)
                .ok_or_else(|| usage("chosen calibration view missing"))?;
            let mut fit = fit.clone();
            if let Some(document) = &mut fit.proposed_preset {
                let source = document
                    .get_mut("policy")
                    .and_then(|value| value.get_mut("calibration"))
                    .and_then(|value| value.get_mut("source"))
                    .ok_or_else(|| usage("chosen calibration proposal lacks provenance"))?;
                source["view_selection"] = json!({"method":"accepted_then_correct_then_uncached_throughput","winner":variant.name,"uncached_units_per_second":throughput,"candidates":variants.iter().map(|variant|json!({"name":variant.name,"fit":variant.fitted.iter().find(|other|other.suite==fit.suite).map(|fit|&fit.fit)})).collect::<Vec<_>>()});
            }
            if !destinations.insert((fit.suite.clone(), fit.field.clone())) {
                return Err(usage("duplicate calibration winner"));
            }
            chosen.push(Chosen {
                variant_index: index,
                fitted: fit,
                throughput,
            });
        }
    }
    Ok(chosen)
}

fn markdown(report: &Report) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "# Loupe calibration views\n\nStatus: **{}**. Input tokens: {}.\n\n",
        report.status, report.token_usage.spent_input
    );
    for winner in &report.selected {
        let _ = writeln!(
            text,
            "{} / {}: {}. Accepted {}/{}; correct {}. Uncached units/s: {:.6}.\n",
            output::cell(&winner.suite),
            output::cell(&winner.field),
            output::cell(&winner.variant),
            winner.accepted,
            winner.cases,
            winner.correct,
            winner.uncached_units_per_second
        );
    }
    for variant in &report.variants {
        let _ = writeln!(text, "## {}\n", output::cell(&variant.name));
        text.push_str(&output::markdown(&variant.evaluation));
    }
    for note in &report.notes {
        let _ = writeln!(text, "{note}\n");
    }
    if let Some(error) = &report.error {
        let _ = writeln!(
            text,
            "Failure (exit {}): {}",
            error.code,
            output::cell(&error.message)
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use crate::run::eval::calibrate::views::Rank;

    #[test]
    fn numerical_quality_tiers_take_precedence_over_speed_and_equal_ranks_keep_declaration_order() {
        let baseline = Rank {
            accepted: 10,
            correct: 9,
            throughput: 1.0,
        };
        assert!(
            !Rank {
                accepted: 9,
                correct: 9,
                throughput: 1e6
            }
            .beats(baseline)
        );
        assert!(
            !Rank {
                accepted: 10,
                correct: 8,
                throughput: 1e6
            }
            .beats(baseline)
        );
        assert!(
            Rank {
                accepted: 10,
                correct: 10,
                throughput: 0.1
            }
            .beats(baseline)
        );
        assert!(
            Rank {
                throughput: 2.0,
                ..baseline
            }
            .beats(baseline)
        );
        assert!(!baseline.beats(baseline));
    }
}
