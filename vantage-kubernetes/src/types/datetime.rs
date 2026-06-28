//! Timestamp helpers for Kubernetes `creationTimestamp` fields.
//!
//! K8s emits RFC3339 / ISO-8601 UTC timestamps (`2026-06-27T10:15:00Z`).
//! We keep the raw string for display but also expose a compact relative
//! "age" (`5d`, `3h`, `12m`, `45s`) the way `kubectl` does.

use chrono::{DateTime, Utc};

/// Parse an RFC3339 timestamp into a UTC `DateTime`.
pub fn parse(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// `kubectl`-style coarse age: the largest single unit. `now` is passed in
/// so the function stays pure and testable.
pub fn age_from(ts: &str, now: DateTime<Utc>) -> Option<String> {
    let then = parse(ts)?;
    let secs = (now - then).num_seconds().max(0);
    Some(human_age(secs))
}

fn human_age(secs: i64) -> String {
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    if secs >= DAY {
        format!("{}d", secs / DAY)
    } else if secs >= HOUR {
        format!("{}h", secs / HOUR)
    } else if secs >= MIN {
        format!("{}m", secs / MIN)
    } else {
        format!("{}s", secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3339() {
        assert!(parse("2026-06-27T10:15:00Z").is_some());
        assert!(parse("not-a-date").is_none());
    }

    #[test]
    fn formats_age() {
        let now = parse("2026-06-27T10:00:00Z").unwrap();
        assert_eq!(age_from("2026-06-22T10:00:00Z", now).unwrap(), "5d");
        assert_eq!(age_from("2026-06-27T07:00:00Z", now).unwrap(), "3h");
        assert_eq!(age_from("2026-06-27T09:45:00Z", now).unwrap(), "15m");
        assert_eq!(age_from("2026-06-27T09:59:30Z", now).unwrap(), "30s");
    }
}
