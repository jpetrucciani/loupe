use std::io::{self, Write};

use loupe_core::budget::Budget;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::metrics::Interval;

pub struct Summary {
    enabled: bool,
    planning: bool,
    budget: Budget,
    units: u64,
    uncertain: u64,
    errors: u64,
    fallback_attempts: u64,
    fallback_units: u64,
    current_unit_fallback: bool,
    fallback_resolved: u64,
    fallback_failed: u64,
    fallback_replayed: u64,
    fallback_elapsed_ns: u128,
    pricing: Option<loupe_core::cost::Pricing>,
}
impl Summary {
    pub fn new(
        enabled: bool,
        planning: bool,
        budget: Budget,
        pricing: Option<loupe_core::cost::Pricing>,
    ) -> Self {
        Self {
            enabled,
            planning,
            budget,
            units: 0,
            uncertain: 0,
            errors: 0,
            fallback_attempts: 0,
            fallback_units: 0,
            current_unit_fallback: false,
            fallback_resolved: 0,
            fallback_failed: 0,
            fallback_replayed: 0,
            fallback_elapsed_ns: 0,
            pricing,
        }
    }
    pub fn unit(&mut self) -> Result<()> {
        self.units = self.units.checked_add(1).ok_or_else(overflow)?;
        self.current_unit_fallback = false;
        Ok(())
    }
    pub fn uncertain(&mut self) -> Result<()> {
        self.uncertain = self.uncertain.checked_add(1).ok_or_else(overflow)?;
        Ok(())
    }
    pub fn error(&mut self) -> Result<()> {
        self.errors = self.errors.checked_add(1).ok_or_else(overflow)?;
        Ok(())
    }
    pub fn fallback_start(&mut self) -> Result<()> {
        self.fallback_attempts = self.fallback_attempts.checked_add(1).ok_or_else(overflow)?;
        if !self.current_unit_fallback {
            self.fallback_units = self.fallback_units.checked_add(1).ok_or_else(overflow)?;
            self.current_unit_fallback = true;
        }
        Ok(())
    }
    pub fn fallback_finish(&mut self, resolved: bool, elapsed: std::time::Duration) -> Result<()> {
        let counter = if resolved {
            &mut self.fallback_resolved
        } else {
            &mut self.fallback_failed
        };
        *counter = counter.checked_add(1).ok_or_else(overflow)?;
        self.fallback_elapsed_ns = self
            .fallback_elapsed_ns
            .checked_add(elapsed.as_nanos())
            .ok_or_else(overflow)?;
        Ok(())
    }
    pub fn fallback_replay(&mut self) -> Result<()> {
        self.fallback_replayed = self.fallback_replayed.checked_add(1).ok_or_else(overflow)?;
        Ok(())
    }
    fn render(&self) -> Result<String> {
        let usage = self.budget.usage()?;
        let metrics = self.budget.metrics.snapshot()?;
        let hit_rate = if metrics.logical_records == 0 {
            "unavailable".to_owned()
        } else {
            format!(
                "{:.6}",
                usage.cache_hits as f64 / metrics.logical_records as f64
            )
        };
        let (cost, unknown_output) = if let Some(pricing) = &self.pricing {
            match pricing.estimate(usage) {
                Ok(estimate) => (format!("{:.6}", estimate.usd), estimate.unknown_output),
                Err(_) => ("unavailable".to_owned(), usage.unknown_attempts > 0),
            }
        } else {
            ("unavailable".to_owned(), usage.unknown_attempts > 0)
        };
        let mut rendered = format!(
            "loupe: units={} attempts={} http_attempts={} input_counted={} input_reported={} unknown_attempts={} uncertain={} cache_hits={} gpu_passes=unavailable logical_records={} succeeded_records={} failed_records={} cancelled_records={} output_reported={} input_reserved={} cache_hit_rate={} latency_samples={} p50_ms={} p95_ms={} latency_bounds=upper latency_kind=uncached_submitted_logical cost_usd_estimate={} cost_unknown_output={} unknown_input_counted={} mode={} errors={}",
            self.units,
            usage.attempts,
            usage.http_attempts,
            usage.spent_input,
            usage.reported_input,
            usage.unknown_attempts,
            self.uncertain,
            usage.cache_hits,
            metrics.logical_records,
            metrics.succeeded_records,
            metrics.failed_records,
            metrics.cancelled_records,
            usage.reported_output,
            usage.reserved_input,
            hit_rate,
            metrics.latency_samples,
            latency(metrics.p50),
            latency(metrics.p95),
            cost,
            unknown_output,
            usage.unknown_input,
            if self.planning { "plan" } else { "classify" },
            self.errors
        );
        if self.fallback_attempts > 0 || self.fallback_replayed > 0 {
            use std::fmt::Write as _;
            let fraction = if self.units == 0 {
                "unavailable".to_owned()
            } else {
                format!("{:.6}", self.fallback_units as f64 / self.units as f64)
            };
            write!(rendered, " fallback_attempts={} fallback_fraction={} fallback_resolved={} fallback_failed={} fallback_cancelled={} fallback_replayed={} fallback_elapsed_ms={:.6} fallback_cost_usd=unavailable fallback_cost_scope=external", self.fallback_attempts, fraction, self.fallback_resolved, self.fallback_failed, self.fallback_attempts.saturating_sub(self.fallback_resolved).saturating_sub(self.fallback_failed), self.fallback_replayed, self.fallback_elapsed_ns as f64 / 1_000_000.0)
                .map_err(|_| overflow())?;
        }
        Ok(rendered)
    }
}
impl Drop for Summary {
    fn drop(&mut self) {
        if self.enabled {
            let line = match self.render() {
                Ok(line) => line,
                Err(error) => format!("loupe: summary unavailable: {error}"),
            };
            // Diagnostics cannot change the classification exit code or panic on a closed stderr.
            let _ = writeln!(io::stderr().lock(), "{line}");
        }
    }
}
fn latency(interval: Option<Interval>) -> String {
    interval.map_or_else(
        || "unavailable".to_owned(),
        |interval| format!("{:.6}", interval.upper_ns as f64 / 1_000_000.0),
    )
}
fn overflow() -> Error {
    Error::new(ErrorKind::Unavailable, "summary unit counter overflow")
}
