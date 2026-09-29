//! The store-backed verbs (`upsert`, `find`), warm-start `Reset`s, the
//! operations budget and write counting.

use super::*;

#[test]
fn upsert_is_idempotent_across_spawns() {
    let script = r#"upsert("fixed", #{ who: sim_id() }); upsert("fixed", #{ who: "same" });"#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script).with_spawn(3, 0.0, 3)]);
    run_for(&engine, 2, 1);
    assert_eq!(log.ids(), vec!["fixed"]);
    assert_eq!(text(&log.get("fixed").unwrap(), "who"), "same");
}

#[test]
fn find_matches_equalities_in_insertion_order() {
    let script = r#"
        insert(#{ id: "a", k: "x" }); insert(#{ id: "b", k: "y" }); insert(#{ id: "c", k: "x" });
        let hits = find(#{ k: "x" });
        insert(#{ id: "result", step: hits[0] + "," + hits[1] });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(text(&log.get("result").unwrap(), "step"), "a,c");
}

#[test]
fn find_edge_cases() {
    let script = r#"
        insert(#{ id: "a", k: "x" });
        insert(#{ id: "none", step: "" + find(#{ nope: 1 }).len() });
        insert(#{ id: "all", step: "" + find(#{}).len() });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(text(&log.get("none").unwrap(), "step"), "0");
    assert_eq!(text(&log.get("all").unwrap(), "step"), "2");
}

#[test]
fn find_uses_an_indexed_column() {
    let store = store_with(&[]);
    store.define(
        "log",
        vantage_memory::TableDef {
            indexed: vec!["k".into()],
            ..Default::default()
        },
    );
    let script = r#"
        insert(#{ id: "a", k: "x" }); insert(#{ id: "b", k: "y" }); insert(#{ id: "c", k: "x" });
        let hits = find(#{ k: "x" });
        insert(#{ id: "result", step: hits[0] + "," + hits[1] });
    "#;
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new("a", "log", script))
        .manual_clock(start())
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    let log = store.table("log");
    assert!(log.is_indexed("k"));
    assert_eq!(text(&log.get("result").unwrap(), "step"), "a,c");
}

#[test]
fn missing_default_table_fails_start() {
    let err = SimEngine::builder()
        .store(&store_with(&["log"]))
        .sim(SimDef::new("a", "nope", "sleep(seconds(1));"))
        .manual_clock(start())
        .start()
        .err()
        .expect("start fails")
        .to_string();
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn warm_start_ends_with_one_reset_per_written_table() {
    let store = store_with(&["log", "other"]);
    let mut log_rx = store.table("log").subscribe();
    let mut other_rx = store.table("other").subscribe();
    let def = SimDef::new("w", "log", "insert(#{ who: \"warm\" }); sleep(minutes(1));")
        .with_warm(Duration::from_secs(600))
        .with_spawn(2, 0.0, 2);
    let _engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .start()
        .unwrap();
    let log_events: Vec<_> = std::iter::from_fn(|| log_rx.try_recv().ok()).collect();
    assert!(
        matches!(log_events.as_slice(), [MemoryChange::Reset]),
        "{log_events:?}"
    );
    assert!(
        other_rx.try_recv().is_err(),
        "an untouched table gets no Reset"
    );
}

#[test]
fn warm_without_writes_sends_no_reset() {
    let store = store_with(&["log"]);
    let mut rx = store.table("log").subscribe();
    let def = SimDef::new("idle", "log", "sleep(minutes(5));").with_warm(Duration::from_secs(600));
    let _engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .start()
        .unwrap();
    assert!(rx.try_recv().is_err());
}

#[test]
fn ops_budget_ends_a_spinning_sim() {
    let def = SimDef::new("spin", "log", "let n = 0; loop { n += 1; }").with_ops(100_000);
    let (engine, _log) = engine_with(vec![def]);
    run_for(&engine, 1, 1);
    let s = engine.stats();
    assert_eq!((s.live, s.errored), (0, 1));
}

#[test]
fn ops_budget_is_per_def() {
    // Some hundreds of thousands of operations, then a write.
    let script = r#"let n = 0; while n < 100000 { n += 1; } insert(#{ id: "done" });"#;
    let run = |def: SimDef| {
        let (engine, log) = engine_with(vec![def]);
        run_for(&engine, 1, 1);
        (engine.stats().errored, log.get("done").is_some())
    };
    assert_eq!(run(SimDef::new("roomy", "log", script)), (0, true));
    assert_eq!(
        run(SimDef::new("tight", "log", script).with_ops(100_000)),
        (1, false)
    );
}

#[test]
fn writes_count_only_changing_sim_writes() {
    let script = r#"
        insert(#{ id: "a" });           // 1
        upsert("a", #{ id: "a" });      // unchanged → 0
        upsert("b", #{ x: 1 });         // 1
        patch("a", #{ x: 2 });          // 1
        patch("missing", #{ x: 2 });    // 0
        delete("b");                    // 1
        delete("b");                    // 0
    "#;
    let (engine, _log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(engine.stats().writes, 4);
}
