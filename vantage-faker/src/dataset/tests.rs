use vantage_memory::MemoryStore;

use super::{DatasetGen, table::TableGen};
use crate::{ColumnGen, FakerColumn, FanOut};

fn pick(values: &[&str]) -> ColumnGen {
    serde_yaml_ng::from_str(&format!("pick: {{ values: [{}] }}", values.join(", "))).unwrap()
}

// `gen` is a reserved keyword as of edition 2024.
fn r#gen() -> DatasetGen {
    DatasetGen::new(Some(42))
        .table(
            TableGen::new("invoice")
                .column(FakerColumn::new("client_id", "string"))
                .column(
                    FakerColumn::new("status", "string").with_generator(pick(&["Open", "Paid"])),
                )
                .reference("client_id", "client")
                .fan_out(FanOut {
                    column: "client_id".into(),
                    min: 1,
                    max: 3,
                })
                .indexed(["client_id"]),
        )
        .table(
            TableGen::new("client")
                .column(FakerColumn::new("name", "string"))
                .count(5),
        )
}

#[test]
fn referenced_tables_are_generated_first() {
    let store = MemoryStore::new();
    let tables = r#gen().generate(&store).unwrap();
    let names: Vec<&str> = tables.iter().map(|t| t.name()).collect();
    assert_eq!(names, ["client", "invoice"]);
    assert_eq!(store.table("client").len(), 5);
    let invoices = store.table("invoice");
    assert!((5..=15).contains(&invoices.len()));
    assert!(invoices.is_indexed("client_id"));
    let client_ids: Vec<String> = store.table("client").ids();
    for id in invoices.ids() {
        let row = invoices.get(&id).unwrap();
        let Some(ciborium::Value::Text(c)) = row.get("client_id") else {
            panic!("{row:?}")
        };
        assert!(client_ids.contains(c), "{c} is not a client id");
    }
}

#[test]
fn same_seed_same_rows() {
    let (a, b) = (MemoryStore::new(), MemoryStore::new());
    r#gen().generate(&a).unwrap();
    r#gen().generate(&b).unwrap();
    for t in ["client", "invoice"] {
        let ra: Vec<_> = a
            .table(t)
            .ids()
            .into_iter()
            .map(|id| a.table(t).get(&id))
            .collect();
        let rb: Vec<_> = b
            .table(t)
            .ids()
            .into_iter()
            .map(|id| b.table(t).get(&id))
            .collect();
        assert_eq!(ra, rb, "{t}");
    }
}

#[test]
fn seeding_is_quiet_then_resets() {
    let store = MemoryStore::new();
    let client = store.table("client");
    let mut rx = client.subscribe();
    DatasetGen::new(Some(1))
        .table(TableGen::new("client").count(3))
        .generate(&store)
        .unwrap();
    let mut events = Vec::new();
    while let Ok(c) = rx.try_recv() {
        events.push(c);
    }
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(events[0], vantage_memory::MemoryChange::Reset));
}

#[test]
fn cycle_is_an_error() {
    let err = DatasetGen::new(None)
        .table(
            TableGen::new("a")
                .column(FakerColumn::new("b_id", "string"))
                .reference("b_id", "b"),
        )
        .table(
            TableGen::new("b")
                .column(FakerColumn::new("a_id", "string"))
                .reference("a_id", "a"),
        )
        .generate(&MemoryStore::new())
        .err()
        .unwrap();
    assert!(err.contains("cycle"), "{err}");
}

#[test]
fn unknown_reference_is_an_error() {
    let err = DatasetGen::new(None)
        .table(
            TableGen::new("a")
                .column(FakerColumn::new("x_id", "string"))
                .reference("x_id", "x"),
        )
        .generate(&MemoryStore::new())
        .err()
        .unwrap();
    assert!(err.contains("a") && err.contains("x"), "{err}");
}

#[test]
fn count_zero_creates_an_empty_table() {
    let store = MemoryStore::new();
    DatasetGen::new(None)
        .table(TableGen::new("empty"))
        .generate(&store)
        .unwrap();
    assert!(store.table_names().contains(&"empty".to_string()));
    assert_eq!(store.table("empty").len(), 0);
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
    let err = DatasetGen::new(None)
        .table(TableGen::new("t").count(1))
        .generate(&store)
        .err()
        .unwrap();
    assert!(err.contains("code"), "{err}");
}

#[test]
fn invalid_generator_names_table_and_column() {
    let bad: ColumnGen = serde_yaml_ng::from_str("range: { min: 5, max: 1 }").unwrap();
    let err = DatasetGen::new(None)
        .table(
            TableGen::new("t")
                .column(FakerColumn::new("n", "int").with_generator(bad))
                .count(1),
        )
        .generate(&MemoryStore::new())
        .err()
        .unwrap();
    assert!(err.contains("t") && err.contains("n"), "{err}");
}

#[test]
fn id_column_generator_is_ignored() {
    let store = MemoryStore::new();
    DatasetGen::new(Some(3))
        .table(
            TableGen::new("t")
                .column(FakerColumn::new("id", "string").with_generator(pick(&["x"])))
                .count(2),
        )
        .generate(&store)
        .unwrap();
    let ids = store.table("t").ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| id != "x"));
}
