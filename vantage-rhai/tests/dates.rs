use std::time::SystemTime;

use chrono::{DateTime, Timelike, Utc};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Dates, Host, Limits};

fn host() -> Host {
    Host::builder(Limits::Ui).vocab(Dates).build()
}

fn eval<T: Clone + Send + Sync + 'static>(src: &str) -> T {
    host()
        .engine()
        .eval::<T>(src)
        .unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn eval_err(src: &str) -> String {
    match host().engine().eval::<Dynamic>(src) {
        Ok(v) => panic!("{src} should throw, got {v:?}"),
        Err(e) => e.to_string(),
    }
}

fn day(src: &str) -> String {
    eval::<String>(&format!("({src}).to_date_string()"))
}

#[test]
fn date_builds_a_utc_datetime_from_text_or_parts() {
    let d: DateTime<Utc> = eval(r#"date("2026-12-10")"#);
    assert_eq!(d.to_rfc3339(), "2026-12-10T00:00:00+00:00");
    let d: DateTime<Utc> = eval("date(2026, 12, 10)");
    assert_eq!(d.to_rfc3339(), "2026-12-10T00:00:00+00:00");
    let d: DateTime<Utc> = eval(r#"date("2026-12-10T10:30:00+02:00")"#);
    assert_eq!(d.to_rfc3339(), "2026-12-10T08:30:00+00:00");
    let d: DateTime<Utc> = eval(r#"date("2026-12-10 08:30:00")"#);
    assert_eq!(d.to_rfc3339(), "2026-12-10T08:30:00+00:00");
    let d: DateTime<Utc> = eval(r#"date(date("2026-12-10"))"#);
    assert_eq!(d.to_rfc3339(), "2026-12-10T00:00:00+00:00");
}

#[test]
fn date_throws_on_text_or_parts_that_are_not_a_date() {
    assert!(eval_err(r#"date("10/12/2026")"#).contains("10/12/2026"));
    assert!(eval_err(r#"date("2026-02-30")"#).contains("2026-02-30"));
    assert!(eval_err("date(2026, 2, 30)").contains("2026-2-30"));
    assert!(eval_err("date(2026, 13, 1)").contains("2026-13-1"));
}

#[test]
fn now_is_an_instant_and_today_is_its_midnight() {
    let before = DateTime::<Utc>::from(SystemTime::now());
    let now: DateTime<Utc> = eval("now()");
    let today: DateTime<Utc> = eval("today()");
    assert!(now >= before && now <= DateTime::<Utc>::from(SystemTime::now()));
    assert_eq!(today.date_naive(), now.date_naive());
    assert_eq!((today.hour(), today.minute(), today.second()), (0, 0, 0));
    assert_eq!(today.nanosecond(), 0);
}

#[test]
fn add_months_clamps_to_the_last_day_of_a_shorter_month() {
    assert_eq!(day(r#"date("2026-01-31").add_months(1)"#), "2026-02-28");
    assert_eq!(day(r#"date("2028-01-31").add_months(1)"#), "2028-02-29");
    assert_eq!(day(r#"date("2026-03-31").add_months(-1)"#), "2026-02-28");
    assert_eq!(day(r#"date("2026-08-31").add_months(13)"#), "2027-09-30");
    assert_eq!(day(r#"date("2028-02-29").add_years(1)"#), "2029-02-28");
    assert_eq!(day(r#"date("2026-01-31").add_months(0)"#), "2026-01-31");
}

#[test]
fn add_days_hours_minutes_seconds_shift_by_signed_offsets() {
    assert_eq!(day(r#"date("2026-12-25").add_days(14)"#), "2027-01-08");
    assert_eq!(day(r#"date("2026-03-01").add_days(-1)"#), "2026-02-28");
    let d: DateTime<Utc> =
        eval(r#"date("2026-12-10").add_hours(2).add_minutes(-30).add_seconds(15)"#);
    assert_eq!(d.to_rfc3339(), "2026-12-10T01:30:15+00:00");
    assert!(eval_err(r#"date("2026-12-10").add_days(9223372036854775807)"#).contains("add_days"));
    assert!(
        eval_err(r#"date("2026-12-10").add_months(-9223372036854775807)"#).contains("add_months")
    );
}

#[test]
fn getters_read_the_utc_calendar_and_clock() {
    let src = r#"let d = date("2026-12-10T08:30:15Z"); [d.year, d.month, d.day, d.weekday, d.hour, d.minute, d.second]"#;
    let parts: Vec<i64> = eval::<vantage_rhai::rhai::Array>(src)
        .into_iter()
        .map(|v| v.as_int().unwrap())
        .collect();
    assert_eq!(parts, [2026, 12, 10, 4, 8, 30, 15]);
    assert_eq!(eval::<i64>(r#"date("2026-12-14").weekday"#), 1);
}

#[test]
fn start_of_day_and_start_of_month_truncate_in_utc() {
    let d: DateTime<Utc> = eval(r#"date("2026-12-10T23:59:59Z").start_of_day()"#);
    assert_eq!(d.to_rfc3339(), "2026-12-10T00:00:00+00:00");
    let d: DateTime<Utc> = eval(r#"date("2026-12-10T23:59:59Z").start_of_month()"#);
    assert_eq!(d.to_rfc3339(), "2026-12-01T00:00:00+00:00");
}

#[test]
fn days_between_counts_whole_days_from_a_to_b() {
    assert_eq!(
        eval::<i64>(r#"days_between(date("2026-12-10"), date("2026-12-24"))"#),
        14
    );
    assert_eq!(
        eval::<i64>(r#"days_between(date("2026-12-24"), date("2026-12-10"))"#),
        -14
    );
    assert_eq!(
        eval::<i64>(r#"days_between("2026-02-01", date("2026-03-01"))"#),
        28
    );
    assert_eq!(
        eval::<i64>(r#"days_between(date("2026-02-01"), "2026-03-01T00:00:00Z")"#),
        28
    );
    assert_eq!(
        eval::<i64>(r#"days_between("2026-12-10", "2026-12-11T23:00:00Z")"#),
        1
    );
    assert!(eval_err(r#"days_between("soon", date("2026-12-10"))"#).contains("soon"));
}

#[test]
fn comparisons_work_between_datetimes_and_against_date_text() {
    let checks = [
        r#"date("2026-12-10") < date("2026-12-11")"#,
        r#"date("2026-12-10") <= date("2026-12-10")"#,
        r#"date("2026-12-11") > date("2026-12-10")"#,
        r#"date("2026-12-10") >= date("2026-12-10")"#,
        r#"date("2026-12-10") == date(2026, 12, 10)"#,
        r#"date("2026-12-10") != date("2026-12-11")"#,
        r#"date("2026-12-10") == "2026-12-10T00:00:00Z""#,
        r#""2026-12-10" == date("2026-12-10")"#,
        r#"date("2026-12-10") < "2026-12-11""#,
        r#""2026-12-09T23:00:00Z" < date("2026-12-10")"#,
        r#"date("2026-12-10") != "not a date""#,
        r#"!(date("2026-12-10") < "not a date")"#,
        r#"!(date("2026-12-10") == "not a date")"#,
        r#"today() < today().add_months(1)"#,
    ];
    for src in checks {
        assert!(eval::<bool>(src), "{src}");
    }
}

#[test]
fn to_date_string_and_format_render_text() {
    assert_eq!(
        eval::<String>(r#"date("2026-12-10T08:30:00Z").to_date_string()"#),
        "2026-12-10"
    );
    assert_eq!(
        eval::<String>(r#"date("2026-12-10T08:30:00Z").format("%d.%m.%Y %H:%M")"#),
        "10.12.2026 08:30"
    );
    assert!(eval_err(r#"date("2026-12-10").format("%Q")"#).contains("%Q"));
}

#[test]
fn a_datetime_interpolates_and_prints_as_rfc3339() {
    assert_eq!(
        eval::<String>(r#"`due ${date("2026-12-10")}`"#),
        "due 2026-12-10T00:00:00+00:00"
    );
    assert_eq!(
        eval::<String>(r#"date("2026-12-10").to_string()"#),
        "2026-12-10T00:00:00+00:00"
    );
    assert_eq!(eval::<String>(r#"type_of(date("2026-12-10"))"#), "DateTime");
}
