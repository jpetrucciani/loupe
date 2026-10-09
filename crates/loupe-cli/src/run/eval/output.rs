use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use loupe_core::error::Result;
use loupe_core::task_metrics::Value as MetricValue;

use crate::run::eval::RunReport;

pub(crate) struct Destination(loupe_core::report_store::Store);
impl Destination {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        loupe_core::report_store::Store::open(root).map(Self)
    }
    pub(crate) fn publish(&mut self, report: &RunReport) -> Result<PathBuf> {
        let mut json = serde_json::to_vec_pretty(report).map_err(crate::run::json_error)?;
        json.push(b'\n');
        self.0.publish(&json, markdown(report).as_bytes())
    }
}
fn number(metric: &MetricValue) -> String {
    metric.value.map_or_else(
        || format!("undefined ({:?}; n={})", metric.undefined, metric.support),
        |value| format!("{value:.6} (n={})", metric.support),
    )
}
pub(crate) fn cell(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('|', "&#124;")
        .replace(['\n', '\r'], " ")
}
pub(crate) fn markdown(report: &RunReport) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Loupe evaluation\n\nStatus: **{}**. Generated: {}.\n",
        cell(&report.status),
        cell(&report.generated_at)
    );
    let _ = writeln!(
        text,
        "Endpoint: {}. Requested model: {}. Verified identity: {}.\n",
        cell(&report.endpoint),
        cell(&report.identity.requested_model),
        report.identity.verified
    );
    if let Some(model) = &report.identity.model {
        let _ = writeln!(text, "Model: {}.\n", cell(model.as_str()));
    }
    if let Some(reason) = &report.identity.reason {
        let _ = writeln!(text, "Identity status: {}.\n", cell(reason));
    }
    let _ = writeln!(text, "Encoder: {}.\n", cell(&report.identity.local_encoder));
    let _ = writeln!(
        text,
        "Input tokens charged: {}. Reported: {}. Attempts: {}. HTTP attempts: {}. Cache hits: {}. Unknown attempts: {}.\n",
        report.token_usage.spent_input,
        report.token_usage.reported_input,
        report.token_usage.attempts,
        report.token_usage.http_attempts,
        report.token_usage.cache_hits,
        report.token_usage.unknown_attempts
    );
    if let Some(cost) = report.cost_usd_estimate {
        let _ = writeln!(
            text,
            "Estimated cost (USD): {cost:.6}. Unknown output cost: {}.\n",
            report.cost_unknown_output
        );
    }
    let _ = writeln!(
        text,
        "{}\n",
        if report.suites.iter().any(|suite| suite.join.is_some()) {
            "Join latency samples measure complete scenario/cardinality pipelines, including blocking, pair inference and assignment. Throughput counts left records. Accuracy timing counts catalog pairs separately. Measured latency is uncached. GPU work is not attributed."
        } else if report.suites.iter().any(|suite| suite.selection.is_some()) {
            "Document selection latency samples measure complete tournament/independent pipelines, including candidate planning, admission, retries and normalization. Throughput counts documents. Measured latency is uncached. GPU work is not attributed."
        } else {
            "Latency samples measure complete packing-group pipelines, including encoding, admission, retries and split/focus phases. Measured latency is uncached. GPU work is not attributed."
        }
    );
    if let Some(acceptance) = &report.acceptance {
        text.push_str(&crate::run::eval::gates::markdown(acceptance));
    }
    for suite in &report.suites {
        let _ = writeln!(
            text,
            "## {} / {}\n\nDataset revision: {}. Dataset BLAKE3: {}. Split seed: {}. Completed accuracy cases: {} of {} selected.\n",
            cell(&suite.manifest.id),
            cell(&suite.partition),
            cell(&suite.manifest.cases.revision),
            cell(&suite.manifest.cases.blake3),
            suite.splits.config.seed,
            suite.selection.as_ref().map_or_else(
                || suite.join.as_ref().map_or(
                    suite.records.len(),
                    crate::run::eval::join::completed_catalogs
                ),
                crate::run::eval::selection::completed_cases
            ),
            suite.selected_case_ids.len()
        );
        let _ = writeln!(
            text,
            "| Phase | Pass | Units | Logical requests | Cache hits | Input charged | p50 ms | p95 ms | Units/s |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
        );
        for phase in &suite.phases {
            let _ = writeln!(
                text,
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                cell(&phase.name),
                phase.pass,
                phase.timing.completed_units,
                phase.logical_requests,
                phase.cache_hits,
                phase.usage.spent_input,
                number(&phase.timing.p50_ms),
                number(&phase.timing.p95_ms),
                number(&phase.timing.units_per_second)
            );
        }
        text.push('\n');
        if let Some(join) = &suite.join {
            text.push_str(&crate::run::eval::join::markdown(join));
        }
        if let Some(selection) = &suite.selection {
            text.push_str(&crate::run::eval::selection::markdown(selection));
        }
        for question in &suite.questions {
            let metrics = &question.metrics;
            let _ = writeln!(
                text,
                "### {} ({})\n\nRubric signature: {}. Cases: {}. Accepted: {}. Ambiguous labels: {}.\n",
                cell(&question.field),
                cell(question.question.type_name()),
                cell(&question.rubric_signature),
                metrics.cases,
                metrics.accepted,
                metrics.ambiguous_labels
            );
            let _ = writeln!(text, "| Metric | Value |\n| --- | --- |");
            for (name, value) in [
                ("Accuracy", &metrics.accuracy),
                ("Macro-F1", &metrics.macro_f1),
                ("F1", &metrics.f1),
                ("AUROC", &metrics.auroc),
                ("Brier", &metrics.brier),
                ("ECE", &metrics.ece),
                ("Coverage", &metrics.coverage),
                ("Selective accuracy", &metrics.selective_accuracy),
                ("MAE", &metrics.mae),
                ("Within-one accuracy", &metrics.within_one_accuracy),
            ] {
                let _ = writeln!(text, "| {name} | {} |", number(value));
            }
            if let Some(bounds) = &metrics.mae_bounds {
                let _ = writeln!(
                    text,
                    "\nOrdinal MAE bounds: {:.6} to {:.6}. Point-label support: {}.\n",
                    bounds.minimum, bounds.maximum, metrics.point_labels
                );
            }
            let _ = writeln!(
                text,
                "\n| Label | True support | Predicted | TP | FP | FN | Precision | Recall | F1 |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
            );
            for label in &metrics.per_label {
                let _ = writeln!(
                    text,
                    "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                    cell(&label.label),
                    label.support,
                    label.predicted,
                    label.true_positive,
                    label.false_positive,
                    label.false_negative,
                    number(&label.precision),
                    number(&label.recall),
                    number(&label.f1)
                );
            }
            let _ = writeln!(
                text,
                "\nCalibration: {:?}. Brier scale: {:?}.\n\n| Bin | Count | Mean predicted | Mean observed |\n| --- | --- | --- | --- |",
                metrics.calibration_kind, metrics.brier_scale
            );
            for bin in &metrics.reliability {
                let _ = writeln!(
                    text,
                    "| {}{:.1}, {:.1}] | {} | {} | {} |",
                    if bin.lower_inclusive { "[" } else { "(" },
                    bin.lower,
                    bin.upper,
                    bin.count,
                    bin.predicted
                        .map_or_else(|| "undefined".to_owned(), |value| format!("{value:.6}")),
                    bin.observed
                        .map_or_else(|| "undefined".to_owned(), |value| format!("{value:.6}"))
                );
            }
            let _ = writeln!(
                text,
                "\n| Confidence cutoff | Accepted | Coverage | Selective accuracy |\n| --- | --- | --- | --- |"
            );
            for point in &metrics.confidence_gates {
                let _ = writeln!(
                    text,
                    "| {:.3} | {} | {} | {} |",
                    point.min_confidence,
                    point.accepted,
                    number(&point.coverage),
                    number(&point.accuracy)
                );
            }
            text.push('\n');
        }
        if let Some(parity) = &suite.parity {
            let _ = writeln!(
                text,
                "Parity fields: {}. Probabilities: {}. Max absolute error: {}. Mean absolute error: {}. Argmax agreement: {}.\n",
                parity.fields,
                parity.probabilities,
                number(&parity.max_absolute_error),
                number(&parity.mean_absolute_error),
                number(&parity.argmax_agreement)
            );
        }
        for warning in &suite.warnings {
            let _ = writeln!(text, "Warning: {}.\n", cell(warning));
        }
    }
    if let Some(error) = &report.error {
        let _ = writeln!(
            text,
            "Evaluation stopped with exit {}: {}.\n",
            error.code,
            cell(&error.message)
        );
    }
    text
}
