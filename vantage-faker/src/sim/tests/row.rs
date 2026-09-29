//! `row()` / `row(t)` and `builtin:` script names.

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
        "let r = row(); insert(#{ id: \"x\", has_id: r.contains(\"id\"), status: r.status, n: r.n });",
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
            "let a = row().score; let b = row().score; let c = row().score; \
             insert(#{ id: \"x\", a: a, b: b, c: c });",
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

#[test]
fn row_on_a_table_without_columns_is_empty() {
    let store = store_with(&["log", "bare"]);
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new(
            "r",
            "log",
            "insert(#{ id: \"x\", n: row(\"bare\").len() });",
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
