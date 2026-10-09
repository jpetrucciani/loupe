use serde::{Deserialize, Serialize};

use crate::budget::Usage;
use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
}
pub struct Estimate {
    pub usd: f64,
    pub unknown_output: bool,
}
impl Pricing {
    pub fn validate(&self) -> Result<()> {
        if [self.input_usd_per_million, self.output_usd_per_million]
            .iter()
            .any(|rate| !rate.is_finite() || *rate < 0.0)
        {
            return Err(Error::new(
                ErrorKind::Config,
                "pricing rates must be finite nonnegative USD per million tokens",
            ));
        }
        Ok(())
    }
    pub fn estimate(&self, usage: Usage) -> Result<Estimate> {
        self.validate()?;
        let input = usage
            .reported_input
            .checked_add(usage.unknown_input)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Unavailable,
                    "estimated billable input-token counter overflow",
                )
            })?;
        let usd = input as f64 / 1_000_000.0 * self.input_usd_per_million
            + usage.reported_output as f64 / 1_000_000.0 * self.output_usd_per_million;
        if !usd.is_finite() {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "estimated USD cost overflow",
            ));
        }
        Ok(Estimate {
            usd,
            unknown_output: usage.unknown_attempts > 0,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::budget::{Budget, Usage};
    use crate::cost::Pricing;
    #[test]
    fn known_usage_and_counted_unknown_attempts_have_explicit_partial_cost_estimates() {
        let pricing = Pricing {
            input_usd_per_million: 2.0,
            output_usd_per_million: 4.0,
        };
        let budget = Budget::new(None);
        budget
            .reserve(1_000_000)
            .unwrap()
            .submit()
            .unwrap()
            .finish(900_000, 250_000)
            .unwrap();
        drop(budget.reserve(500_000).unwrap().submit().unwrap());
        let estimate = pricing.estimate(budget.usage().unwrap()).unwrap();
        assert!((estimate.usd - 3.8).abs() < 1e-12);
        assert!(estimate.unknown_output);
        assert_eq!(budget.usage().unwrap().unknown_input, 500_000);
        assert_eq!(pricing.estimate(Usage::default()).unwrap().usd, 0.0);
    }
    #[test]
    fn invalid_or_overflowing_prices_never_produce_a_plausible_number() {
        for rate in [f64::NAN, f64::INFINITY, -1.0] {
            assert_eq!(
                Pricing {
                    input_usd_per_million: rate,
                    output_usd_per_million: 1.0
                }
                .validate()
                .unwrap_err()
                .exit_code(),
                78
            );
        }
        let pricing = Pricing {
            input_usd_per_million: f64::MAX,
            output_usd_per_million: 0.0,
        };
        assert!(
            pricing
                .estimate(Usage {
                    reported_input: u64::MAX,
                    ..Usage::default()
                })
                .is_err()
        );
        let pricing = Pricing {
            input_usd_per_million: 1.0,
            output_usd_per_million: 1.0,
        };
        assert!(
            pricing
                .estimate(Usage {
                    reported_input: u64::MAX,
                    unknown_input: 1,
                    ..Usage::default()
                })
                .is_err()
        );
    }
}
