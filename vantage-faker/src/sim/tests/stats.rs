//! `SimEngine::stats`: spawn, end, error and write counters.

use super::*;

#[test]
fn stats_count_spawned_ended_and_writes() {
    let script = r#"
        let id = insert(#{ who: "a" });
        patch(id, #{ step: "1" });
        set(id, "step", "2");
        sleep(seconds(10));
        delete(id);
    "#;
    let (engine, _log) =
        engine_with(vec![SimDef::new("a", "log", script).with_spawn(3, 0.0, 3)]);
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
fn stale_writes_are_not_counted() {
    let script = r#"
        patch("nope", #{ step: "1" });
        set("nope", "step", "2");
        delete("nope");
    "#;
    let (engine, _log) =
        engine_with(vec![SimDef::new("a", "log", script).with_spawn(1, 0.0, 1)]);
    run_for(&engine, 2, 1);
    let s = engine.stats();
    assert_eq!(s.writes, 0);
    assert_eq!(s.errored, 0);
    assert_eq!(s.ended, 1);
}
