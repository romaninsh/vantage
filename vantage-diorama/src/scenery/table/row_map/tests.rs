use std::cell::Cell;

use vantage_types::Record;

use super::*;

thread_local! {
    static RESCANS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn note_rescan() {
    RESCANS.with(|c| c.set(c.get() + 1));
}

fn rescans() -> usize {
    RESCANS.with(Cell::get)
}

fn row(status: RowStatus) -> Row {
    Arc::new(EnrichedRecord {
        record: Record::new(),
        status,
        fetched_at: None,
    })
}

/// The breakdown counted the slow way, to check the running one against.
fn scanned(map: &RowMap) -> RowStatusSummary {
    let mut s = RowStatusSummary::default();
    for row in map.values() {
        tally(&mut s, &row.status, 1);
    }
    s
}

#[test]
fn hydrating_rows_never_rescans() {
    const N: usize = 500;
    let mut map = RowMap::from_map((0..N).map(|i| (i, row(RowStatus::Incomplete))).collect());
    let before = rescans();

    // The detail pass replaces each list-pass row as it hydrates, one at a
    // time; the counts follow without walking the map.
    for i in 0..N {
        let status = if i % 50 == 0 {
            RowStatus::LoadFailed { error: "x".into() }
        } else {
            RowStatus::Fresh
        };
        map.insert(i, row(status));
        let s = map.summary();
        assert_eq!(s.loaded, N);
        assert_eq!(s.incomplete, N - i - 1);
    }

    assert_eq!(rescans(), before, "hydration must not rescan the map");
    let s = map.summary();
    assert_eq!((s.fresh, s.failed, s.incomplete), (N - 10, 10, 0));
    assert_eq!(s, scanned(&map));
}

#[test]
fn writes_keep_counts_in_step() {
    let mut map = RowMap::default();
    map.insert(0, row(RowStatus::Fresh));
    map.insert(1, row(RowStatus::PendingWrite));
    map.insert(2, row(RowStatus::Incomplete));
    assert_eq!(map.summary(), scanned(&map));
    assert_eq!(map.summary().loaded, 3);

    map.insert(1, row(RowStatus::WriteFailed { error: "x".into() }));
    assert_eq!(map.summary().pending_write, 0);
    assert_eq!(map.summary().failed, 1);

    map.remove(&1);
    map.remove(&9);
    assert_eq!(map.summary(), scanned(&map));
    assert_eq!(map.summary().failed, 0);

    map.insert(5, row(RowStatus::PendingWrite));
    assert_eq!(map.truncate(2).len(), 2);
    assert_eq!(map.summary(), scanned(&map));
    assert_eq!(map.summary().loaded, 1);

    map.clear();
    assert_eq!(map.summary(), RowStatusSummary::default());
}

#[test]
fn shifts_keep_the_map_dense_and_counted() {
    let mut map = RowMap::from_map((0..4).map(|i| (i, row(RowStatus::Fresh))).collect());
    map.insert_shift(1, row(RowStatus::PendingWrite));
    assert_eq!(map.keys().copied().collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
    assert_eq!(map[&1].status, RowStatus::PendingWrite);
    assert_eq!(map.summary(), scanned(&map));

    let removed = map.remove_shift(1).expect("held");
    assert_eq!(removed.status, RowStatus::PendingWrite);
    assert_eq!(map.keys().copied().collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(map.summary(), scanned(&map));

    map.insert_shift(4, row(RowStatus::Incomplete));
    assert!(map.remove_shift(0).is_some());
    assert_eq!(map.keys().copied().collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(map[&3].status, RowStatus::Incomplete);
    assert_eq!(map.summary(), scanned(&map));
}
