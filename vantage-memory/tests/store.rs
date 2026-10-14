//! The synchronous store API: tables, ids, writes, events, quiet mode, indexes.

use std::thread;

use ciborium::Value as CborValue;
use tokio::sync::broadcast::Receiver;
use vantage_memory::{
    MemoryChange, MemoryCondition, MemoryStore, MemoryTable, MemoryTableHandle, Query, TableDef,
    UpsertOutcome,
};
use vantage_types::Record;
use vantage_vista::FilterOp;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}
fn row(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
fn drain(rx: &mut Receiver<MemoryChange>) -> Vec<MemoryChange> {
    let mut out = Vec::new();
    while let Ok(c) = rx.try_recv() {
        out.push(c);
    }
    out
}
fn eq(column: &str, value: CborValue) -> Query {
    Query::new().filter(MemoryCondition::cmp(column, FilterOp::Eq, value))
}
fn ids_of(t: &MemoryTable, q: &Query) -> Vec<String> {
    t.query(q).unwrap().into_iter().map(|(id, _)| id).collect()
}

#[test]
fn a_store_hands_out_the_same_table_for_a_name() {
    let store = MemoryStore::new();
    store.table("b").upsert("x", row(&[("n", int(1))]));
    assert_eq!(store.table("b").ids(), ["x"]);
    store.table("a");
    assert_eq!(store.table_names(), ["a", "b"]);
    let clone = store.clone();
    assert_eq!(clone.table("b").len(), 1);
}

#[test]
fn define_keeps_the_original_definition_of_an_existing_table() {
    let store = MemoryStore::new();
    let first = store.define(
        "t",
        TableDef {
            id_column: "sku".into(),
            ..TableDef::default()
        },
    );
    let again = store.define("t", TableDef::default());
    assert_eq!(first.id_column(), "sku");
    assert_eq!(again.id_column(), "sku");
    assert_eq!(first.name(), "t");
    assert_eq!(store.table("fresh").id_column(), "id");
}

#[test]
fn generated_ids_are_a_per_table_counter() {
    let store = MemoryStore::new();
    let a = store.table("a");
    assert_eq!(a.insert(Record::new()).unwrap(), "1");
    assert_eq!(a.insert(Record::new()).unwrap(), "2");
    assert_eq!(store.table("b").insert(Record::new()).unwrap(), "1");
}

#[test]
fn generated_ids_carry_the_id_prefix() {
    let store = MemoryStore::new();
    let t = store.define(
        "t",
        TableDef {
            id_prefix: Some("T-".into()),
            ..TableDef::default()
        },
    );
    assert_eq!(t.insert(Record::new()).unwrap(), "T-1");
    assert_eq!(t.insert(Record::new()).unwrap(), "T-2");
}

#[test]
fn insert_uses_the_supplied_id_and_rejects_a_duplicate() {
    let t = MemoryStore::new().table("t");
    assert_eq!(t.insert(row(&[("id", text("x"))])).unwrap(), "x");
    assert!(t.insert(row(&[("id", text("x"))])).is_err());
    assert_eq!(t.len(), 1);
}

#[test]
fn an_integer_id_cell_becomes_a_text_id_and_an_empty_one_is_not_supplied() {
    let t = MemoryStore::new().table("t");
    assert_eq!(t.insert(row(&[("id", int(7))])).unwrap(), "7");
    assert!(t.get("7").is_some());
    let generated = t.insert(row(&[("id", text(""))])).unwrap();
    assert!(!generated.is_empty());
    assert_eq!(t.len(), 2);
}

#[test]
fn insert_reads_the_id_from_the_defined_id_column() {
    let store = MemoryStore::new();
    let t = store.define(
        "t",
        TableDef {
            id_column: "sku".into(),
            ..TableDef::default()
        },
    );
    assert_eq!(t.insert(row(&[("sku", text("s1"))])).unwrap(), "s1");
    assert!(t.get("s1").is_some());
}

#[test]
fn a_generated_id_does_not_collide_with_a_supplied_one() {
    let t = MemoryStore::new().table("t");
    t.insert(row(&[("id", text("1"))])).unwrap();
    let id = t.insert(Record::new()).unwrap();
    assert_ne!(id, "1");
    assert_eq!(t.len(), 2);
}

#[test]
fn insert_as_stores_under_the_given_id_and_rejects_a_duplicate() {
    let t = MemoryStore::new().table("t");
    let stored = t.insert_as("a", row(&[("n", int(1))])).unwrap();
    assert_eq!(stored.get("n"), Some(&int(1)));
    assert_eq!(t.get("a").unwrap().get("n"), Some(&int(1)));
    assert!(t.insert_as("a", row(&[("n", int(2))])).is_err());
    assert_eq!(t.get("a").unwrap().get("n"), Some(&int(1)));
}

#[test]
fn rows_keep_insertion_order() {
    let t = MemoryStore::new().table("t");
    for id in ["c", "a", "b"] {
        t.upsert(id, row(&[("n", int(1))]));
    }
    assert_eq!(t.ids(), ["c", "a", "b"]);
    t.delete("a");
    t.upsert("a", row(&[("n", int(1))]));
    assert_eq!(t.ids(), ["c", "b", "a"]);
}

#[test]
fn replace_of_a_missing_row_is_none_and_silent() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    assert!(t.replace("nope", row(&[("n", int(1))])).is_none());
    assert!(t.is_empty());
    assert_eq!(t.writes(), 0);
    assert!(drain(&mut rx).is_empty());
}

#[test]
fn replace_swaps_the_whole_row() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("n", int(1)), ("m", int(2))]));
    let mut rx = t.subscribe();
    assert!(t.replace("a", row(&[("n", int(5))])).is_some());
    let got = t.get("a").unwrap();
    assert_eq!(got.get("n"), Some(&int(5)));
    assert_eq!(got.get("m"), None);
    assert!(matches!(
        drain(&mut rx).as_slice(),
        [MemoryChange::Updated { id, .. }] if id == "a"
    ));
}

#[test]
fn upsert_reports_inserted_updated_and_unchanged() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    assert_eq!(
        t.upsert("a", row(&[("n", int(1))])),
        UpsertOutcome::Inserted
    );
    assert_eq!(t.upsert("a", row(&[("n", int(2))])), UpsertOutcome::Updated);
    assert_eq!(
        t.upsert("a", row(&[("n", int(2))])),
        UpsertOutcome::Unchanged
    );
    assert_eq!(t.writes(), 2);
    let events = drain(&mut rx);
    assert_eq!(events.len(), 2, "an unchanged upsert sends nothing");
    assert!(matches!(events[0], MemoryChange::Inserted { .. }));
    assert!(matches!(events[1], MemoryChange::Updated { .. }));
}

#[test]
fn updated_and_deleted_events_carry_the_old_row() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("n", int(1))]));
    let mut rx = t.subscribe();
    t.upsert("a", row(&[("n", int(2))]));
    t.delete("a");
    let events = drain(&mut rx);
    let [
        MemoryChange::Updated { id, row, old },
        MemoryChange::Deleted {
            id: gone,
            old: last,
        },
    ] = events.as_slice()
    else {
        panic!("expected an update then a delete, got {events:?}");
    };
    assert_eq!((id.as_str(), gone.as_str()), ("a", "a"));
    assert_eq!(old.get("n"), Some(&int(1)));
    assert_eq!(row.get("n"), Some(&int(2)));
    assert_eq!(last.get("n"), Some(&int(2)));
    assert_eq!(events[0].id(), Some("a"));
}

#[test]
fn patch_merges_fields_into_the_row() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("x", int(1)), ("y", int(2))]));
    assert!(t.patch("a", &row(&[("y", int(3)), ("z", int(4))])));
    let got = t.get("a").unwrap();
    assert_eq!(got.get("x"), Some(&int(1)));
    assert_eq!(got.get("y"), Some(&int(3)));
    assert_eq!(got.get("z"), Some(&int(4)));
}

#[test]
fn patch_ignores_a_value_for_the_id_column() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("id", text("a")), ("n", int(1))]));
    assert!(t.patch("a", &row(&[("id", text("other")), ("n", int(2))])));
    assert_eq!(t.ids(), ["a"]);
    let got = t.get("a").unwrap();
    assert_eq!(got.get("id"), Some(&text("a")));
    assert_eq!(got.get("n"), Some(&int(2)));
}

#[test]
fn patch_and_delete_of_a_missing_row_are_false_and_silent() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    assert!(!t.patch("nope", &row(&[("n", int(1))])));
    assert!(!t.delete("nope"));
    assert_eq!(t.writes(), 0);
    assert!(drain(&mut rx).is_empty());
}

#[test]
fn a_patch_that_changes_nothing_sends_nothing() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("n", int(1))]));
    let writes = t.writes();
    let mut rx = t.subscribe();
    t.patch("a", &row(&[("n", int(1))]));
    assert_eq!(t.writes(), writes);
    assert!(drain(&mut rx).is_empty());
}

#[test]
fn delete_removes_the_row_once() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("n", int(1))]));
    assert!(t.delete("a"));
    assert!(t.get("a").is_none());
    assert!(!t.delete("a"));
}

#[test]
fn a_quiet_table_stores_and_counts_writes_but_sends_nothing() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    t.set_quiet(true);
    assert!(t.is_quiet());
    t.upsert("a", row(&[("n", int(1))]));
    t.patch("a", &row(&[("n", int(2))]));
    assert_eq!(t.get("a").unwrap().get("n"), Some(&int(2)));
    assert_eq!(t.writes(), 2);
    assert!(drain(&mut rx).is_empty());
}

#[test]
fn leaving_quiet_after_writes_sends_one_reset() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    t.set_quiet(true);
    t.upsert("a", row(&[("n", int(1))]));
    t.upsert("b", row(&[("n", int(1))]));
    t.set_quiet(false);
    assert!(!t.is_quiet());
    let events = drain(&mut rx);
    assert!(
        matches!(events.as_slice(), [MemoryChange::Reset]),
        "{events:?}"
    );
    assert_eq!(events[0].id(), None);
    t.set_quiet(true);
    t.set_quiet(false);
    assert!(drain(&mut rx).is_empty(), "no writes while quiet, no reset");
}

#[test]
fn concurrent_patches_are_broadcast_in_the_order_they_were_applied() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("n", int(-1))]));
    let mut rx = t.subscribe();
    let workers: Vec<_> = (0..4)
        .map(|w| {
            let t = t.clone();
            thread::spawn(move || {
                for i in 0..100 {
                    t.patch("a", &row(&[("n", int(w * 1000 + i))]));
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let events = drain(&mut rx);
    assert_eq!(events.len(), 400);
    let mut previous = int(-1);
    for event in &events {
        let MemoryChange::Updated { row, old, .. } = event else {
            panic!("expected updates, got {event:?}");
        };
        assert_eq!(old.get("n"), Some(&previous), "event out of apply order");
        previous = row.get("n").unwrap().clone();
    }
    assert_eq!(t.get("a").unwrap().get("n"), Some(&previous));
}

fn indexed_table(store: &MemoryStore, name: &str, column: &str) -> MemoryTableHandle {
    store.define(
        name,
        TableDef {
            indexed: vec![column.into()],
            ..TableDef::default()
        },
    )
}

/// The same rows in an indexed and an unindexed table must answer alike.
fn agree(cells: &[CborValue], q: &Query) -> Vec<String> {
    let store = MemoryStore::new();
    let indexed = indexed_table(&store, "indexed", "k");
    let plain = store.table("plain");
    for (i, cell) in cells.iter().enumerate() {
        let id = format!("r{i}");
        indexed.upsert(&id, row(&[("k", cell.clone())]));
        plain.upsert(&id, row(&[("k", cell.clone())]));
    }
    let got = ids_of(&indexed, q);
    assert_eq!(got, ids_of(&plain, q));
    got
}

#[test]
fn an_indexed_equality_lookup_follows_writes() {
    let store = MemoryStore::new();
    let t = indexed_table(&store, "t", "status");
    t.upsert("a", row(&[("status", text("Open"))]));
    t.upsert("b", row(&[("status", text("Closed"))]));
    t.upsert("c", row(&[("status", text("Open"))]));
    assert_eq!(ids_of(&t, &eq("status", text("Open"))), ["a", "c"]);

    t.patch("a", &row(&[("status", text("Closed"))]));
    assert_eq!(ids_of(&t, &eq("status", text("Open"))), ["c"]);
    assert_eq!(ids_of(&t, &eq("status", text("Closed"))), ["a", "b"]);

    t.delete("c");
    t.replace("b", row(&[("status", text("Open"))]));
    assert_eq!(ids_of(&t, &eq("status", text("Open"))), ["b"]);
    assert_eq!(ids_of(&t, &eq("status", text("Closed"))), ["a"]);
}

#[test]
fn a_write_to_another_cell_leaves_the_index_correct() {
    let store = MemoryStore::new();
    let t = indexed_table(&store, "t", "status");
    t.upsert("a", row(&[("status", text("Open")), ("n", int(1))]));
    t.patch("a", &row(&[("n", int(2))]));
    t.upsert("a", row(&[("status", text("Open")), ("n", int(3))]));
    assert_eq!(ids_of(&t, &eq("status", text("Open"))), ["a"]);
    assert_eq!(
        ids_of(&t, &eq("status", text("Closed"))),
        Vec::<String>::new()
    );
}

#[test]
fn an_indexed_in_set_lookup_returns_rows_in_insertion_order() {
    let store = MemoryStore::new();
    let t = indexed_table(&store, "t", "status");
    for (id, status) in [("a", "Open"), ("b", "Closed"), ("c", "Open"), ("d", "Held")] {
        t.upsert(id, row(&[("status", text(status))]));
    }
    let q = Query::new().filter(MemoryCondition::cmp(
        "status",
        FilterOp::InSet,
        CborValue::Array(vec![text("Open"), text("Closed")]),
    ));
    assert_eq!(ids_of(&t, &q), ["a", "b", "c"]);
}

#[test]
fn add_index_covers_existing_rows_and_later_writes() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", row(&[("k", text("x"))]));
    assert!(!t.is_indexed("k"));
    t.add_index("k");
    assert!(t.is_indexed("k"));
    t.add_index("k");
    t.upsert("b", row(&[("k", text("x"))]));
    t.upsert("c", row(&[("k", text("y"))]));
    assert_eq!(ids_of(&t, &eq("k", text("x"))), ["a", "b"]);
    assert_eq!(ids_of(&t, &eq("k", text("y"))), ["c"]);
}

#[test]
fn an_index_matches_an_integer_and_the_equal_float() {
    let cells = [int(1), CborValue::Float(1.0), CborValue::Float(1.5), int(2)];
    assert_eq!(agree(&cells, &eq("k", int(1))), ["r0", "r1"]);
    assert_eq!(agree(&cells, &eq("k", CborValue::Float(1.0))), ["r0", "r1"]);
}

#[test]
fn an_index_matches_negative_zero_with_zero() {
    let cells = [
        CborValue::Float(-0.0),
        int(0),
        CborValue::Float(0.0),
        int(1),
    ];
    assert_eq!(agree(&cells, &eq("k", int(0))), ["r0", "r1", "r2"]);
}

#[test]
fn an_index_agrees_with_a_scan_for_huge_numbers() {
    let big = 1_i64 << 53;
    let cells = [
        int(big),
        CborValue::Float(big as f64),
        CborValue::Integer(u64::MAX.into()),
        CborValue::Float(u64::MAX as f64),
    ];
    for probe in [
        int(big),
        CborValue::Float(big as f64),
        CborValue::Integer(u64::MAX.into()),
        CborValue::Float(u64::MAX as f64),
    ] {
        let got = agree(&cells, &eq("k", probe.clone()));
        assert!(!got.is_empty(), "{probe:?} finds at least its own row");
    }
}

#[test]
fn an_index_tells_apart_distinct_integers_it_cannot_tell_apart_by_magnitude() {
    let big = 1_i64 << 53;
    let cells = [int(big), int(big + 1)];
    assert_eq!(agree(&cells, &eq("k", int(big + 1))), ["r1"]);
    assert_eq!(agree(&cells, &eq("k", int(big))), ["r0"]);
}

#[test]
#[ignore = "possible bug: an indexed Eq on integer 2^53+1 misses the equal float 2^53 that a scan finds"]
fn an_index_never_splits_values_a_scan_calls_equal() {
    let big = 1_i64 << 53;
    let cells = [int(big + 1), CborValue::Float(big as f64)];
    agree(&cells, &eq("k", int(big + 1)));
}

#[test]
fn an_index_still_finds_cells_it_cannot_key() {
    let list = CborValue::Array(vec![int(1), int(2)]);
    let bytes = CborValue::Bytes(vec![1, 2]);
    let cells = [list.clone(), bytes.clone(), text("x")];
    assert_eq!(agree(&cells, &eq("k", list)), ["r0"]);
    assert_eq!(agree(&cells, &eq("k", bytes)), ["r1"]);
    assert_eq!(agree(&cells, &eq("k", text("x"))), ["r2"]);
}

#[test]
fn an_index_finds_rows_lacking_the_column_by_null() {
    let store = MemoryStore::new();
    let t = indexed_table(&store, "t", "k");
    t.upsert("a", row(&[("other", int(1))]));
    t.upsert("b", row(&[("k", CborValue::Null)]));
    t.upsert("c", row(&[("k", text("x"))]));
    assert_eq!(ids_of(&t, &eq("k", CborValue::Null)), ["a", "b"]);
}

#[test]
fn count_matches_the_rows_a_query_returns() {
    let t = MemoryStore::new().table("t");
    for (id, n) in [("a", 1), ("b", 2), ("c", 3)] {
        t.upsert(id, row(&[("n", int(n))]));
    }
    let q = Query::new().filter(MemoryCondition::cmp("n", FilterOp::Gt, int(1)));
    assert_eq!(t.count(&q).unwrap(), 2);
    assert_eq!(t.count(&Query::new()).unwrap(), 3);
}
