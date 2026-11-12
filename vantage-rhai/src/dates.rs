//! UTC dates and datetimes for scripts: one `DateTime` type, constructors,
//! calendar arithmetic, getters and comparisons.
//!
//! A date is a `DateTime` at midnight UTC, so one value type serves both a
//! `type: date` column and a datetime field. Reads arrive as text; `date(x)`
//! turns that text back into a value and comparisons accept it directly.

use std::fmt::Write as _;
use std::time::SystemTime;

use chrono::{
    DateTime, Datelike, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike, Utc,
};
use rhai::{Dynamic, Engine, EvalAltResult, ImmutableString};

use crate::host::Vocab;

type Instant = DateTime<Utc>;
type Fallible<T> = Result<T, Box<EvalAltResult>>;

/// The date vocabulary; register it on any host with `.vocab(Dates)`.
pub struct Dates;

impl Vocab for Dates {
    fn register(&self, engine: &mut Engine) {
        engine.register_type_with_name::<Instant>("DateTime");
        engine.register_fn("to_string", |dt: &mut Instant| dt.to_rfc3339());
        engine.register_fn("to_debug", |dt: &mut Instant| dt.to_rfc3339());

        engine.register_fn("now", now);
        engine.register_fn("today", || start_of_day(&now()));
        engine.register_fn("date", |value: Dynamic| instant("date", value));
        engine.register_fn("date", from_parts);
        engine.register_fn("days_between", |a: Dynamic, b: Dynamic| {
            Ok::<_, Box<EvalAltResult>>(
                (instant("days_between", b)? - instant("days_between", a)?).num_days(),
            )
        });

        engine.register_fn("add_days", |dt: &mut Instant, n: i64| {
            shift(dt, "add_days", n, TimeDelta::try_days(n))
        });
        engine.register_fn("add_hours", |dt: &mut Instant, n: i64| {
            shift(dt, "add_hours", n, TimeDelta::try_hours(n))
        });
        engine.register_fn("add_minutes", |dt: &mut Instant, n: i64| {
            shift(dt, "add_minutes", n, TimeDelta::try_minutes(n))
        });
        engine.register_fn("add_seconds", |dt: &mut Instant, n: i64| {
            shift(dt, "add_seconds", n, TimeDelta::try_seconds(n))
        });
        engine.register_fn("add_months", |dt: &mut Instant, n: i64| {
            add_months(dt, "add_months", n, Some(n))
        });
        engine.register_fn("add_years", |dt: &mut Instant, n: i64| {
            add_months(dt, "add_years", n, n.checked_mul(12))
        });
        engine.register_fn("start_of_day", |dt: &mut Instant| start_of_day(dt));
        engine.register_fn("start_of_month", |dt: &mut Instant| {
            start_of_day(dt)
                .with_day(1)
                .expect("day 1 exists in every month")
        });

        engine.register_get("year", |dt: &mut Instant| i64::from(dt.year()));
        engine.register_get("month", |dt: &mut Instant| i64::from(dt.month()));
        engine.register_get("day", |dt: &mut Instant| i64::from(dt.day()));
        engine.register_get("weekday", |dt: &mut Instant| {
            i64::from(dt.weekday().number_from_monday())
        });
        engine.register_get("hour", |dt: &mut Instant| i64::from(dt.hour()));
        engine.register_get("minute", |dt: &mut Instant| i64::from(dt.minute()));
        engine.register_get("second", |dt: &mut Instant| i64::from(dt.second()));

        engine.register_fn("to_date_string", |dt: &mut Instant| {
            dt.format("%Y-%m-%d").to_string()
        });
        engine.register_fn("format", |dt: &mut Instant, fmt: ImmutableString| {
            let mut out = String::new();
            write!(out, "{}", dt.format(&fmt))
                .map_err(|_| format!("format: '{fmt}' is not a valid format"))?;
            Ok::<_, Box<EvalAltResult>>(out)
        });

        register_comparisons(engine);
    }
}

fn now() -> Instant {
    Instant::from(SystemTime::now())
}

fn start_of_day(dt: &Instant) -> Instant {
    dt.date_naive().and_time(NaiveTime::MIN).and_utc()
}

/// RFC 3339 with any offset, `YYYY-MM-DD` (midnight), or a datetime with no
/// offset (SQL text), all read as UTC.
fn parse(text: &str) -> Option<Instant> {
    let text = text.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
        return Some(dt.with_timezone(&Utc));
    }
    if text.len() == 10 {
        return NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .map(|d| d.and_time(NaiveTime::MIN).and_utc());
    }
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
        .iter()
        .find_map(|f| NaiveDateTime::parse_from_str(text, f).ok())
        .map(|dt| dt.and_utc())
}

fn instant(func: &str, value: Dynamic) -> Fallible<Instant> {
    if let Some(dt) = value.clone().try_cast::<Instant>() {
        return Ok(dt);
    }
    match value.clone().try_cast::<ImmutableString>() {
        Some(text) => parse(&text).ok_or_else(|| {
            format!("{func}: '{text}' is not a date (expected YYYY-MM-DD or RFC 3339)").into()
        }),
        None => Err(format!(
            "{func}: expected a DateTime or date text, got {}",
            value.type_name()
        )
        .into()),
    }
}

fn from_parts(y: i64, m: i64, d: i64) -> Fallible<Instant> {
    i32::try_from(y)
        .ok()
        .zip(u32::try_from(m).ok().zip(u32::try_from(d).ok()))
        .and_then(|(y, (m, d))| NaiveDate::from_ymd_opt(y, m, d))
        .map(|date| date.and_time(NaiveTime::MIN).and_utc())
        .ok_or_else(|| format!("date: {y}-{m}-{d} is not a calendar date").into())
}

fn out_of_range(func: &str, n: i64) -> Box<EvalAltResult> {
    format!("{func}({n}) is out of range").into()
}

fn shift(dt: &Instant, func: &str, n: i64, delta: Option<TimeDelta>) -> Fallible<Instant> {
    delta
        .and_then(|delta| dt.checked_add_signed(delta))
        .ok_or_else(|| out_of_range(func, n))
}

/// Calendar months; a day past the target month's end clamps to its last
/// day (31 Jan + 1 month = 28/29 Feb).
fn add_months(dt: &Instant, func: &str, n: i64, months: Option<i64>) -> Fallible<Instant> {
    months
        .and_then(|months| {
            let count = Months::new(u32::try_from(months.unsigned_abs()).ok()?);
            if months >= 0 {
                dt.checked_add_months(count)
            } else {
                dt.checked_sub_months(count)
            }
        })
        .ok_or_else(|| out_of_range(func, n))
}

/// `==` `!=` `<` `<=` `>` `>=` between two DateTimes and between a DateTime and
/// text `date()` accepts (a stored value reads back as text). Text that is not
/// a date compares unequal and never orders.
fn register_comparisons(engine: &mut Engine) {
    macro_rules! compare {
        ($op:literal, $cmp:expr, $on_bad_text:expr) => {
            engine.register_fn($op, |a: Instant, b: Instant| $cmp(&a, &b));
            engine.register_fn($op, |a: ImmutableString, b: Instant| {
                parse(&a).map_or($on_bad_text, |a| $cmp(&a, &b))
            });
            engine.register_fn($op, |a: Instant, b: ImmutableString| {
                parse(&b).map_or($on_bad_text, |b| $cmp(&a, &b))
            });
        };
    }
    compare!("==", |a: &Instant, b: &Instant| a == b, false);
    compare!("!=", |a: &Instant, b: &Instant| a != b, true);
    compare!("<", |a: &Instant, b: &Instant| a < b, false);
    compare!("<=", |a: &Instant, b: &Instant| a <= b, false);
    compare!(">", |a: &Instant, b: &Instant| a > b, false);
    compare!(">=", |a: &Instant, b: &Instant| a >= b, false);
}
