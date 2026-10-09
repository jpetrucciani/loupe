use crate::args::Eval;
use crate::run::eval::{Failure, Phase, RunScope, usage_delta};
use loupe_core::backend::{Backend, Evaluation};
use loupe_core::config::Profile;
use loupe_core::encoder::{ClefEncoder, Counts};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::wire::Request;
use serde_json::Value;
use std::time::Duration;
use tokio::time::Instant;

pub(in crate::run::eval) struct Context<'a, 'b> {
    pub(in crate::run::eval) options: &'a Eval,
    pub(in crate::run::eval) profile: &'a Profile,
    pub(in crate::run::eval) encoder: Option<&'a ClefEncoder>,
    pub(in crate::run::eval) scope: &'a RunScope<'b>,
    response_model: std::sync::Mutex<Option<String>>,
    signal_code: std::sync::Mutex<Option<u8>>,
}
impl<'a, 'b> Context<'a, 'b> {
    pub(in crate::run::eval) fn new(
        options: &'a Eval,
        profile: &'a Profile,
        encoder: Option<&'a ClefEncoder>,
        scope: &'a RunScope<'b>,
    ) -> Self {
        Self {
            options,
            profile,
            encoder,
            scope,
            response_model: std::sync::Mutex::new(None),
            signal_code: std::sync::Mutex::new(None),
        }
    }
    pub(in crate::run::eval) async fn evaluate(
        &self,
        backend: &Backend,
        request: &Request,
        counts: &Counts,
    ) -> Result<Evaluation> {
        let result = tokio::select! {
            biased;
            signal=crate::run::interrupt_signal()=>{
                let code=signal?;
                *self.signal_code.lock().map_err(|_|Error::new(ErrorKind::Input,"evaluation signal lock poisoned"))?=Some(code);
                return Err(Error::new(ErrorKind::Interrupted,"evaluation interrupted"));
            },
            result=backend.evaluate(request,counts,self.scope.budget,self.options.strict,self.scope.deadline)=>result?,
        };
        let reported = loupe_core::identity::Model::parse(&result.response.model).map_or_else(
            |_| result.response.model.clone(),
            |model| model.as_str().to_owned(),
        );
        let mut observed = self
            .response_model
            .lock()
            .map_err(|_| Error::new(ErrorKind::Input, "evaluation identity lock poisoned"))?;
        if observed.as_ref().is_some_and(|model| model != &reported) {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "response model identity changed during evaluation",
            ));
        }
        *observed = Some(reported);
        Ok(result)
    }
    pub(in crate::run::eval) fn failure(&self, error: Error) -> Failure {
        let code = match self.signal_code.lock() {
            Ok(code) => code.unwrap_or_else(|| error.exit_code()),
            Err(_) => error.exit_code(),
        };
        Failure {
            code,
            message: error.to_string(),
        }
    }
}
pub(in crate::run::eval) struct Measurement<'a> {
    pub(in crate::run::eval) name: &'a str,
    pub(in crate::run::eval) pass: usize,
    pub(in crate::run::eval) cache: bool,
    pub(in crate::run::eval) groups: Vec<Value>,
    pub(in crate::run::eval) latencies: Vec<Duration>,
    pub(in crate::run::eval) units: usize,
    pub(in crate::run::eval) started: Instant,
    pub(in crate::run::eval) before: loupe_core::budget::Usage,
}
impl Measurement<'_> {
    pub(in crate::run::eval) fn finish(
        self,
        context: &Context<'_, '_>,
        error: Option<Failure>,
    ) -> Result<Phase> {
        let usage = usage_delta(context.scope.budget.usage()?, self.before)?;
        let logical_requests = self
            .groups
            .iter()
            .filter_map(|group| {
                group
                    .get("pairs")
                    .or_else(|| group.get("requests"))
                    .and_then(Value::as_array)
            })
            .map(|records| {
                records
                    .iter()
                    .filter(|record| !record["evaluation"].is_null())
                    .count() as u64
            })
            .sum();
        Ok(Phase {
            name: self.name.into(),
            pass: self.pass,
            cache_requested: self.cache,
            cache_hits: usage.cache_hits,
            logical_requests,
            cache_hit_rate: (logical_requests != 0)
                .then(|| usage.cache_hits as f64 / logical_requests as f64),
            usage,
            timing: loupe_core::task_metrics::timing(
                &self.latencies,
                self.units,
                self.started.elapsed(),
            ),
            groups: self.groups,
            error,
        })
    }
}
