//! Deterministic conversion of selected verbatim spans, without model generation.

use chrono::{
    DateTime, Datelike, Days, LocalResult, Months, NaiveDate, NaiveDateTime, SecondsFormat,
    TimeZone, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::extract::Kind;

pub const TIMEZONE_DATA_VERSION: &str = chrono_tz::IANA_TZDB_VERSION;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub now: Option<String>,
    pub tz: Option<String>,
    pub currency: Option<String>,
    pub decimal_separator: Option<Separator>,
    pub date_order: Option<DateOrder>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Separator {
    Dot,
    Comma,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DateOrder {
    Mdy,
    Dmy,
}

impl std::str::FromStr for Separator {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "dot" => Ok(Self::Dot),
            "comma" => Ok(Self::Comma),
            _ => Err(usage("decimal separator must be dot|comma")),
        }
    }
}
impl std::str::FromStr for DateOrder {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "mdy" => Ok(Self::Mdy),
            "dmy" => Ok(Self::Dmy),
            _ => Err(usage("date order must be mdy|dmy")),
        }
    }
}

pub struct Normalizer {
    pub options: Options,
    now: Option<DateTime<Utc>>,
    tz: Option<Tz>,
}

#[derive(Debug, Serialize)]
pub struct Normalized {
    pub value: Value,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<&'static str>,
}

impl Normalizer {
    pub fn for_kind(kind: Kind, options: Options) -> Result<Self> {
        if kind != Kind::Dates
            && (options.now.is_some() || options.tz.is_some() || options.date_order.is_some())
            || kind != Kind::Money
                && (options.currency.is_some() || options.decimal_separator.is_some())
        {
            return Err(usage(
                "date/currency normalization options require the corresponding candidate kind",
            ));
        }
        Self::new(options)
    }
    pub fn new(options: Options) -> Result<Self> {
        let now = options
            .now
            .as_deref()
            .map(|value| {
                DateTime::parse_from_rfc3339(value)
                    .map(|value| value.with_timezone(&Utc))
                    .map_err(|_| usage("now must be an RFC 3339 timestamp with an offset"))
            })
            .transpose()?;
        let tz = options
            .tz
            .as_deref()
            .map(|value| {
                value.parse::<Tz>().map_err(|_| {
                    usage("tz must be an IANA timezone, such as UTC or America/New_York")
                })
            })
            .transpose()?;
        if let Some(currency) = &options.currency {
            validate_currency(currency)?;
        }
        Ok(Self { options, now, tz })
    }

    pub fn normalize(&self, kind: Kind, span: &str) -> Result<Normalized> {
        match kind {
            Kind::Dates => self.date(span),
            Kind::Money => self.money(span),
            Kind::Semver => {
                let version = semver::Version::parse(span.strip_prefix('v').unwrap_or(span))
                    .map_err(|error| input(&format!("invalid semver: {error}")))?
                    .to_string();
                Ok(Normalized {
                    value: json!(version),
                    text: version,
                    precision: None,
                })
            }
            _ => Ok(Normalized {
                value: json!(span),
                text: span.to_owned(),
                precision: None,
            }),
        }
    }

    fn anchor(&self) -> Result<NaiveDate> {
        let now = self
            .now
            .ok_or_else(|| input("relative/yearless dates require --now"))?;
        let tz = self
            .tz
            .ok_or_else(|| input("relative/yearless dates require --tz"))?;
        Ok(now.with_timezone(&tz).date_naive())
    }

    fn date(&self, span: &str) -> Result<Normalized> {
        if let Ok(timestamp) = DateTime::parse_from_rfc3339(span) {
            let text = timestamp.to_rfc3339_opts(SecondsFormat::AutoSi, true);
            return Ok(Normalized {
                value: json!(text),
                text,
                precision: Some("instant"),
            });
        }
        let naive = [
            "%Y-%m-%dT%H:%M",
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%d %H:%M",
            "%Y-%m-%d %H:%M:%S%.f",
        ]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(span, format).ok());
        let (naive, precision) = if let Some(naive) = naive {
            (naive, "instant")
        } else {
            let date = if let Ok(date) = NaiveDate::parse_from_str(span, "%Y-%m-%d") {
                date
            } else if span.contains('/') {
                let mdy = NaiveDate::parse_from_str(span, "%m/%d/%Y").ok();
                let dmy = NaiveDate::parse_from_str(span, "%d/%m/%Y").ok();
                match (self.options.date_order, mdy, dmy) {
                    (Some(DateOrder::Mdy), Some(date), _)
                    | (Some(DateOrder::Dmy), _, Some(date)) => date,
                    (None, Some(a), Some(b)) if a == b => a,
                    (None, Some(date), None) | (None, None, Some(date)) => date,
                    (None, Some(_), Some(_)) => {
                        return Err(input(
                            "ambiguous numeric date; provide --date-order mdy|dmy",
                        ));
                    }
                    _ => return Err(input("invalid numeric date for the specified date order")),
                }
            } else {
                self.named_or_relative(span)?
            };
            (
                date.and_hms_opt(0, 0, 0)
                    .ok_or_else(|| input("date outside clock range"))?,
                "day",
            )
        };
        let tz = self
            .tz
            .ok_or_else(|| input("dates without a source offset require --tz"))?;
        let timestamp = match tz.from_local_datetime(&naive) {
            LocalResult::Single(value) => value,
            LocalResult::Ambiguous(_, _) => {
                return Err(input(
                    "ambiguous local time during timezone transition; use an explicit source offset",
                ));
            }
            LocalResult::None => {
                return Err(input("local time does not exist in the selected timezone"));
            }
        };
        let text = timestamp.to_rfc3339_opts(SecondsFormat::AutoSi, true);
        Ok(Normalized {
            value: json!(text),
            text,
            precision: Some(precision),
        })
    }

    fn named_or_relative(&self, span: &str) -> Result<NaiveDate> {
        let lower = span.to_ascii_lowercase();
        let words = lower
            .split_whitespace()
            .map(|word| word.trim_end_matches([',', '.']))
            .collect::<Vec<_>>();
        if let [a, b, tail @ ..] = words.as_slice() {
            let (month, day) = if let Some(month) = month(a) {
                (Some(month), *b)
            } else {
                (month(b), *a)
            };
            if let Some(month) = month {
                let day = day
                    .trim_end_matches(|value: char| value.is_ascii_alphabetic())
                    .parse()
                    .map_err(|_| input("invalid day"))?;
                let year = match tail {
                    [] => self.anchor()?.year(),
                    [year] => year.parse().map_err(|_| input("invalid year"))?,
                    _ => return Err(input("invalid date")),
                };
                return NaiveDate::from_ymd_opt(year, month, day)
                    .ok_or_else(|| input("invalid calendar date"));
            }
        }
        let base = self.anchor()?;
        let days = match words.as_slice() {
            ["today"] => Some(0),
            ["tomorrow"] => Some(1),
            ["yesterday"] => Some(-1),
            ["day", "after", "tomorrow"] => Some(2),
            ["day", "before", "yesterday"] => Some(-2),
            [direction, day] if matches!(*direction, "next" | "last" | "this") => {
                let target = [
                    "monday",
                    "tuesday",
                    "wednesday",
                    "thursday",
                    "friday",
                    "saturday",
                    "sunday",
                ]
                .iter()
                .position(|value| value == day)
                .ok_or_else(|| input("invalid weekday"))?;
                let difference = i64::try_from(target).map_err(|_| input("weekday overflow"))?
                    - i64::from(base.weekday().num_days_from_monday());
                Some(match *direction {
                    "next" => {
                        if difference > 0 {
                            difference
                        } else {
                            difference + 7
                        }
                    }
                    "last" => {
                        if difference < 0 {
                            difference
                        } else {
                            difference - 7
                        }
                    }
                    _ => difference,
                })
            }
            _ => None,
        };
        if let Some(days) = days {
            return shift_days(base, days);
        }
        let (number, unit, negative) = match words.as_slice() {
            ["in", number, unit] => (*number, *unit, false),
            [number, unit, "ago"] => (*number, *unit, true),
            _ => return Err(input("unsupported date syntax")),
        };
        let number = number
            .parse::<u32>()
            .map_err(|_| input("relative date quantity outside range"))?;
        let count = match unit.trim_end_matches('s') {
            "day" => {
                return shift_days(
                    base,
                    if negative {
                        -i64::from(number)
                    } else {
                        i64::from(number)
                    },
                );
            }
            "week" => {
                return shift_days(
                    base,
                    if negative {
                        -i64::from(number) * 7
                    } else {
                        i64::from(number) * 7
                    },
                );
            }
            "month" => number,
            "year" => number
                .checked_mul(12)
                .ok_or_else(|| input("relative year overflow"))?,
            _ => return Err(input("unsupported relative date unit")),
        };
        (if negative {
            base.checked_sub_months(Months::new(count))
        } else {
            base.checked_add_months(Months::new(count))
        })
        .ok_or_else(|| input("relative date outside calendar range"))
    }

    fn money(&self, span: &str) -> Result<Normalized> {
        let mut body = span.trim().replace('−', "-");
        let parentheses = body.starts_with('(') && body.ends_with(')');
        if parentheses {
            body = body
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
                .ok_or_else(|| input("invalid money parentheses"))?
                .to_owned();
        }
        let mut signs = Vec::new();
        let mut codes = Vec::new();
        let mut symbols = Vec::new();
        loop {
            body = body.trim().to_owned();
            if body.starts_with(['+', '-']) {
                signs.push(body.remove(0));
                continue;
            }
            let prefix = body
                .get(..3)
                .filter(|part| part.bytes().all(|byte| byte.is_ascii_uppercase()));
            let suffix = body
                .len()
                .checked_sub(3)
                .and_then(|start| body.get(start..))
                .filter(|part| part.bytes().all(|byte| byte.is_ascii_uppercase()));
            if let Some(code) = prefix {
                codes.push(code.to_owned());
                body.drain(..3);
                continue;
            }
            if let Some(code) = suffix {
                codes.push(code.to_owned());
                body.truncate(body.len() - 3);
                continue;
            }
            let symbol = body
                .chars()
                .next()
                .filter(|value| "$€£¥₹₩₽₿".contains(*value));
            let last = body
                .chars()
                .next_back()
                .filter(|value| "$€£¥₹₩₽₿".contains(*value));
            if let Some(symbol) = symbol {
                symbols.push(symbol);
                body.drain(..symbol.len_utf8());
                continue;
            }
            if let Some(symbol) = last {
                symbols.push(symbol);
                body.truncate(body.len() - symbol.len_utf8());
                continue;
            }
            break;
        }
        if signs.len() > 1 || parentheses && !signs.is_empty() {
            return Err(input("conflicting money signs"));
        }
        let currency = codes
            .first()
            .cloned()
            .or_else(|| self.options.currency.clone())
            .or_else(|| match symbols.as_slice() {
                ['€'] => Some("EUR".into()),
                ['£'] => Some("GBP".into()),
                ['₹'] => Some("INR".into()),
                ['₩'] => Some("KRW".into()),
                ['₽'] => Some("RUB".into()),
                _ => None,
            })
            .ok_or_else(|| input("ambiguous/missing ISO currency; provide --currency"))?;
        validate_currency(&currency)
            .map_err(|_| input("span contains an unsupported ISO currency code"))?;
        if codes.iter().any(|code| code != &currency)
            || self
                .options
                .currency
                .as_ref()
                .is_some_and(|code| code != &currency)
        {
            return Err(input(
                "source currency conflicts with --currency or another source code",
            ));
        }
        for symbol in symbols {
            let compatible = match symbol {
                '$' => [
                    "USD", "CAD", "AUD", "NZD", "HKD", "SGD", "BBD", "BMD", "BSD", "BZD", "FJD",
                    "GYD", "JMD", "KYD", "LRD", "NAD", "SBD", "SRD", "TTD", "TWD", "XCD",
                ]
                .contains(&currency.as_str()),
                '¥' => ["JPY", "CNY"].contains(&currency.as_str()),
                '€' => currency == "EUR",
                '£' => currency == "GBP",
                '₹' => currency == "INR",
                '₩' => currency == "KRW",
                '₽' => currency == "RUB",
                _ => false,
            };
            if !compatible {
                return Err(input("currency symbol conflicts with the ISO currency"));
            }
        }
        let amount = decimal(
            &body,
            self.options.decimal_separator,
            parentheses || signs.first() == Some(&'-'),
        )?;
        let text = format!("{amount} {currency}");
        Ok(Normalized {
            value: json!({"amount":amount,"currency":currency}),
            text,
            precision: None,
        })
    }
}

fn decimal(number: &str, separator: Option<Separator>, negative: bool) -> Result<String> {
    let mut number = number.trim().to_owned();
    let mut shift = 0i32;
    for (suffix, power) in [
        ("thousand", 3),
        ("million", 6),
        ("billion", 9),
        ("trillion", 12),
        ("k", 3),
        ("K", 3),
    ] {
        if let Some(prefix) = number.strip_suffix(suffix) {
            number = prefix.trim().to_owned();
            shift = power;
            break;
        }
    }
    if let Some(position) = number.find(['e', 'E']) {
        let exponent = number[position + 1..]
            .parse::<i32>()
            .map_err(|_| input("invalid money exponent"))?;
        shift = shift
            .checked_add(exponent)
            .filter(|value| value.unsigned_abs() <= 1024)
            .ok_or_else(|| input("money magnitude outside supported range"))?;
        number.truncate(position);
    }
    let decimal = match separator {
        Some(Separator::Dot) => Some('.'),
        Some(Separator::Comma) => Some(','),
        None => match (number.rfind('.'), number.rfind(',')) {
            (Some(a), Some(b)) => Some(if a > b { '.' } else { ',' }),
            (Some(position), None) | (None, Some(position)) => {
                let mark = number.as_bytes()[position] as char;
                let occurrences = number.matches(mark).count();
                let fraction = number.len() - position - 1;
                if occurrences == 1 && fraction == 3 {
                    return Err(input(
                        "ambiguous money separator; provide --decimal-separator dot|comma",
                    ));
                }
                (occurrences == 1).then_some(mark)
            }
            _ => None,
        },
    };
    let (whole, fraction) = if let Some(mark) = decimal {
        number.rsplit_once(mark).unwrap_or((&number, ""))
    } else {
        (number.as_str(), "")
    };
    if decimal.is_some_and(|mark| whole.contains(mark)) {
        return Err(input("repeated decimal separator"));
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(input("invalid money decimal fraction"));
    }
    let groups = whole
        .split([',', '.', ' ', '\'', '’', '\u{a0}'])
        .collect::<Vec<_>>();
    if groups
        .iter()
        .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || groups.len() > 1
            && (groups[0].len() > 3 || groups.iter().skip(1).any(|part| part.len() != 3))
    {
        return Err(input("invalid money digit grouping"));
    }
    let mut digits = groups.concat() + fraction;
    let point = i64::try_from(digits.len()).map_err(|_| input("money too long"))?
        - i64::try_from(fraction.len()).map_err(|_| input("money too long"))?
        + i64::from(shift);
    if digits.len() > 4096 || !(-1024..=4096).contains(&point) {
        return Err(input("normalized money exceeds digit bound"));
    }
    if point <= 0 {
        digits = format!(
            "0.{}{}",
            "0".repeat(usize::try_from(-point).map_err(|_| input("money overflow"))?),
            digits
        );
    } else {
        let point = usize::try_from(point).map_err(|_| input("money overflow"))?;
        if point >= digits.len() {
            digits.push_str(&"0".repeat(point - digits.len()));
        } else {
            digits.insert(point, '.');
        }
    }
    let (whole, fraction) = digits.split_once('.').unwrap_or((&digits, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    let canonical = if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    };
    Ok(if negative && canonical != "0" {
        format!("-{canonical}")
    } else {
        canonical
    })
}

fn shift_days(base: NaiveDate, days: i64) -> Result<NaiveDate> {
    (if days < 0 {
        base.checked_sub_days(Days::new(days.unsigned_abs()))
    } else {
        base.checked_add_days(Days::new(days.unsigned_abs()))
    })
    .ok_or_else(|| input("relative date outside calendar range"))
}
fn month(value: &str) -> Option<u32> {
    [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|month| value.get(..3) == Some(*month))
    .and_then(|index| u32::try_from(index + 1).ok())
}

fn validate_currency(value: &str) -> Result<()> {
    // ISO 4217 Maintenance Agency list-one.xml, published 2026-09-17.
    const CODES: &str = "AED AFN ALL AMD AOA ARS AUD AWG AZN BAM BBD BDT BHD BIF BMD BND BOB BOV BRL BSD BTN BWP BYN BZD CAD CDF CHE CHF CHW CLF CLP CNY COP COU CRC CUP CVE CZK DJF DKK DOP DZD EGP ERN ETB EUR FJD FKP GBP GEL GHS GIP GMD GNF GTQ GYD HKD HNL HTG HUF IDR ILS INR IQD IRR ISK JMD JOD JPY KES KGS KHR KMF KPW KRW KWD KYD KZT LAK LBP LKR LRD LSL LYD MAD MDL MGA MKD MMK MNT MOP MRU MUR MVR MWK MXN MXV MYR MZN NAD NGN NIO NOK NPR NZD OMR PAB PEN PGK PHP PKR PLN PYG QAR RON RSD RUB RWF SAR SBD SCR SDG SEK SGD SHP SLE SOS SRD SSP STN SVC SYP SZL THB TJS TMT TND TOP TRY TTD TWD TZS UAH UGX USD USN UYI UYU UYW UZS VED VES VND VUV WST XAD XAF XAG XAU XBA XBB XBC XBD XCD XCG XDR XOF XPD XPF XPT XSU XTS XUA XXX YER ZAR ZMW ZWG";
    if CODES.split_whitespace().any(|code| code == value) {
        Ok(())
    } else {
        Err(usage(
            "currency must be a supported uppercase ISO 4217 code",
        ))
    }
}
fn usage(value: &str) -> Error {
    Error::new(ErrorKind::Usage, value)
}
fn input(value: &str) -> Error {
    Error::new(ErrorKind::Input, format!("normalization: {value}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::extract::Kind;
    use crate::normalize::{Normalizer, Options, Separator};
    #[test]
    fn dates_resolve_calendar_days_and_refuse_missing_or_ambiguous_information() {
        let normalize = Normalizer::new(Options {
            now: Some("2026-10-31T23:30:00-04:00".into()),
            tz: Some("America/New_York".into()),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(
            normalize.normalize(Kind::Dates, "tomorrow").unwrap().text,
            "2026-11-01T00:00:00-04:00"
        );
        assert_eq!(
            normalize.normalize(Kind::Dates, "in 1 month").unwrap().text,
            "2026-11-30T00:00:00-05:00"
        );
        assert!(
            normalize
                .normalize(Kind::Dates, "2026-11-01T01:30:00")
                .is_err()
        );
        assert!(
            normalize
                .normalize(Kind::Dates, "2026-03-08T02:30:00")
                .is_err()
        );
        assert!(normalize.normalize(Kind::Dates, "01/02/2026").is_err());
        assert_eq!(
            normalize.normalize(Kind::Dates, "31/12/2026").unwrap().text,
            "2026-12-31T00:00:00-05:00"
        );
        let missing = Normalizer::new(Options::default()).unwrap();
        assert!(missing.normalize(Kind::Dates, "today").is_err());
        assert!(missing.normalize(Kind::Dates, "2026-10-06").is_err());
        assert_eq!(
            missing
                .normalize(Kind::Dates, "2026-10-06T12:30:45.125Z")
                .unwrap()
                .text,
            "2026-10-06T12:30:45.125Z"
        );
    }
    #[test]
    fn decimal_money_is_exact_and_requires_currency_and_separator_disambiguation() {
        let normalize = Normalizer::new(Options::default()).unwrap();
        for (span, expected) in [
            ("USD 1,234.50", "1234.5 USD"),
            ("EUR 1.234,50", "1234.5 EUR"),
            ("(EUR 20.00)", "-20 EUR"),
            ("USD 0.1 million", "100000 USD"),
            ("USD 1e-3", "0.001 USD"),
            ("USD 9007199254740993.01", "9007199254740993.01 USD"),
            ("EUR -0.00", "0 EUR"),
        ] {
            assert_eq!(
                normalize.normalize(Kind::Money, span).unwrap().text,
                expected
            );
        }
        for span in ["$20", "¥20", "BTC 2", "USD 1,234", "USD 1 23", "-$-20"] {
            assert!(normalize.normalize(Kind::Money, span).is_err(), "{span}");
        }
        let usd = Normalizer::new(Options {
            currency: Some("USD".into()),
            decimal_separator: Some(Separator::Dot),
            ..Options::default()
        })
        .unwrap();
        assert_eq!(
            usd.normalize(Kind::Money, "$1,234").unwrap().text,
            "1234 USD"
        );
        assert!(usd.normalize(Kind::Money, "EUR 20").is_err());
    }
    #[test]
    fn semver_preserves_prerelease_and_build_and_rejects_invalid_identifiers() {
        let normalize = Normalizer::new(Options::default()).unwrap();
        assert_eq!(
            normalize
                .normalize(Kind::Semver, "v1.2.3-rc.1+build.01")
                .unwrap()
                .text,
            "1.2.3-rc.1+build.01"
        );
        for span in ["01.2.3", "1.2.3-01", "1.2"] {
            assert!(normalize.normalize(Kind::Semver, span).is_err());
        }
    }
}
