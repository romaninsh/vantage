//! Plans `DatasetGen::generate` rejects, all before touching the store.

use super::*;

fn fails(dataset: DatasetGen, store: &MemoryStore) -> String {
    dataset
        .generate(store)
        .err()
        .expect("generate fails")
        .to_string()
}

fn linked(from: &str, column: &str, to: &str) -> TableGen {
    TableGen::new(from)
        .column(FakerColumn::new(column, "string"))
        .reference(column, to)
}

#[test]
fn cycle_is_an_error() {
    let dataset = DatasetGen::new(None)
        .table(linked("a", "b_id", "b"))
        .table(linked("b", "a_id", "a"));
    let err = fails(dataset, &MemoryStore::new());
    assert!(err.contains("cycle"), "{err}");
}

#[test]
fn unknown_reference_is_an_error() {
    let err = fails(
        DatasetGen::new(None).table(linked("a", "x_id", "x")),
        &MemoryStore::new(),
    );
    assert!(err.contains("a") && err.contains("x"), "{err}");
}

#[test]
fn existing_table_with_other_id_column_is_an_error() {
    let store = MemoryStore::new();
    store.define(
        "t",
        vantage_memory::TableDef {
            id_column: "code".into(),
            ..Default::default()
        },
    );
    let err = fails(
        DatasetGen::new(None).table(TableGen::new("t").count(1)),
        &store,
    );
    assert!(err.contains("code"), "{err}");
}

#[test]
fn invalid_generator_names_table_and_column() {
    let bad: ColumnGen = serde_yaml_ng::from_str("range: { min: 5, max: 1 }").unwrap();
    let dataset = DatasetGen::new(None).table(
        TableGen::new("t")
            .column(FakerColumn::new("n", "int").with_generator(bad))
            .count(1),
    );
    let err = fails(dataset, &MemoryStore::new());
    assert!(err.contains("t") && err.contains("n"), "{err}");
}

#[test]
fn duplicate_table_names_are_an_error() {
    let dataset = DatasetGen::new(None)
        .table(TableGen::new("t").count(1))
        .table(TableGen::new("t").count(2));
    let err = fails(dataset, &MemoryStore::new());
    assert!(err.contains("declared twice"), "{err}");
}

#[test]
fn fan_out_on_a_plain_column_is_an_error() {
    let dataset = DatasetGen::new(None).table(TableGen::new("t").fan_out(FanOut {
        column: "k".into(),
        min: 1,
        max: 2,
    }));
    let err = fails(dataset, &MemoryStore::new());
    assert!(err.contains("not a reference column"), "{err}");
}

#[test]
fn a_late_error_seeds_no_table() {
    let bad: ColumnGen = serde_yaml_ng::from_str("range: { min: 5, max: 1 }").unwrap();
    let store = MemoryStore::new();
    let dataset = DatasetGen::new(None)
        .table(TableGen::new("good").count(3))
        .table(
            TableGen::new("bad")
                .column(FakerColumn::new("n", "int").with_generator(bad))
                .count(1),
        );
    fails(dataset, &store);
    assert!(store.table_names().is_empty());

    // A fan-out over a parent that generated nothing fails after planning,
    // still before any table is written.
    let dataset = DatasetGen::new(None).table(TableGen::new("parent")).table(
        linked("child", "parent_id", "parent").fan_out(FanOut {
            column: "parent_id".into(),
            min: 1,
            max: 2,
        }),
    );
    let err = fails(dataset, &store);
    assert!(err.contains("no rows"), "{err}");
    assert!(store.table_names().is_empty());
}
