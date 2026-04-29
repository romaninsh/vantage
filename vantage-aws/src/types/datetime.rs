//! AWS timestamp wrapper.
//!
//! AWS Query and JSON-1.1 protocols return timestamps in a few shapes:
//! ISO 8601 with `Z` suffix (`2024-03-15T10:30:00Z`), ISO 8601 with
//! fractional seconds (`2024-03-15T10:30:00.123Z`), and Unix epoch
//! seconds (less common, mostly JSON-1.1). We accept all three on
//! parse and render as `YYYY-MM-DD HH:MM` for display — minute
//! granularity is what's useful in a list view; full precision is one
//! `.into_inner()` away if a caller needs it.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

use vantage_types::{RichText, Style, TerminalRender};

/// Owned timestamp parsed from an AWS response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AwsDateTime(pub DateTime<Utc>);

impl AwsDateTime {
    /// Parse a timestamp. Panics on malformed input.
    pub fn parse(s: &str) -> Self {
        Self::try_parse(s).unwrap_or_else(|e| panic!("invalid AWS timestamp {s:?}: {e}"))
    }

    /// Non-panicking parser. Accepts ISO 8601 with optional fractional
    /// seconds, or a numeric string of Unix epoch seconds (with or
    /// without fractional part).
    pub fn try_parse(s: &str) -> Result<Self, String> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(AwsDateTime(dt.with_timezone(&Utc)));
        }
        // Some AWS responses elide the timezone; treat naive as UTC.
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
            return Ok(AwsDateTime(Utc.from_utc_datetime(&naive)));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Ok(AwsDateTime(Utc.from_utc_datetime(&naive)));
        }
        // Epoch seconds (possibly fractional).
        if let Ok(secs) = s.parse::<f64>() {
            let whole = secs.trunc() as i64;
            let nanos = ((secs.fract()) * 1_000_000_000.0).round() as u32;
            if let Some(dt) = DateTime::<Utc>::from_timestamp(whole, nanos) {
                return Ok(AwsDateTime(dt));
            }
        }
        Err(format!("unrecognised timestamp shape: {s}"))
    }

    pub fn into_inner(self) -> DateTime<Utc> {
        self.0
    }
}

impl fmt::Display for AwsDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // RFC 3339 — the format AWS itself emits.
        write!(f, "{}", self.0.to_rfc3339())
    }
}

impl FromStr for AwsDateTime {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        AwsDateTime::try_parse(s)
    }
}

impl Serialize for AwsDateTime {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for AwsDateTime {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        AwsDateTime::try_parse(&s).map_err(serde::de::Error::custom)
    }
}

impl TerminalRender for AwsDateTime {
    fn render(&self) -> RichText {
        // YYYY-MM-DD HH:MM with date in default and time dim.
        let date = self.0.format("%Y-%m-%d").to_string();
        let time = self.0.format("%H:%M").to_string();
        RichText::new()
            .push(date, Style::Default)
            .push(" ", Style::Default)
            .push(time, Style::Dim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_zulu() {
        let d = AwsDateTime::parse("2024-03-15T10:30:00Z");
        assert_eq!(
            d.0.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2024-03-15 10:30:00"
        );
    }

    #[test]
    fn parses_iso_with_fractional_seconds() {
        let d = AwsDateTime::parse("2024-03-15T10:30:00.123Z");
        assert_eq!(
            d.0.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2024-03-15 10:30:00"
        );
    }

    #[test]
    fn parses_epoch_seconds() {
        // 2024-03-15T10:30:00Z -> 1710498600
        let d = AwsDateTime::parse("1710498600");
        assert_eq!(d.0.format("%Y-%m-%d %H:%M").to_string(), "2024-03-15 10:30");
    }

    #[test]
    #[should_panic(expected = "invalid AWS timestamp")]
    fn panics_on_garbage() {
        let _ = AwsDateTime::parse("not-a-date");
    }

    #[test]
    fn renders_minute_granularity() {
        let d = AwsDateTime::parse("2024-03-15T10:30:45.789Z");
        assert_eq!(d.render().to_plain(), "2024-03-15 10:30");
    }
}
