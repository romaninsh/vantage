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
fn insert_as_stores_and_rejects_duplicates() {
    let t = MemoryStore::new().table("t");
    let row = t.insert_as("a", rec(&[("n", text("x"))])).unwrap();
    assert_eq!(row.get("id"), Some(&text("a")));
    let mut rx = t.subscribe();
    let err = t.insert_as("a", Record::new()).unwrap_err().to_string();
    assert!(err.contains("already exists"), "{err}");
    assert!(rx.try_recv().is_err());
    assert_eq!(t.get("a").unwrap().get("n"), Some(&text("x")));
}

#[test]
fn replace_missing_is_none_and_silent() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    assert!(t.replace("a", rec(&[("n", text("x"))])).is_none());
    assert!(t.is_empty());
    assert!(rx.try_recv().is_err());
    t.upsert("a", Record::new());
    let row = t.replace("a", rec(&[("n", text("y"))])).unwrap();
    assert_eq!(row.get("n"), Some(&text("y")));
}

#[test]
fn upsert_reports_its_outcome() {
    let t = MemoryStore::new().table("t");
    assert_eq!(
        t.upsert("a", rec(&[("n", text("x"))])),
        UpsertOutcome::Inserted
    );
    assert_eq!(
        t.upsert("a", rec(&[("n", text("x"))])),
        UpsertOutcome::Unchanged
    );
    assert_eq!(
        t.upsert("a", rec(&[("n", text("y"))])),
        UpsertOutcome::Updated
    );
}

#[test]
fn patch_cannot_change_the_id_column() {
    let t = MemoryStore::new().table("t");
    t.upsert("a", rec(&[("n", text("x"))]));
    assert!(t.patch("a", &rec(&[("id", text("b")), ("n", text("y"))])));
    let row = t.get("a").unwrap();
    assert_eq!(row.get("id"), Some(&text("a")));
    assert_eq!(row.get("n"), Some(&text("y")));
}

#[test]
fn unquiet_after_quiet_writes_sends_reset() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    t.set_quiet(true);
    t.set_quiet(false);
    assert!(rx.try_recv().is_err(), "no writes, no reset");
    t.set_quiet(true);
    t.upsert("a", Record::new());
    t.set_quiet(false);
    let change = rx.try_recv().unwrap();
    assert!(matches!(change, MemoryChange::Reset));
    assert_eq!(change.id(), None);
    assert!(rx.try_recv().is_err());
}

#[test]
fn quiet_toggles_never_lose_a_write() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (t, stop) = (t.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(Ordering::Relaxed) && n < 3000 {
                t.upsert(&n.to_string(), Record::new());
                n += 1;
            }
            n
        })
    };
    for i in 0..400 {
        t.set_quiet(i % 2 == 0);
    }
    t.set_quiet(false);
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    while rx.try_recv().is_ok() {}
    // The race strands `missed = true` after quiet mode has ended, so an
    // empty quiet cycle would then send a spurious Reset.
    t.set_quiet(true);
    t.set_quiet(false);
    assert!(rx.try_recv().is_err(), "an empty quiet cycle sent an event");
    assert!(!t.is_quiet());
}

#[test]
fn index_follows_changed_cells_only() {
    let s = MemoryStore::new();
    let t = s.define(
        "t",
        TableDef {
            indexed: vec!["status".into()],
            ..TableDef::default()
        },
    );
    let eq = |v: &str| Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, text(v)));
    t.upsert("a", rec(&[("status", text("Open")), ("n", text("1"))]));
    t.upsert("b", rec(&[("status", text("Open"))]));
    t.patch("a", &rec(&[("n", text("2"))]));
    let open: Vec<String> = t
        .query(&eq("Open"))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(open, ["a", "b"]);
    t.replace("a", rec(&[("status", text("Closed"))]));
    assert_eq!(t.count(&eq("Open")).unwrap(), 1);
    assert_eq!(t.count(&eq("Closed")).unwrap(), 1);
    t.delete("b");
    assert_eq!(t.count(&eq("Open")).unwrap(), 0);
}
