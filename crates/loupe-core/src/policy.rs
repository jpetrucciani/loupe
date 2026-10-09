use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Probability(f64);

impl Probability {
    pub fn new(value: f64) -> Result<Self> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(Error::new(
                ErrorKind::Input,
                format!("probability must be finite and in [0, 1], got {value}"),
            ))
        }
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub threshold: Probability,
    pub margin: f64,
    pub min_confidence: Probability,
    pub min_gap: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            threshold: Probability(0.5),
            margin: 0.0,
            min_confidence: Probability(0.0),
            min_gap: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Selected {
    Boolean(bool),
    Choice(String),
    Score(f64),
}

#[derive(Clone, Debug, Serialize)]
pub struct Assessment {
    pub selected: Selected,
    pub confidence: Probability,
    pub gap: Option<f64>,
    pub uncertain: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Filter {
    Keep,
    Drop,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UncertainAction {
    Keep,
    #[default]
    Drop,
    Fail,
    Stderr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Stdout,
    Stderr,
    Omit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuestionKind {
    Noul,
    Choice,
    Score,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyFlag {
    Threshold,
    Margin,
    MinConfidence,
    MinGap,
}

impl PolicyFlag {
    pub fn validate(self, kind: QuestionKind) -> Result<()> {
        let (name, valid) = match self {
            Self::Threshold => ("threshold", kind == QuestionKind::Noul),
            Self::Margin => ("margin", kind == QuestionKind::Noul),
            Self::MinConfidence => ("min-confidence", kind != QuestionKind::Noul),
            Self::MinGap => ("min-gap", kind != QuestionKind::Noul),
        };
        if valid {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Usage,
                format!("--{name} is not applicable to {kind:?} questions"),
            ))
        }
    }
}

impl Assessment {
    pub fn destination(&self, action: UncertainAction) -> Result<Destination> {
        if !self.uncertain {
            return Ok(Destination::Stdout);
        }
        match action {
            UncertainAction::Keep => Ok(Destination::Stdout),
            UncertainAction::Drop => Ok(Destination::Omit),
            UncertainAction::Stderr => Ok(Destination::Stderr),
            UncertainAction::Fail => Err(Error::new(ErrorKind::Uncertain, "decision is uncertain")),
        }
    }

    pub fn filter(&self, filter: Filter, action: UncertainAction) -> Result<Destination> {
        let Selected::Boolean(yes) = self.selected else {
            return Err(Error::new(
                ErrorKind::Usage,
                "keep/drop require a noul question",
            ));
        };
        if self.uncertain {
            return self.destination(action);
        }
        Ok(if yes == (filter == Filter::Keep) {
            Destination::Stdout
        } else {
            Destination::Omit
        })
    }

    pub fn test_exit_code(&self) -> Result<u8> {
        let Selected::Boolean(yes) = self.selected else {
            return Err(Error::new(ErrorKind::Usage, "is requires a noul question"));
        };
        Ok(if self.uncertain { 2 } else { u8::from(!yes) })
    }
}

impl Policy {
    pub fn from_field(document: &serde_json::Value, id: &str, kind: QuestionKind) -> Result<Self> {
        let mut effective = document.clone();
        let Some(fields) = document
            .get("policy")
            .and_then(|policy| policy.get("fields"))
        else {
            return Self::from_document(document, kind);
        };
        let fields = fields
            .as_object()
            .ok_or_else(|| Error::new(ErrorKind::Usage, "policy.fields must be a table"))?;
        if let Some(values) = fields.get(id) {
            let values = values
                .as_object()
                .ok_or_else(|| Error::new(ErrorKind::Usage, "field policy must be a table"))?;
            for (key, value) in values {
                if !matches!(
                    key.as_str(),
                    "threshold" | "margin" | "min_confidence" | "min_gap"
                ) {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "field policies contain only decision thresholds/gates",
                    ));
                }
                effective["policy"][key] = value.clone();
            }
        }
        Self::from_document(&effective, kind)
    }

    pub fn from_document(document: &serde_json::Value, kind: QuestionKind) -> Result<Self> {
        let mut policy = Self::default();
        if let Some(values) = document.get("policy") {
            let values = values
                .as_object()
                .ok_or_else(|| Error::new(ErrorKind::Usage, "preset policy must be a table"))?;
            for (key, flag) in [
                ("threshold", PolicyFlag::Threshold),
                ("margin", PolicyFlag::Margin),
                ("min_confidence", PolicyFlag::MinConfidence),
                ("min_gap", PolicyFlag::MinGap),
            ] {
                if let Some(value) = values.get(key) {
                    flag.validate(kind)?;
                    let number = Probability::new(value.as_f64().ok_or_else(|| {
                        Error::new(
                            ErrorKind::Usage,
                            format!("preset policy {key} must be a finite number"),
                        )
                    })?)
                    .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
                    match flag {
                        PolicyFlag::Threshold => policy.threshold = number,
                        PolicyFlag::Margin => policy.margin = number.get(),
                        PolicyFlag::MinConfidence => policy.min_confidence = number,
                        PolicyFlag::MinGap => policy.min_gap = number.get(),
                    }
                }
            }
        }
        Ok(policy)
    }

    pub fn validate(&self) -> Result<()> {
        for (name, value) in [("margin", self.margin), ("min-gap", self.min_gap)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(Error::new(
                    ErrorKind::Usage,
                    format!("{name} must be finite and in [0, 1]"),
                ));
            }
        }
        Ok(())
    }

    pub fn noul(&self, probability: Probability) -> Result<Assessment> {
        self.validate()?;
        let value = probability.get();
        Ok(Assessment {
            selected: Selected::Boolean(value >= self.threshold.get()),
            confidence: Probability::new(value.max(1.0 - value))?,
            gap: None,
            uncertain: (value - self.threshold.get()).abs() < self.margin,
        })
    }

    pub fn choice(&self, probabilities: &BTreeMap<String, Probability>) -> Result<Assessment> {
        self.options(probabilities, None)
    }

    pub fn score(
        &self,
        value: f64,
        probabilities: &BTreeMap<String, Probability>,
    ) -> Result<Assessment> {
        if !value.is_finite() {
            return Err(Error::new(ErrorKind::Input, "score must be finite"));
        }
        self.options(probabilities, Some(value))
    }

    fn options(
        &self,
        probabilities: &BTreeMap<String, Probability>,
        score: Option<f64>,
    ) -> Result<Assessment> {
        self.validate()?;
        let mut ranked: Vec<_> = probabilities.iter().collect();
        ranked.sort_by(|(left_key, left), (right_key, right)| {
            right
                .get()
                .total_cmp(&left.get())
                .then_with(|| left_key.cmp(right_key))
        });
        let (label, confidence) = ranked
            .first()
            .ok_or_else(|| Error::new(ErrorKind::Input, "answer has no option probabilities"))?;
        let second = ranked
            .get(1)
            .map_or(0.0, |(_, probability)| probability.get());
        let gap = confidence.get() - second;
        let tied_choice = score.is_none() && ranked.len() > 1 && gap == 0.0;
        Ok(Assessment {
            selected: score.map_or_else(|| Selected::Choice((*label).clone()), Selected::Score),
            confidence: **confidence,
            gap: Some(gap),
            uncertain: confidence.get() < self.min_confidence.get()
                || gap < self.min_gap
                || tied_choice,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::policy::{
        Destination, Filter, Policy, PolicyFlag, Probability, QuestionKind, Selected,
        UncertainAction,
    };
    use std::collections::BTreeMap;

    #[test]
    fn threshold_equality_is_yes_and_margin_is_strict() {
        let mut policy = Policy::default();
        let answer = policy.noul(Probability::new(0.5).unwrap()).unwrap();
        assert_eq!(answer.selected, Selected::Boolean(true));
        assert!(!answer.uncertain);
        policy.margin = 0.125;
        assert!(
            policy
                .noul(Probability::new(0.5).unwrap())
                .unwrap()
                .uncertain
        );
        assert!(
            !policy
                .noul(Probability::new(0.625).unwrap())
                .unwrap()
                .uncertain
        );
    }

    #[test]
    fn probabilities_reject_nan_infinity_and_out_of_range() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.1] {
            assert!(Probability::new(value).is_err());
        }
        assert!(Probability::new(0.0).is_ok());
        assert!(Probability::new(1.0).is_ok());
    }

    #[test]
    fn exact_choice_ties_use_sorted_keys_and_are_uncertain() {
        let options = BTreeMap::from([
            ("z".into(), Probability::new(0.5).unwrap()),
            ("a".into(), Probability::new(0.5).unwrap()),
        ]);
        let result = Policy::default().choice(&options).unwrap();
        assert_eq!(result.selected, Selected::Choice("a".into()));
        assert!(result.uncertain);
    }

    #[test]
    fn confidence_and_gap_have_independent_gates() {
        let options = BTreeMap::from([
            ("a".into(), Probability::new(0.55).unwrap()),
            ("b".into(), Probability::new(0.45).unwrap()),
        ]);
        let mut policy = Policy::default();
        assert!(!policy.choice(&options).unwrap().uncertain);
        policy.min_gap = 0.2;
        assert!(policy.choice(&options).unwrap().uncertain);
        policy.min_gap = 0.0;
        policy.min_confidence = Probability::new(0.6).unwrap();
        assert!(policy.choice(&options).unwrap().uncertain);
    }

    #[test]
    fn score_confidence_is_max_probability_and_empty_options_fail() {
        let options = BTreeMap::from([
            ("0".into(), Probability::new(0.25).unwrap()),
            ("1".into(), Probability::new(0.75).unwrap()),
        ]);
        let result = Policy::default().score(0.75, &options).unwrap();
        assert_eq!(result.selected, Selected::Score(0.75));
        assert_eq!(result.confidence.get(), 0.75);
        assert!(Policy::default().choice(&BTreeMap::new()).is_err());
    }

    #[test]
    fn default_keep_drop_uncertain_buckets_partition_every_probability() {
        let policy = Policy {
            margin: 0.1,
            ..Policy::default()
        };
        for step in 0..=1000_u32 {
            let assessment = policy
                .noul(Probability::new(f64::from(step) / 1000.0).unwrap())
                .unwrap();
            let keep = assessment
                .filter(Filter::Keep, UncertainAction::default())
                .unwrap()
                == Destination::Stdout;
            let drop = assessment
                .filter(Filter::Drop, UncertainAction::default())
                .unwrap()
                == Destination::Stdout;
            assert_eq!(
                u8::from(keep) + u8::from(drop) + u8::from(assessment.uncertain),
                1
            );
        }
    }

    #[test]
    fn explicit_uncertainty_actions_are_separate_from_filter_polarity() {
        let policy = Policy {
            margin: 0.1,
            ..Policy::default()
        };
        let uncertain = policy.noul(Probability::new(0.5).unwrap()).unwrap();
        for filter in [Filter::Keep, Filter::Drop] {
            assert_eq!(
                uncertain.filter(filter, UncertainAction::Keep).unwrap(),
                Destination::Stdout
            );
            assert_eq!(
                uncertain.filter(filter, UncertainAction::Drop).unwrap(),
                Destination::Omit
            );
            assert_eq!(
                uncertain.filter(filter, UncertainAction::Stderr).unwrap(),
                Destination::Stderr
            );
            assert_eq!(
                uncertain
                    .filter(filter, UncertainAction::Fail)
                    .unwrap_err()
                    .exit_code(),
                3
            );
        }
        assert_eq!(uncertain.test_exit_code().unwrap(), 2);
    }

    #[test]
    fn tests_have_distinct_yes_no_uncertain_exit_codes_and_validate_types() {
        let policy = Policy::default();
        assert_eq!(
            policy
                .noul(Probability::new(0.75).unwrap())
                .unwrap()
                .test_exit_code()
                .unwrap(),
            0
        );
        assert_eq!(
            policy
                .noul(Probability::new(0.25).unwrap())
                .unwrap()
                .test_exit_code()
                .unwrap(),
            1
        );
        let options = BTreeMap::from([("a".into(), Probability::new(1.0).unwrap())]);
        let choice = policy.choice(&options).unwrap();
        assert_eq!(
            choice
                .filter(Filter::Keep, UncertainAction::Drop)
                .unwrap_err()
                .exit_code(),
            64
        );
        assert_eq!(choice.test_exit_code().unwrap_err().exit_code(), 64);
    }

    #[test]
    fn inapplicable_policy_flags_are_usage_errors() {
        for kind in [
            QuestionKind::Noul,
            QuestionKind::Choice,
            QuestionKind::Score,
        ] {
            for flag in [PolicyFlag::Threshold, PolicyFlag::Margin] {
                assert_eq!(flag.validate(kind).is_ok(), kind == QuestionKind::Noul);
            }
            for flag in [PolicyFlag::MinConfidence, PolicyFlag::MinGap] {
                assert_eq!(flag.validate(kind).is_ok(), kind != QuestionKind::Noul);
            }
        }
        assert_eq!(
            PolicyFlag::Margin
                .validate(QuestionKind::Score)
                .unwrap_err()
                .exit_code(),
            64
        );
    }
}
