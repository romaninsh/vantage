use ciborium::Value as CborValue;
use vantage_types::Record;

use super::*;

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

#[test]
fn insert_generates_counter_ids_and_sets_id_column() {
    let t = MemoryStore::new().table("ticket");
    let a = t.insert(rec(&[("status", text("Open"))])).unwrap();
    let b = t.insert(rec(&[("status", text("Closed"))])).unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("1", "2"));
    assert_eq!(t.get("1").unwrap().get("id"), Some(&text("1")));
    assert_eq!(t.ids(), vec!["1", "2"]);
}

#[test]
fn insert_uses_supplied_id_and_rejects_duplicates() {
    let t = MemoryStore::new().table("ticket");
    assert_eq!(t.insert(rec(&[("id", text("T-9"))])).unwrap(), "T-9");
    assert!(t.insert(rec(&[("id", text("T-9"))])).is_err());
    let n = t
        .insert(rec(&[("id", CborValue::Integer(7.into()))]))
        .unwrap();
    assert_eq!(n, "7");
}

#[test]
fn generated_ids_skip_supplied_ones() {
    let t = MemoryStore::new().table("ticket");
    t.insert(rec(&[("id", text("2"))])).unwrap();
    let ids: Vec<String> = (0..3).map(|_| t.insert(Record::new()).unwrap()).collect();
    assert_eq!(ids, vec!["1", "3", "4"]);
}

#[test]
fn id_prefix_applies_to_generated_ids() {
    let s = MemoryStore::new();
    let t = s.define(
        "ticket",
        TableDef {
            id_prefix: Some("T-".into()),
            ..TableDef::default()
        },
    );
    assert_eq!(t.insert(Record::new()).unwrap(), "T-1");
}

#[test]
fn upsert_inserts_then_replaces() {
    let t = MemoryStore::new().table("ticket");
    let mut rx = t.subscribe();
    t.upsert("a", rec(&[("n", CborValue::Integer(1.into()))]));
    t.upsert("a", rec(&[("n", CborValue::Integer(2.into()))]));
    assert!(matches!(
        rx.try_recv().unwrap(),
        MemoryChange::Inserted { .. }
    ));
    assert!(matches!(
        rx.try_recv().unwrap(),
        MemoryChange::Updated { .. }
    ));
    assert_eq!(
        t.get("a").unwrap().get("n"),
        Some(&CborValue::Integer(2.into()))
    );
    assert_eq!(t.get("a").unwrap().get("id"), Some(&text("a")));
    assert_eq!(t.writes(), 2);
}

#[test]
fn identical_upsert_sends_nothing() {
    let t = MemoryStore::new().table("ticket");
    t.upsert("a", rec(&[("n", CborValue::Integer(1.into()))]));
    let mut rx = t.subscribe();
    t.upsert("a", rec(&[("n", CborValue::Integer(1.into()))]));
    assert!(rx.try_recv().is_err());
    assert_eq!(t.writes(), 1);
}

#[test]
fn patch_merges_and_reports_old_row() {
    let t = MemoryStore::new().table("ticket");
    t.upsert(
        "a",
        rec(&[
            ("status", text("Open")),
            ("n", CborValue::Integer(1.into())),
        ]),
    );
    let mut rx = t.subscribe();
    assert!(t.patch("a", &rec(&[("status", text("Closed"))])));
    let row = t.get("a").unwrap();
    assert_eq!(row.get("status"), Some(&text("Closed")));
    assert_eq!(row.get("n"), Some(&CborValue::Integer(1.into())));
    match rx.try_recv().unwrap() {
        MemoryChange::Updated { old, row, .. } => {
            assert_eq!(old.get("status"), Some(&text("Open")));
            assert_eq!(row.get("status"), Some(&text("Closed")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn noop_patch_sends_nothing() {
    let t = MemoryStore::new().table("ticket");
    t.upsert("a", rec(&[("status", text("Open"))]));
    let mut rx = t.subscribe();
    assert!(t.patch("a", &rec(&[("status", text("Open"))])));
    assert!(rx.try_recv().is_err());
    assert_eq!(t.writes(), 1);
}

#[test]
fn patch_and_delete_on_missing_id_return_false_silently() {
    let t = MemoryStore::new().table("ticket");
    let mut rx = t.subscribe();
    assert!(!t.patch("nope", &rec(&[("status", text("x"))])));
    assert!(!t.delete("nope"));
    assert!(rx.try_recv().is_err());
    assert_eq!(t.writes(), 0);
}

#[test]
fn delete_removes_and_reports_old_row() {
    let t = MemoryStore::new().table("ticket");
    t.upsert("a", rec(&[("status", text("Open"))]));
    let mut rx = t.subscribe();
    assert!(t.delete("a"));
    assert!(t.get("a").is_none());
    match rx.try_recv().unwrap() {
        MemoryChange::Deleted { id, old } => {
            assert_eq!(id, "a");
            assert_eq!(old.get("status"), Some(&text("Open")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn quiet_stores_without_events_but_counts_writes() {
    let t = MemoryStore::new().table("ticket");
    let mut rx = t.subscribe();
    t.set_quiet(true);
    t.insert(Record::new()).unwrap();
    assert!(rx.try_recv().is_err());
    assert_eq!((t.len(), t.writes()), (1, 1));
    t.set_quiet(false);
    t.insert(Record::new()).unwrap();
    assert!(rx.try_recv().is_ok());
}

#[test]
fn store_returns_the_same_table_by_name() {
    let s = MemoryStore::new();
    s.table("a").insert(Record::new()).unwrap();
    assert_eq!(s.table("a").len(), 1);
    assert_eq!(s.table_names(), vec!["a"]);
}
