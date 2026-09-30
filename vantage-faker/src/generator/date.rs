//! `Date`: relative/ISO bounds, random or even spread, RFC 3339 UTC output.
//! Calendar maths is Howard Hinnant's days/civil conversion, so no date crate
//! is needed.

use std::time::{SystemTime, UNIX_EPOCH};

use ciborium::Value as CborValue;
use fake::rand::RngExt as _;

use super::hash::{self, STREAM_DATE};
use super::{Cell, Spread};

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn generate(cell: &mut Cell<'_>, from: &str, to: &str, spread: Spread) -> CborValue {
    let (Some(a), Some(b)) = (parse_when(from, cell.now), parse_when(to, cell.now)) else {
        return CborValue::Null;
    };
    let (lo, hi) = (a.min(b), a.max(b));
    let secs = match spread {
        Spread::Random if lo < hi => cell.rng.random_range(lo..=hi),
        Spread::Random => lo,
        Spread::Even => {
            // `seq` wraps at `rows`: a `row()` caller has no fixed row
            // count, so past the first `rows` calls the spread repeats
            // instead of pinning at `to` forever.
            let rows = cell.rows.max(1);
            let seq = cell.seq % rows;
            let step = (b - a) as f64 / (rows.saturating_sub(1).max(1)) as f64;
            let jitter = hash::unit(cell.salt, seq as u64, STREAM_DATE) * 0.5 * step;
            let t = a as f64 + seq as f64 * step + jitter;
            (t.floor() as i64).clamp(lo, hi)
        }
    };
    CborValue::Text(rfc3339(secs))
}

/// `now`, `±N{s,m,h,d,w}` from `now`, `YYYY-MM-DD`, or
/// `YYYY-MM-DDTHH:MM[:SS][Z]` — to unix seconds.
pub(crate) fn parse_when(s: &str, now: i64) -> Option<i64> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("now") {
        return Some(now);
    }
    if let Some(sign) = s.chars().next().filter(|c| *c == '+' || *c == '-') {
        let body = &s[1..];
        let unit = body.chars().last()?;
        let n: i64 = body[..body.len() - unit.len_utf8()].trim().parse().ok()?;
        let per = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3_600,
            'd' => 86_400,
            'w' => 604_800,
            _ => return None,
        };
        let offset = n.checked_mul(per)?;
        return Some(if sign == '-' {
            now - offset
        } else {
            now + offset
        });
    }
    parse_iso(s)
}

fn parse_iso(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z').unwrap_or(s);
    let (date, time) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut ymd = date.split('-');
    let y: i64 = ymd.next()?.parse().ok()?;
    let m: u32 = ymd.next()?.parse().ok()?;
    let d: u32 = ymd.next()?.parse().ok()?;
    if ymd.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, m, d) * 86_400;
    if let Some(t) = time {
        let mut hms = t.split(':');
        let h: i64 = hms.next()?.parse().ok()?;
        let mi: i64 = hms.next()?.parse().ok()?;
        let se: i64 = hms.next().map_or(Some(0), |x| x.parse().ok())?;
        if hms.next().is_some() || h > 23 || mi > 59 || se > 60 {
            return None;
        }
        secs += h * 3_600 + mi * 60 + se;
    }
    Some(secs)
}

pub(crate) fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem / 60) % 60,
        rem % 60
    )
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_and_iso_bounds() {
        let now = 1_790_553_600; // 2026-09-28T00:00:00Z
        assert_eq!(rfc3339(now), "2026-09-28T00:00:00Z");
        assert_eq!(parse_when("now", now), Some(now));
        assert_eq!(parse_when("-12h", now), Some(now - 43_200));
        assert_eq!(parse_when("+30d", now), Some(now + 30 * 86_400));
        assert_eq!(parse_when("2026-09-28", 0), Some(now));
        assert_eq!(parse_when("2026-09-28T01:02:03Z", 0), Some(now + 3_723));
        assert_eq!(
            parse_when("2024-02-29 23:59", 0).map(rfc3339).as_deref(),
            Some("2024-02-29T23:59:00Z")
        );
        assert_eq!(parse_when("-5y", now), None);
        assert_eq!(parse_when("2026-13-01", now), None);
    }
}
