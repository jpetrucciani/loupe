use std::ops::Range;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::candidates::{Candidate, Document};
use crate::error::{Error, ErrorKind, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Dates,
    Money,
    Semver,
    Emails,
    Hostnames,
    Tickers,
}

impl FromStr for Kind {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "dates" | "date" => Ok(Self::Dates),
            "money" => Ok(Self::Money),
            "semver" | "versions" => Ok(Self::Semver),
            "emails" | "email" => Ok(Self::Emails),
            "hostnames" | "hostname" => Ok(Self::Hostnames),
            "tickers" | "ticker" => Ok(Self::Tickers),
            _ => Err(Error::new(
                ErrorKind::Usage,
                "--candidates must be dates|money|semver|emails|hostnames|tickers",
            )),
        }
    }
}

/// Compiled once per invocation. Ranges always address original UTF-8 bytes;
/// parsing neither rewrites the source nor accepts model-generated spans.
pub struct Parser {
    kind: Kind,
    pattern: Regex,
}

impl Parser {
    pub fn new(kind: Kind) -> Result<Self> {
        let label = r"[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?";
        let domain = format!(r"{label}(?:\.{label})+");
        let atom = r"[A-Za-z0-9!#$%&'*+/=?^_`{|}~-]+";
        let pattern = match kind {
            Kind::Dates => {
                let month = "(?:January|February|March|April|May|June|July|August|September|October|November|December|Jan|Feb|Mar|Apr|Jun|Jul|Aug|Sep|Sept|Oct|Nov|Dec)";
                let weekday = "(?:Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday)";
                format!(r"(?i)[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}(?:[T ][0-9]{{2}}:[0-9]{{2}}(?::[0-9]{{2}}(?:\.[0-9]+)?)?(?:Z|[+-][0-9]{{2}}:[0-9]{{2}})?)?|[0-9]{{1,2}}/[0-9]{{1,2}}/[0-9]{{4}}|{month}\.?[ \t]+[0-9]{{1,2}}(?:st|nd|rd|th)?(?:,?[ \t]+[0-9]{{4}})?|[0-9]{{1,2}}(?:st|nd|rd|th)?[ \t]+{month}\.?(?:[ \t]+[0-9]{{4}})?|day[ \t]+after[ \t]+tomorrow|day[ \t]+before[ \t]+yesterday|today|tomorrow|yesterday|(?:next|last|this)[ \t]+{weekday}|in[ \t]+[0-9]+[ \t]+(?:days?|weeks?|months?|years?)|[0-9]+[ \t]+(?:days?|weeks?|months?|years?)[ \t]+ago")
            }
            Kind::Money => {
                let currency = r"(?:[A-Z]{3}|[$€£¥₹₩₽₿])";
                let number = r"[0-9]+(?:[,. '’\x{00a0}][0-9]+)*(?:[eE][+-]?[0-9]+)?";
                let magnitude = r"(?:[ \t]+(?:thousand|million|billion|trillion)|[kK])?";
                format!(r"(?:\()?[-+−]?{currency}[ \t]*[-+−]?{number}{magnitude}(?:[ \t]+[A-Z]{{3}})?(?:\))?|(?:\()?[-+−]?{number}{magnitude}[ \t]*{currency}(?:\))?")
            }
            Kind::Semver => r"v?(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?".to_owned(),
            Kind::Emails => format!(r#"(?:{atom}(?:\.{atom})*|"(?:[^"\\\r\n]|\\[\x20-\x7e])+")@(?:{domain}|\[(?:IPv6:)?[0-9A-Fa-f:.]+\])"#),
            Kind::Hostnames => format!(r"{domain}|localhost"),
            Kind::Tickers => r"(?:[A-Z][A-Z0-9_-]{0,15}:|\$)?[A-Z][A-Z0-9]{0,11}(?:[.-][A-Z0-9]{1,6})?".to_owned(),
        };
        Ok(Self {
            kind,
            pattern: Regex::new(&pattern).map_err(|error| {
                Error::new(ErrorKind::Config, format!("candidate parser: {error}"))
            })?,
        })
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn parse<'a>(&self, document: &Document<'a>) -> Result<Vec<Candidate<'a>>> {
        document.parsed(self.spans(document.text()))
    }

    pub fn spans(&self, text: &str) -> Vec<Range<usize>> {
        self.pattern
            .find_iter(text)
            .filter(|value| {
                boundary(text, value.range(), self.kind) && valid(value.as_str(), self.kind)
            })
            .map(|value| value.range())
            .collect()
    }
}

fn boundary(text: &str, range: Range<usize>, kind: Kind) -> bool {
    let before = text.get(..range.start).and_then(|s| s.chars().next_back());
    let after = text.get(range.end..).and_then(|s| s.chars().next());
    let continuation = |value: char| {
        value.is_alphanumeric()
            || value == '_'
            || match kind {
                Kind::Dates => matches!(value, '/' | ':' | '+' | '-'),
                Kind::Money => matches!(value, '$' | '€' | '£' | '¥' | '₹' | '₩' | '₽' | '₿'),
                Kind::Semver => matches!(value, '+' | '-'),
                Kind::Emails => "!#$%&'*+/=?^`{|}~@-".contains(value),
                Kind::Hostnames | Kind::Tickers => value == '-',
            }
    };
    if before.is_some_and(continuation) || after.is_some_and(continuation) {
        return false;
    }
    // A final prose period is a delimiter; a period continuing a malformed
    // token must not allow a valid-looking substring to become a candidate.
    if before == Some('.') {
        return false;
    }
    if after == Some('.') {
        let following = text.get(range.end..).and_then(|s| s.chars().nth(1));
        if following.is_some_and(|value| continuation(value) || value == '.') {
            return false;
        }
    }
    true
}

fn valid(value: &str, kind: Kind) -> bool {
    match kind {
        Kind::Dates => valid_date(value),
        Kind::Money => value.starts_with('(') == value.ends_with(')'),
        Kind::Semver => semver::Version::parse(value.strip_prefix('v').unwrap_or(value)).is_ok(),
        Kind::Emails => value.rsplit_once('@').is_some_and(|(_, host)| {
            if let Some(literal) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                literal
                    .strip_prefix("IPv6:")
                    .unwrap_or(literal)
                    .parse::<std::net::IpAddr>()
                    .is_ok()
            } else {
                valid_host(host)
            }
        }),
        Kind::Hostnames => valid_host(value),
        Kind::Tickers => true,
    }
}

fn valid_host(value: &str) -> bool {
    value.len() <= 253
        && value
            .rsplit('.')
            .next()
            .is_some_and(|label| label.bytes().any(|byte| byte.is_ascii_alphabetic()))
}

fn valid_date(value: &str) -> bool {
    if value.as_bytes().get(4) == Some(&b'-') {
        if let Some(date) = value.get(..10) {
            if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                return false;
            }
            if value.len() == 10 {
                return true;
            }
            if value.ends_with(['Z', 'z'])
                || value
                    .get(19..)
                    .is_some_and(|tail| tail.contains(['+', '-']))
            {
                return DateTime::parse_from_rfc3339(value).is_ok();
            }
            let time = value.get(11..).unwrap_or("");
            return ["%H:%M", "%H:%M:%S", "%H:%M:%S%.f"]
                .iter()
                .any(|format| chrono::NaiveTime::parse_from_str(time, format).is_ok());
        }
        return false;
    }
    if value.contains('/') {
        return ["%m/%d/%Y", "%d/%m/%Y"]
            .iter()
            .any(|format| NaiveDate::parse_from_str(value, format).is_ok());
    }
    let words = value.split_whitespace().collect::<Vec<_>>();
    if let [first, second, tail @ ..] = words.as_slice() {
        let month = |word: &str| match word.to_ascii_lowercase().get(..3) {
            Some("jan") => Some(1),
            Some("feb") => Some(2),
            Some("mar") => Some(3),
            Some("apr") => Some(4),
            Some("may") => Some(5),
            Some("jun") => Some(6),
            Some("jul") => Some(7),
            Some("aug") => Some(8),
            Some("sep") => Some(9),
            Some("oct") => Some(10),
            Some("nov") => Some(11),
            Some("dec") => Some(12),
            _ => None,
        };
        let (month, day) = if let Some(month) = month(first) {
            (Some(month), *second)
        } else {
            (month(second), *first)
        };
        if let Some(month) = month {
            let day = day.trim_end_matches(',').to_ascii_lowercase();
            let day = ["st", "nd", "rd", "th"]
                .iter()
                .find_map(|suffix| day.strip_suffix(suffix))
                .unwrap_or(&day);
            let year = tail.first().map_or(Some(2000), |year| year.parse().ok());
            return day
                .parse()
                .ok()
                .zip(year)
                .is_some_and(|(day, year)| NaiveDate::from_ymd_opt(year, month, day).is_some());
        }
    }
    // Relative forms require an explicit clock/timezone at normalization.
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::candidates::Document;
    use crate::extract::{Kind, Parser};

    fn values(kind: Kind, text: &str) -> Vec<&str> {
        let parser = Parser::new(kind).unwrap();
        parser
            .spans(text)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn dates_cover_absolute_relative_and_yearless_forms_without_invalid_iso_fragments() {
        let text = "2024-02-29 2026-02-30 2026-10-06T12:30:45.125-04:00 2026-10-06T25:30:00Z 31/12/2026 October 6th, 2026 6 Oct 2026 tomorrow day after tomorrow next Monday in 3 months 2 days ago March 10";
        assert_eq!(
            values(Kind::Dates, text),
            vec![
                "2024-02-29",
                "2026-10-06T12:30:45.125-04:00",
                "31/12/2026",
                "October 6th, 2026",
                "6 Oct 2026",
                "tomorrow",
                "day after tomorrow",
                "next Monday",
                "in 3 months",
                "2 days ago",
                "March 10"
            ]
        );
        assert!(values(Kind::Dates, "February 30, 2026 31 April 2026 March 0").is_empty());
        assert_eq!(
            values(
                Kind::Dates,
                "February 29 February 29, 2024 February 29, 2025"
            ),
            vec!["February 29", "February 29, 2024"]
        );
    }

    #[test]
    fn money_preserves_currency_sign_grouping_magnitude_and_parentheses() {
        let text = "USD 1,234.50; -€1.234,56; (£20); 125.00 CAD; ₹1,00,000; $2.5 million; CHF 1’234.00; USD 1e6; ($12; $14)";
        assert_eq!(
            values(Kind::Money, text),
            vec![
                "USD 1,234.50",
                "-€1.234,56",
                "(£20)",
                "125.00 CAD",
                "₹1,00,000",
                "$2.5 million",
                "CHF 1’234.00",
                "USD 1e6"
            ]
        );
    }

    #[test]
    fn versions_keep_prefix_metadata_and_reject_malformed_partial_tokens() {
        let text = "v1.2.3-alpha.1+build.001, 0.0.0. 01.2.3 1.2.3-alpha.01 1.2.3.4 1.2.3+ α1.2.3";
        assert_eq!(
            values(Kind::Semver, text),
            vec!["v1.2.3-alpha.1+build.001", "0.0.0"]
        );
    }

    #[test]
    fn addresses_accept_dot_atoms_quoted_locals_and_ip_literals_without_partial_invalid_atoms() {
        let text = r#"a+b@example.com "a b"@example.org admin@[127.0.0.1] root@[IPv6:2001:db8::1] a..b@example.com αfoo@example.net x@[999.1.2.3]"#;
        assert_eq!(
            values(Kind::Emails, text),
            vec![
                "a+b@example.com",
                r#""a b"@example.org"#,
                "admin@[127.0.0.1]",
                "root@[IPv6:2001:db8::1]"
            ]
        );
        assert_eq!(
            values(Kind::Emails, "Contact: a@example.com.\r\n"),
            vec!["a@example.com"]
        );
    }

    #[test]
    fn hosts_and_tickers_are_lexical_source_spans_with_explicit_boundaries() {
        assert_eq!(
            values(
                Kind::Hostnames,
                "https://api.example.com/path db.internal. localhost 1.2.3 -bad.example.com a..example.com"
            ),
            vec!["api.example.com", "db.internal", "localhost"]
        );
        assert!(values(Kind::Hostnames, &format!("{}.example.com", "a".repeat(64))).is_empty());
        assert_eq!(
            values(
                Kind::Tickers,
                "$AAPL NYSE:BRK.B BRK-B CME:ESZ6 BTC-USD 123 abcAAPL $123 αAAPL"
            ),
            vec!["$AAPL", "NYSE:BRK.B", "BRK-B", "CME:ESZ6", "BTC-USD"]
        );
    }

    #[test]
    fn every_parser_keeps_utf8_bytes_physical_lines_and_duplicate_values_at_distinct_offsets() {
        let cases = [
            (Kind::Dates, "2026-10-06"),
            (Kind::Money, "€12.50"),
            (Kind::Semver, "v1.2.3"),
            (Kind::Emails, "a@example.com"),
            (Kind::Hostnames, "api.example.com"),
            (Kind::Tickers, "$AAPL"),
        ];
        for (kind, value) in cases {
            let text = format!("東京🙂\r\n{value}\r\n\r\n{value}");
            let document = Document::new(&text, Some("fixture.txt".into()));
            let parser = Parser::new(kind).unwrap();
            let catalog = parser.parse(&document).unwrap();
            assert_eq!(catalog.len(), 2, "{kind:?}");
            for (position, candidate) in catalog.iter().enumerate() {
                assert_eq!(candidate.id.to_string(), format!("C{}", position + 1));
                assert_eq!(candidate.verbatim, value);
                assert_eq!(
                    &text.as_bytes()[candidate.location.byte_start as usize
                        ..candidate.location.byte_end as usize],
                    value.as_bytes()
                );
                assert_eq!(
                    candidate.location.line_start,
                    if position == 0 { 2 } else { 4 }
                );
            }
            let offset = "東京🙂\r\n".len();
            let chunk = offset..text.len();
            let local = parser.spans(document.slice(chunk.clone()).unwrap());
            let mapped = document.map_chunk(chunk, &local).unwrap();
            let rebuilt = document.parsed(mapped).unwrap();
            assert_eq!(
                serde_json::to_value(catalog).unwrap(),
                serde_json::to_value(rebuilt).unwrap()
            );
        }
    }
}
