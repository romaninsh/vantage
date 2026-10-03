use ciborium::Value as CborValue;
use vantage_memory::MemoryStore;

use super::{DatasetGen, ExtraFields, table::TableGen};
use crate::{ColumnGen, FakerColumn, FanOut};

mod errors;

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

fn text(row: &vantage_types::Record<CborValue>, col: &str) -> String {
    match row.get(col) {
        Some(CborValue::Text(s)) => s.clone(),
        other => panic!("{col} is not text: {other:?}"),
    }
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
        let c = text(&invoices.get(&id).unwrap(), "client_id");
        assert!(client_ids.contains(&c), "{c} is not a client id");
    }
}

#[test]
fn references_pick_only_rows_this_call_generated() {
    let store = MemoryStore::new();
    let client = store.table("client");
    for i in 0..50 {
        client.upsert(&format!("old-{i}"), vantage_types::Record::new());
    }
    let tables = DatasetGen::new(Some(8))
        .table(TableGen::new("client").count(3))
        .table(
            TableGen::new("invoice")
                .column(FakerColumn::new("client_id", "string"))
                .reference("client_id", "client")
                .count(40),
        )
        .generate(&store)
        .unwrap();
    let generated: Vec<String> = (0..3).map(crate::seed_id).collect();
    let invoices = &tables[1];
    for id in invoices.ids() {
        let c = text(&invoices.get(&id).unwrap(), "client_id");
        assert!(generated.contains(&c), "{c} was not generated in this call");
    }
}

#[test]
fn existing_table_gets_its_indexes() {
    let store = MemoryStore::new();
    store.table("t");
    DatasetGen::new(None)
        .table(TableGen::new("t").indexed(["k"]).count(1))
        .generate(&store)
        .unwrap();
    assert!(store.table("t").is_indexed("k"));
}

#[test]
fn same_seed_same_rows() {
    let (a, b) = (MemoryStore::new(), MemoryStore::new());
    r#gen().generate(&a).unwrap();
    r#gen().generate(&b).unwrap();
    for t in ["client", "invoice"] {
        let read = |s: &MemoryStore| -> Vec<_> {
            let table = s.table(t);
            table.ids().iter().map(|id| table.get(id)).collect()
        };
        assert_eq!(read(&a), read(&b), "{t}");
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
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(events[0], vantage_memory::MemoryChange::Reset));
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

#[test]
fn extra_fields_ride_on_every_row() {
    let store = MemoryStore::new();
    DatasetGen::new(Some(1))
        .table(
            TableGen::new("t")
                .column(FakerColumn::new("name", "string"))
                .extra_fields(ExtraFields { count: 2, size: 8 })
                .count(2),
        )
        .generate(&store)
        .unwrap();
    let row = store.table("t").get(&crate::seed_id(0)).unwrap();
    assert_eq!(row.len(), 4, "id, name and two extras");
    // "{id}:{i}:" cut to 8 bytes.
    assert_eq!(text(&row, "extra_0002"), "00000000");
}

#[test]
fn weirdness_changes_the_rows() {
    let names = |weirdness: f64| {
        let store = MemoryStore::new();
        DatasetGen::new(Some(4))
            .table(
                TableGen::new("t")
                    .column(FakerColumn::new("name", "string"))
                    .weirdness(weirdness)
                    .count(30),
            )
            .generate(&store)
            .unwrap();
        let t = store.table("t");
        t.ids()
            .iter()
            .map(|id| text(&t.get(id).unwrap(), "name"))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(0.0), names(0.0));
    assert_ne!(names(0.0), names(1.0));
}
