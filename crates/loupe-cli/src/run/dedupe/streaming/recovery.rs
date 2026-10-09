use crate::run::json_error;
use loupe_core::backend::Evaluation;
use loupe_core::dedupe::streaming::{Horizon, Snapshot};
use loupe_core::encoder::Counts;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::wire::Request;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pair {
    pub(crate) representative: u64,
    pub(crate) request: Request,
    pub(crate) counts: Counts,
    pub(crate) evaluation: Evaluation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fallback: Option<loupe_core::fallback::Execution>,
}
impl Pair {
    pub(crate) fn verify(
        &self,
        representative: u64,
        request: &Request,
        counts: &Counts,
    ) -> Result<()> {
        if self.representative != representative
            || serde_json::to_value(&self.request).map_err(json_error)?
                != serde_json::to_value(request).map_err(json_error)?
            || serde_json::to_value(&self.counts).map_err(json_error)?
                != serde_json::to_value(counts).map_err(json_error)?
        {
            return Err(Error::new(
                ErrorKind::Input,
                "streaming saved pair plan changed",
            ));
        }
        self.evaluation.response.validate(request)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Progress {
    pub(crate) admitted: Duration,
    pub(crate) wall: Duration,
    pub(crate) pairs: Vec<Pair>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Completion {
    pub(crate) history: Snapshot,
    pub(crate) wall: Duration,
    pub(crate) kept: bool,
    pub(crate) row: Value,
}
pub(crate) struct Clock {
    anchor: Instant,
    base: Duration,
    recovered: Option<(Duration, Duration)>,
    horizon: Horizon,
}
impl Clock {
    pub(crate) fn new(started: Instant, horizon: Horizon) -> Self {
        Self {
            anchor: started,
            base: Duration::ZERO,
            recovered: None,
            horizon,
        }
    }
    pub(crate) fn restored(&mut self, clock: Duration, wall: Duration) {
        self.recovered = Some((clock, wall));
    }
    pub(crate) fn admit(&mut self) -> Result<(Duration, Duration)> {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                Error::new(
                    ErrorKind::Input,
                    format!("cannot record recovery wall clock: {error}"),
                )
            })?;
        if let Some((clock, then)) = self.recovered.take() {
            let downtime = if matches!(self.horizon, Horizon::Time { .. }) {
                wall.checked_sub(then).ok_or_else(|| {
                    Error::new(
                        ErrorKind::Input,
                        "wall clock moved backwards; cannot resume a time window",
                    )
                })?
            } else {
                Duration::ZERO
            };
            self.base = clock
                .checked_add(downtime)
                .ok_or_else(|| Error::new(ErrorKind::Input, "streaming recovery clock overflow"))?;
            self.anchor = Instant::now();
        }
        let admitted = self
            .base
            .checked_add(self.anchor.elapsed())
            .ok_or_else(|| Error::new(ErrorKind::Input, "streaming admission clock overflow"))?;
        Ok((admitted, wall))
    }
}
