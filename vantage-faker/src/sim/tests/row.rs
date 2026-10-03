//! `fake_row()` and `builtin:` script names.

use super::*;
use crate::{ColumnGen, FakerColumn};

fn pick(values: &[&str]) -> ColumnGen {
    serde_yaml_ng::from_str(&format!("pick: {{ values: [{}] }}", values.join(", "))).unwrap()
}

fn engine_with_columns(script: &str) -> (SimEngine, MemoryTableHandle) {
    let store = store_with(&["log"]);
    let engine = SimEngine::builder()
        .store(&store)
        .columns(
            "log",
            vec![
                FakerColumn::new("id", "string"),
                FakerColumn::new("status", "string").with_generator(pick(&["Open"])),
                FakerColumn::new("n", "int"),
            ],
        )
        .sim(SimDef::new("r", "log", script))
        .manual_clock(start())
        .seed(4)
        .start()
        .unwrap();
    (engine, store.table("log"))
}

#[test]
fn row_generates_declared_columns_without_id() {
    let (engine, log) = engine_with_columns(
        "let r = table().fake_row(); table().insert(#{ id: \"x\", has_id: r.contains(\"id\"), status: r.status, n: r.n });",
    );
    run_for(&engine, 1, 1);
    let rec = log.get("x").unwrap();
    assert_eq!(text(&rec, "status"), "Open");
    assert_eq!(rec.get("has_id"), Some(&CborValue::Bool(false)));
    assert!(matches!(rec.get("n"), Some(CborValue::Integer(_))));
}

#[test]
fn row_walk_column_varies_across_calls_in_one_sim() {
    let store = store_with(&["log"]);
    let engine = SimEngine::builder()
        .store(&store)
        .columns(
            "log",
            vec![
                FakerColumn::new("id", "string"),
                FakerColumn::new("score", "int").with_generator(ColumnGen::Walk {
                    start: 0.0,
                    step: 1000.0,
                    min: None,
                    max: None,
                    decimals: None,
                }),
            ],
        )
        .sim(SimDef::new(
            "r",
            "log",
            "let a = table().fake_row().score; let b = table().fake_row().score; let c = table().fake_row().score; \
             table().insert(#{ id: \"x\", a: a, b: b, c: c });",
        ))
        .manual_clock(start())
        .seed(4)
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    let rec = store.table("log").get("x").unwrap();
    let (a, b, c) = (num(&rec, "a"), num(&rec, "b"), num(&rec, "c"));
    assert!(
        a != b || b != c,
        "walk did not vary across calls: {a} {b} {c}"
    );
}

/// A one-sim-per-row def, like `builtin:fifo`, calling `fake_row()` on a `walk`
/// column thousands of times over its lifetime. The walk's memo is shared
/// per table (`Kind::row_state`), so growing it costs O(1) per call rather
/// than replaying the series from scratch each time: this checks the walk
/// stays continuous and that 5k sims still run quickly.
#[test]
fn row_walk_column_stays_continuous_and_cheap_across_many_sims() {
    let store = store_with(&["log"]);
    let step = 5.0;
    let engine = SimEngine::builder()
        .store(&store)
        .columns(
            "log",
            vec![
                FakerColumn::new("id", "string"),
                FakerColumn::new("score", "int").with_generator(ColumnGen::Walk {
                    start: 0.0,
                    step,
                    min: Some(0.0),
                    max: Some(1000.0),
                    decimals: None,
                }),
            ],
        )
        .sim(
            SimDef::new(
                "r",
                "log",
                "let r = table().fake_row(); table().insert(#{ id: sim_id().to_string(), score: r.score });",
            )
            .with_spawn(0, 60.0, 50),
        )
        .manual_clock(start())
        .seed(4)
        .start()
        .unwrap();

    let began = std::time::Instant::now();
    run_for(&engine, 5000, 1);
    let elapsed = began.elapsed();
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");

    let log = store.table("log");
    let mut ids = log.ids();
    assert!(ids.len() >= 4000, "only {} sims ran", ids.len());
    ids.sort_by_key(|id| id.parse::<u64>().unwrap());
    let scores: Vec<f64> = ids
        .iter()
        .map(|id| {
            let rec = log.get(id).unwrap();
            num(&rec, "score")
        })
        .collect();
    for w in scores.windows(2) {
        assert!(
            (w[0] - w[1]).abs() <= step + 2.0,
            "walk jumped {} -> {}, not a continuing series",
            w[0],
            w[1]
        );
    }
}

#[test]
fn row_on_a_table_without_columns_is_empty() {
    let store = store_with(&["log", "bare"]);
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new(
            "r",
            "log",
            "table().insert(#{ id: \"x\", n: table(\"bare\").fake_row().len() });",
        ))
        .manual_clock(start())
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    assert_eq!(
        store.table("log").get("x").unwrap().get("n"),
        Some(&CborValue::Integer(0.into()))
    );
}

#[test]
fn builtin_names_resolve_and_unknown_errors() {
    use crate::sim::builtin::{BUILTINS, builtin};
    for name in BUILTINS {
        assert!(builtin(name).is_some(), "{name}");
    }
    assert!(builtin("nope").is_none());
}
