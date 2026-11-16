use chrono::{DateTime, TimeDelta, Utc};
use vantage_rhai::{Clock, Dates, Env, Expr, Host, Limits};

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn host(clock: &Clock) -> Host {
    let clock = clock.clone();
    Host::builder(Limits::Ui)
        .vocab_fn(|engine| Dates::register_with(engine, clock))
        .build()
}

fn eval(host: &Host, src: &str) -> DateTime<Utc> {
    host.engine().eval(src).unwrap()
}

#[test]
fn a_rebound_expression_reads_the_new_clock() {
    let compiled = host(&Clock::system())
        .compile(&Expr::from("now() < date(\"2026-12-01\")"))
        .unwrap();
    let clock = Clock::manual(at("2026-11-14T09:30:00Z"));
    let rebound = compiled.rebind(&host(&clock));
    assert!(rebound.eval_bool(&Env::new()).unwrap());
    clock.advance(TimeDelta::days(30));
    assert!(!rebound.eval_bool(&Env::new()).unwrap());
}

#[test]
fn a_manual_clock_drives_now_and_today() {
    let clock = Clock::manual(at("2026-11-14T09:30:00Z"));
    let host = host(&clock);
    assert_eq!(eval(&host, "now()"), at("2026-11-14T09:30:00Z"));
    assert_eq!(eval(&host, "today()"), at("2026-11-14T00:00:00Z"));

    clock.advance(TimeDelta::days(2) + TimeDelta::hours(15));
    assert_eq!(eval(&host, "now()"), at("2026-11-17T00:30:00Z"));
    assert_eq!(eval(&host, "today()"), at("2026-11-17T00:00:00Z"));

    clock.advance_months(3);
    assert_eq!(eval(&host, "now()"), at("2027-02-17T00:30:00Z"));
}

#[test]
fn a_forked_clock_advances_alone() {
    let clock = Clock::manual(at("2026-01-31T12:00:00Z"));
    let fork = clock.fork();
    fork.advance_months(1);
    assert_eq!(fork.now(), at("2026-02-28T12:00:00Z"));
    assert_eq!(clock.now(), at("2026-01-31T12:00:00Z"));

    clock.advance(TimeDelta::hours(1));
    assert_eq!(fork.now(), at("2026-02-28T12:00:00Z"));
    assert_eq!(eval(&host(&clock), "now()"), at("2026-01-31T13:00:00Z"));

    let system = Clock::system();
    assert!(system.fork().is_system());
    let before = DateTime::<Utc>::from(std::time::SystemTime::now());
    assert!(eval(&host(&system), "now()") >= before);
}
