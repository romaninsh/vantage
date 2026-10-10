//! Sleeps that do not move the clock, and inserts over an existing id.

use std::time::Instant;

use super::*;

/// A warm start over `script` on the manual clock; returns how long it took.
fn warm(script: &str) -> (SimEngine, MemoryTableHandle, Duration) {
    let store = store_with(&["log"]);
    let def = SimDef::new("w", "log", script)
        .with_spawn(3, 0.0, 3)
        .with_warm(Duration::from_secs(7200));
    let t = Instant::now();
    let engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .start()
        .unwrap();
    (engine, store.table("log"), t.elapsed())
}

fn wait_for_no_sims(engine: &SimEngine) {
    let t = Instant::now();
    while engine.live() > 0 && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(engine.live(), 0, "the runaway sims ended");
}

#[test]
fn still_sleep_loops_end_the_sim_during_a_warm_start() {
    for script in [
        "loop { sleep(0); }",
        "loop { sleep(-5.0); }",
        "loop { wait_until(now_secs() - 10.0); }",
        "sleep(minutes(5)); loop { sleep(0); }",
    ] {
        let (engine, _log, took) = warm(script);
        assert!(
            took < Duration::from_secs(5),
            "{script}: warm took {took:?}"
        );
        assert_eq!(engine.live(), 0, "{script}");
    }
}

#[test]
fn still_sleep_loops_end_the_sim_when_live() {
    for script in ["loop { sleep(0); }", "loop { sleep(-1); }"] {
        let engine = SimEngine::builder()
            .store(&store_with(&["log"]))
            .sim(SimDef::new("spin", "log", script).with_spawn(2, 0.0, 2))
            .start()
            .unwrap();
        wait_for_no_sims(&engine);
    }
}

#[test]
fn an_occasional_still_sleep_is_harmless() {
    let script = r#"
        for i in 0..(current::MAX_STILL * 3) { sleep(0); sleep(seconds(1)); }
        table().insert(#{ who: "finished", at: now_secs() });
        sleep(hours(10));
    "#
    .replace("current::MAX_STILL", &current::MAX_STILL_SLEEPS.to_string());
    let (engine, log, _) = warm(&script);
    engine.settle();
    let rows = rows(&log);
    assert_eq!(rows.len(), 3, "every sim finished its loop");
    assert_eq!(engine.live(), 3);
}

#[test]
fn an_insert_without_an_id_gets_a_uuid_v7() {
    let store = store_with(&["log"]);
    let log = store.table("log");
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new(
            "u",
            "log",
            r#"table().insert(#{ name: "x" });"#,
        ))
        .manual_clock(start())
        .start()
        .unwrap();
    engine.settle();
    let ids = log.ids();
    assert_eq!(ids.len(), 1);
    let id = uuid::Uuid::parse_str(&ids[0]).expect("the id parses as a UUID");
    assert_eq!(id.get_version_num(), 7);
}

#[test]
fn inserting_an_existing_id_keeps_the_stored_row() {
    let store = store_with(&["log"]);
    let log = store.table("log");
    let mut rx = log.subscribe();
    let script = r#"
        table().insert(#{ id: "x", who: "first" });
        table().insert(#{ id: "x", who: "second" });
    "#;
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new("u", "log", script))
        .manual_clock(start())
        .start()
        .unwrap();
    engine.settle();
    assert!(matches!(rx.try_recv(), Ok(MemoryChange::Inserted { .. })));
    assert!(rx.try_recv().is_err());
    let rows = rows(&log);
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0], "who"), "first");
    assert_eq!(engine.stats().errored, 0);
}
