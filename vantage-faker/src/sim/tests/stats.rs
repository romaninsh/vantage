//! `SimEngine::stats`: spawn, end, error and write counters.

use std::sync::{Arc, Mutex};

use super::*;

#[test]
fn stats_count_spawned_ended_and_writes() {
    let script = r#"
        let id = table().insert(#{ who: "a" });
        table().patch(id, #{ step: "1" });
        table().patch(id, #{ step: "2" });
        sleep(seconds(10));
        table().delete(id);
    "#;
    let (engine, _log) = engine_with(vec![SimDef::new("a", "log", script).with_spawn(3, 0.0, 3)]);
    run_for(&engine, 20, 5);
    let s = engine.stats();
    assert_eq!(s.live, 0);
    assert_eq!(s.spawned, 3);
    assert_eq!(s.ended, 3);
    assert_eq!(s.errored, 0);
    assert_eq!(s.writes, 12);
}

#[test]
fn stats_count_errors_apart_from_ends() {
    let (engine, _log) = engine_with(vec![
        SimDef::new("bad", "log", r#"sleep(seconds(1)); throw "boom";"#).with_spawn(2, 0.0, 2),
        SimDef::new("good", "log", "sleep(seconds(1));").with_spawn(1, 0.0, 1),
    ]);
    run_for(&engine, 5, 1);
    let s = engine.stats();
    assert_eq!((s.spawned, s.ended, s.errored), (3, 1, 2));
}

#[test]
fn sim_running_at_stop_counts_as_ended() {
    let (engine, _log) = engine_with(vec![
        SimDef::new("a", "log", "sleep(seconds(100));").with_spawn(2, 0.0, 2),
    ]);
    run_for(&engine, 5, 1);
    assert_eq!(engine.stats().live, 2);
    engine.stop();
    let s = engine.stats();
    assert_eq!((s.live, s.spawned, s.ended, s.errored), (0, 2, 2, 0));
}

#[test]
fn on_sim_error_reports_every_failure_uncapped() {
    let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_in_cb = seen.clone();
    let store = store_with(&["log"]);
    let engine = SimEngine::builder()
        .store(&store)
        .manual_clock(start())
        .seed(1)
        .on_sim_error(move |def, error| {
            seen_in_cb
                .lock()
                .unwrap()
                .push((def.to_string(), error.to_string()));
        })
        .sim(SimDef::new("bad", "log", r#"throw "boom";"#).with_spawn(3, 0.0, 3))
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    let calls = seen.lock().unwrap();
    assert_eq!(
        calls.len(),
        3,
        "every failing sim reported, not rate-limited"
    );
    for (def, error) in calls.iter() {
        assert_eq!(def, "bad");
        assert!(error.contains("boom"), "{error}");
    }
}

/// The total is the store's write counters since `start`: a seed written
/// before it is left out; a narrowed handle's write, and a write another
/// caller makes through the store's Rust API, count.
#[test]
fn writes_count_the_store_from_engine_start() {
    let store = store_with(&["log"]);
    let row = |k: &str, v: &str| -> Record<CborValue> {
        [(k.to_string(), CborValue::Text(v.into()))]
            .into_iter()
            .collect()
    };
    store
        .table("log")
        .insert_as("seeded", row("who", "seed"))
        .unwrap();
    let script = r#"
        table().insert(#{ id: "sim", who: "a" });            // 1
        table().where("who", "a").patch("sim", #{ n: 1 });  // 1
    "#;
    let engine = SimEngine::builder()
        .store(&store)
        .manual_clock(start())
        .seed(1)
        .sim(SimDef::new("a", "log", script))
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    assert_eq!(engine.stats().writes, 2, "the seed is not counted");

    assert!(store.table("log").patch("seeded", &row("who", "host")));
    store
        .table("other")
        .insert_as("x", row("who", "host"))
        .unwrap();
    assert_eq!(
        engine.stats().writes,
        4,
        "Rust-API writes share the counter"
    );
}

#[test]
fn stale_writes_are_not_counted() {
    let script = r#"
        table().patch("nope", #{ step: "1" });
        table().patch("nope", #{ step: "2" });
        table().delete("nope");
    "#;
    let (engine, _log) = engine_with(vec![SimDef::new("a", "log", script).with_spawn(1, 0.0, 1)]);
    run_for(&engine, 2, 1);
    let s = engine.stats();
    assert_eq!(s.writes, 0);
    assert_eq!(s.errored, 0);
    assert_eq!(s.ended, 1);
}
