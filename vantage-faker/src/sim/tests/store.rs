//! The store-backed verbs (`upsert`, `where`), warm-start `Reset`s, the
//! operations budget and write counting.

use super::*;
use crate::FakerColumn;

#[test]
fn upsert_is_idempotent_across_spawns() {
    let script = r#"table().upsert("fixed", #{ who: sim_id() }); table().upsert("fixed", #{ who: "same" });"#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script).with_spawn(3, 0.0, 3)]);
    run_for(&engine, 2, 1);
    assert_eq!(log.ids(), vec!["fixed"]);
    assert_eq!(text(&log.get("fixed").unwrap(), "who"), "same");
}

#[test]
fn find_matches_equalities_in_insertion_order() {
    let script = r#"
        table().insert(#{ id: "a", k: "x" }); table().insert(#{ id: "b", k: "y" }); table().insert(#{ id: "c", k: "x" });
        let hits = table().where("k", "x").ids();
        table().insert(#{ id: "result", step: hits[0] + "," + hits[1] });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(text(&log.get("result").unwrap(), "step"), "a,c");
}

#[test]
fn find_edge_cases() {
    let script = r#"
        table().insert(#{ id: "a", k: "x" });
        table().insert(#{ id: "none", step: "" + table().where("nope", 1).ids().len() });
        table().insert(#{ id: "all", step: "" + table().ids().len() });
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
        table().insert(#{ id: "a", k: "x" }); table().insert(#{ id: "b", k: "y" }); table().insert(#{ id: "c", k: "x" });
        let hits = table().where("k", "x").ids();
        table().insert(#{ id: "result", step: hits[0] + "," + hits[1] });
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
    let def = SimDef::new(
        "w",
        "log",
        "table().insert(#{ who: \"warm\" }); sleep(minutes(1));",
    )
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
    let script = r#"let n = 0; while n < 100000 { n += 1; } table().insert(#{ id: "done" });"#;
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
fn import_counts_each_inserted_row() {
    let store = store_with(&["log", "copy"]);
    let script = r#"
        table().insert(#{ id: "a" });           // 1
        table().insert(#{ id: "b" });           // 1
        table("copy").import_from(table());     // 2
    "#;
    let engine = SimEngine::builder()
        .store(&store)
        .manual_clock(start())
        .seed(1)
        .sim(SimDef::new("a", "log", script))
        .start()
        .expect("engine starts");
    run_for(&engine, 1, 1);
    assert_eq!(store.table("copy").len(), 2);
    assert_eq!(engine.stats().writes, 4);
}

#[test]
fn writes_count_only_changing_sim_writes() {
    let script = r#"
        table().insert(#{ id: "a" });           // 1
        table().upsert("a", #{ id: "a" });      // unchanged → 0
        table().upsert("b", #{ x: 1 });         // 1
        table().patch("a", #{ x: 2 });          // 1
        table().patch("missing", #{ x: 2 });    // 0
        table().delete("b");                    // 1
        table().delete("b");                    // 0
    "#;
    let (engine, _log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(engine.stats().writes, 4);
}

#[test]
fn sims_can_sort_limit_and_list() {
    let script = r#"
        table().insert(#{ id: "a", n: 1 });
        table().insert(#{ id: "b", n: 3 });
        table().insert(#{ id: "c", n: 2 });
        let top = table().sort("n", "desc").limit(2).list();
        table().insert(#{ id: "result", step: top[0].id + "," + top[1].id });
    "#;
    let store = store_with(&["log"]);
    let engine = SimEngine::builder()
        .store(&store)
        .columns("log", vec![FakerColumn::new("n", "int")])
        .manual_clock(start())
        .sim(SimDef::new("a", "log", script))
        .start()
        .unwrap();
    let log = store.table("log");
    run_for(&engine, 1, 1);
    assert_eq!(text(&log.get("result").unwrap(), "step"), "b,c");
}
