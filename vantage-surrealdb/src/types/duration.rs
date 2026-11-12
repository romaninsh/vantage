//! `std::time::Duration` as SurrealDB's compact duration (CBOR tag 14,
//! `[seconds, nanos]` with trailing zeros left out). Decoding also accepts
//! tag 13 over duration text (`1h30m`), and so does the lenient decode over
//! plain text.

use std::time::Duration;

use ciborium::Value as CborValue;

use crate::types::{SurrealType, SurrealTypeDurationMarker};

impl SurrealType for Duration {
    type Target = SurrealTypeDurationMarker;

    fn to_cbor(&self) -> CborValue {
        let mut parts = Vec::new();
        if self.as_secs() > 0 || self.subsec_nanos() > 0 {
            parts.push(CborValue::Integer(self.as_secs().into()));
        }
        if self.subsec_nanos() > 0 {
            parts.push(CborValue::Integer(self.subsec_nanos().into()));
        }
        CborValue::Tag(14, Box::new(CborValue::Array(parts)))
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(14, inner) => {
                let CborValue::Array(parts) = *inner else {
                    return None;
                };
                let int = |i: usize| match parts.get(i) {
                    None => Some(0u64),
                    Some(CborValue::Integer(n)) => u64::try_from(*n).ok(),
                    Some(_) => None,
                };
                if parts.len() > 2 {
                    return None;
                }
                Some(Duration::new(int(0)?, u32::try_from(int(1)?).ok()?))
            }
            CborValue::Tag(13, inner) => match *inner {
                CborValue::Text(s) => parse_duration(&s),
                _ => None,
            },
            _ => None,
        }
    }

    fn coerce_cbor(value: CborValue) -> Option<Self> {
        match value {
            CborValue::Text(s) => parse_duration(&s),
            other => Self::from_cbor(other),
        }
    }
}

/// SurrealDB duration text: one or more `<integer><unit>`, units `y w d h m
/// s ms us µs ns` (a year is 365 days).
fn parse_duration(text: &str) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut rest = text.trim();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        if digits == 0 {
            return None;
        }
        let amount: u64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let unit_len = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(rest.len());
        let part = match &rest[..unit_len] {
            "ns" => Duration::from_nanos(amount),
            "us" | "µs" => Duration::from_micros(amount),
            "ms" => Duration::from_millis(amount),
            "s" => Duration::from_secs(amount),
            "m" => Duration::from_secs(amount.checked_mul(60)?),
            "h" => Duration::from_secs(amount.checked_mul(3_600)?),
            "d" => Duration::from_secs(amount.checked_mul(86_400)?),
            "w" => Duration::from_secs(amount.checked_mul(604_800)?),
            "y" => Duration::from_secs(amount.checked_mul(31_536_000)?),
            _ => return None,
        };
        total = total.checked_add(part)?;
        rest = &rest[unit_len..];
    }
    Some(total)
}
