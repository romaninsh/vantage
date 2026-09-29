use ciborium::Value as CborValue;
use vantage_types::Record;

use super::*;
use crate::eval::{MemoryCondition, Query};
use vantage_vista::FilterOp;

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

#[test]
fn concurrent_patches_broadcast_in_apply_order() {
    let t = MemoryStore::new().table("ticket");
    let mut rx = t.subscribe();
    t.upsert("a", Record::new());

    let handles: Vec<_> = (0..4u32)
        .map(|thread| {
            let t = t.clone();
            std::thread::spawn(move || {
                for i in 0..200u32 {
                    let v = thread * 1000 + i;
                    t.patch("a", &rec(&[("n", CborValue::Integer(v.into()))]));
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    let mut events = Vec::new();
    while let Ok(change) = rx.try_recv() {
        events.push(change);
    }
    assert_eq!(events.len(), 1 + 4 * 200);

    for pair in events.windows(2) {
        let row = match &pair[0] {
            MemoryChange::Inserted { row, .. } | MemoryChange::Updated { row, .. } => row,
            other => panic!("{other:?}"),
        };
        let old = match &pair[1] {
            MemoryChange::Updated { old, .. } => old,
            other => panic!("{other:?}"),
        };
        assert_eq!(row, old, "events out of order relative to writes");
    }
}

fn indexed() -> MemoryTableHandle {
    let s = MemoryStore::new();
    s.define(
        "t",
        TableDef {
            indexed: vec!["status".into(), "n".into()],
            ..TableDef::default()
        },
    )
}

fn all_ids(t: &MemoryTableHandle, q: &Query) -> Vec<String> {
    t.query(q).unwrap().into_iter().map(|(id, _)| id).collect()
}

#[test]
fn indexed_eq_agrees_with_scan_through_writes() {
    let t = indexed();
    for (id, status) in [("a", "Open"), ("b", "Closed"), ("c", "Open")] {
        t.upsert(id, rec(&[("status", text(status))]));
    }
    t.patch("a", &rec(&[("status", text("Closed"))]));
    t.delete("c");
    t.upsert("d", rec(&[("status", text("Open"))]));
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, text("Closed")));
    assert_eq!(all_ids(&t, &q), ["a", "b"]);
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, text("Open")));
    assert_eq!(all_ids(&t, &q), ["d"]);
}

#[test]
fn add_index_covers_existing_rows_and_later_writes() {
    let t = MemoryStore::new().table("t");
    for (id, status) in [("a", "Open"), ("b", "Closed"), ("c", "Open")] {
        t.upsert(id, rec(&[("status", text(status))]));
    }
    assert!(!t.is_indexed("status"));
    t.add_index("status");
    assert!(t.is_indexed("status"));
    let open = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, text("Open")));
    assert!(t.rows.read().indexes.candidates(&open).is_some());
    assert_eq!(all_ids(&t, &open), ["a", "c"]);

    t.patch("a", &rec(&[("status", text("Closed"))]));
    t.upsert("d", rec(&[("status", text("Open"))]));
    t.delete("c");
    t.add_index("status");
    assert_eq!(all_ids(&t, &open), ["d"]);
    let closed = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, text("Closed")));
    assert_eq!(all_ids(&t, &closed), ["a", "b"]);
}

#[test]
fn indexed_in_set_keeps_insertion_order() {
    let t = indexed();
    for (id, status) in [("a", "x"), ("b", "y"), ("c", "z"), ("d", "x")] {
        t.upsert(id, rec(&[("status", text(status))]));
    }
    let set = CborValue::Array(vec![text("x"), text("z")]);
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::InSet, set));
    assert_eq!(all_ids(&t, &q), ["a", "c", "d"]);
}

#[test]
fn index_matches_int_and_integral_float() {
    let t = indexed();
    t.upsert("a", rec(&[("n", CborValue::Float(1.0))]));
    t.upsert("b", rec(&[("n", CborValue::Integer(2.into()))]));
    let q = Query::new().filter(MemoryCondition::cmp(
        "n",
        FilterOp::Eq,
        CborValue::Integer(1.into()),
    ));
    assert_eq!(all_ids(&t, &q), ["a"]);
}

#[test]
fn unkeyable_values_still_found_via_index_path() {
    let t = indexed();
    let map = CborValue::Map(vec![(text("k"), text("v"))]);
    t.upsert("a", rec(&[("status", map.clone())]));
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, map));
    assert_eq!(all_ids(&t, &q), ["a"]);
}

#[test]
fn huge_integral_float_found_by_indexed_eq_on_equal_huge_integer() {
    let t = indexed();
    // ciborium's `Integer` only holds magnitudes up to `u64::MAX`; this is
    // still well past `MAX_EXACT` (2^53), which is what the index cares about.
    let n: i128 = 1 << 60;
    t.upsert("a", rec(&[("n", CborValue::Float(n as f64))]));
    let big = ciborium::value::Integer::try_from(n).unwrap();
    let q = Query::new().filter(MemoryCondition::cmp(
        "n",
        FilterOp::Eq,
        CborValue::Integer(big),
    ));
    assert_eq!(all_ids(&t, &q), ["a"]);
}

#[test]
fn negative_zero_float_found_by_indexed_eq_zero() {
    let t = indexed();
    t.upsert("a", rec(&[("n", CborValue::Float(-0.0))]));
    let q = Query::new().filter(MemoryCondition::cmp(
        "n",
        FilterOp::Eq,
        CborValue::Integer(0.into()),
    ));
    assert_eq!(all_ids(&t, &q), ["a"]);
}

#[test]
fn distinct_huge_integers_sharing_an_index_key_are_filtered_exactly() {
    let t = indexed();
    let a: i128 = 1 << 60;
    let b = a + 1; // rounds to the same f64 as `a`, so they share an index key
    assert_eq!(a as f64, b as f64);
    t.upsert(
        "a",
        rec(&[(
            "n",
            CborValue::Integer(ciborium::value::Integer::try_from(a).unwrap()),
        )]),
    );
    t.upsert(
        "b",
        rec(&[(
            "n",
            CborValue::Integer(ciborium::value::Integer::try_from(b).unwrap()),
        )]),
    );
    let q = Query::new().filter(MemoryCondition::cmp(
        "n",
        FilterOp::Eq,
        CborValue::Integer(ciborium::value::Integer::try_from(a).unwrap()),
    ));
    assert_eq!(all_ids(&t, &q), ["a"]);
}
