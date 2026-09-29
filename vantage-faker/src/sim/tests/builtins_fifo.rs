//! `builtin:fifo`: one sim per row, newest first, each row leaving after its
//! retention.

use super::*;
use crate::FakerColumn;

fn fifo_engine(rate: f64, args: serde_json::Value) -> (SimEngine, MemoryTableHandle) {
    let store = store_with(&["arrival"]);
    let def = SimDef::new(
        "in",
        "arrival",
        crate::sim::builtin::builtin("fifo").unwrap(),
    )
    .with_spawn(0, rate, 500)
    .with_args(args);
    let engine = SimEngine::builder()
        .store(&store)
        .columns("arrival", vec![FakerColumn::new("name", "string")])
        .sim(def)
        .manual_clock(start())
        .seed(9)
        .start()
        .unwrap();
    (engine, store.table("arrival"))
}

#[test]
fn fifo_population_settles_and_lists_newest_first() {
    let (engine, t) = fifo_engine(
        60.0,
        serde_json::json!({ "retention_lo": 20, "retention_hi": 20 }),
    );
    run_for(&engine, 120, 1);
    let n = t.len();
    assert!((15..=25).contains(&n), "population {n}");
    let ids = t.ids();
    let mut sorted = ids.clone();
    sorted.sort();
    let newest = sorted.first().unwrap();
    let oldest = sorted.last().unwrap();
    assert!(t.get(newest).is_some() && t.get(oldest).is_some());
    assert!(newest < oldest);
    assert!(
        t.get(newest).unwrap().get("name").is_some(),
        "rows come from row()"
    );
}

#[test]
fn fifo_rows_expire() {
    let (engine, t) = fifo_engine(
        60.0,
        serde_json::json!({ "retention_lo": 5, "retention_hi": 5 }),
    );
    run_for(&engine, 30, 1);
    let before: Vec<String> = t.ids();
    run_for(&engine, 10, 1);
    assert!(
        before.iter().all(|id| t.get(id).is_none()),
        "every earlier row expired"
    );
}

#[test]
fn fifo_bad_args_error_the_sim_only() {
    let (engine, t) = fifo_engine(60.0, serde_json::json!({ "retention_lo": "20s" }));
    run_for(&engine, 5, 1);
    assert!(engine.stats().errored >= 1);
    assert_eq!(t.len(), 0);
}
