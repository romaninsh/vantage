//! Warm start: sims catch up on virtual time before the engine goes live.

use std::time::Instant;

use super::*;

/// Born every sim minute; steps every 10 minutes; gone after 50.
const LIFE: &str = r#"
    let id = insert(#{ who: "w", step: 0, at: now_secs() });
    for i in 1..=5 {
        sleep(minutes(10));
        patch(id, #{ step: i });
    }
    delete(id);
"#;

fn warm_def(clock: f64) -> SimDef {
    SimDef::new("w", "log", LIFE)
        .with_spawn(0, 1.0, 100)
        .with_clock(clock)
        .with_warm(Duration::from_secs(3600))
}

/// After the warm start and the first live spawn (at age 0).
fn assert_mid_life(engine: &SimEngine, log: &MemoryTable) {
    engine.settle();
    let rows = rows(log);
    // Born at -59..=0 min; the ones born 50+ minutes ago are gone.
    assert_eq!(rows.len(), 50);
    for row in &rows {
        let age_min = (T0 as f64 - num(row, "at")) / 60.0;
        assert!((0.0..50.0).contains(&age_min), "age {age_min}");
        assert_eq!(num(row, "step"), (age_min / 10.0).floor(), "age {age_min}");
    }
    assert_eq!(engine.live(), 50);
}

#[test]
fn warm_start_opens_mid_life_instantly_and_quietly() {
    let store = store_with(&["log"]);
    let log = store.table("log");
    let mut rx = log.subscribe();
    let t = Instant::now();
    let engine = SimEngine::builder()
        .store(&store)
        .sim(warm_def(1.0))
        .manual_clock(start())
        .start()
        .unwrap();
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "warm took {:?}",
        t.elapsed()
    );
    assert_mid_life(&engine, &log);
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(
        matches!(
            events.as_slice(),
            [MemoryChange::Reset, MemoryChange::Inserted { .. }]
        ),
        "one Reset for the warm start, then the live spawn at age 0: {events:?}"
    );

    // Born at 1..=10 min; gone: the ones born at -49..=-40 min.
    run_for(&engine, 600, 10);
    assert!(rx.try_recv().is_ok());
    assert_eq!(rows(&log).len(), 50);
}

#[test]
fn warm_window_is_sim_time_on_a_fast_clock() {
    let store = store_with(&["log"]);
    let log = store.table("log");
    let engine = SimEngine::builder()
        .store(&store)
        .sim(warm_def(60.0))
        .manual_clock(start())
        .start()
        .unwrap();
    assert_mid_life(&engine, &log);
    // Ten sim minutes are ten real seconds at 60x.
    run_for(&engine, 10, 1);
    assert_eq!(rows(&log).len(), 50);
}

#[test]
fn warm_progress_reports_each_window_and_skips_no_warm() {
    let started = |def: SimDef| {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let _engine = SimEngine::builder()
            .store(&store_with(&["log"]))
            .sim(def)
            .manual_clock(start())
            .on_warm_progress(move |p| sink.lock().unwrap().push(p))
            .start()
            .unwrap();
        seen.lock().unwrap().clone()
    };

    let seen = started(warm_def(1.0));
    assert_eq!(seen.len(), 256);
    assert!(seen[0] > 0.0);
    assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
    assert_eq!(seen.last(), Some(&1.0));

    let cold = SimDef::new("w", "log", LIFE).with_spawn(0, 1.0, 100);
    assert!(started(cold).is_empty());
}

#[test]
fn warm_start_on_the_system_clock_goes_live() {
    let store = store_with(&["log"]);
    let log = store.table("log");
    let def = SimDef::new(
        "w",
        "log",
        r#"insert(#{ who: "w", at: now_secs() }); sleep(days(1));"#,
    )
    .with_spawn(1, 60.0, 1000)
    .with_warm(Duration::from_secs(600));
    let engine = SimEngine::builder().store(&store).sim(def).start().unwrap();
    // A burst at -10 min plus one a second after it, then live ones.
    let n = rows(&log).len();
    assert!((600..610).contains(&n), "{n} rows");
    assert!(engine.live() >= 600);
}
