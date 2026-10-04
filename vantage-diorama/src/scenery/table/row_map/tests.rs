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
        dirty_fields: None,
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
    map.insert(2, row(RowStatus::Stale));
    assert_eq!(map.summary(), scanned(&map));
    assert_eq!(map.summary().loaded, 3);

    map.insert(1, row(RowStatus::WriteFailed { error: "x".into() }));
    assert_eq!(map.summary().pending_write, 0);
    assert_eq!(map.summary().failed, 1);

    map.remove(&1);
    map.remove(&9);
    assert_eq!(map.summary(), scanned(&map));
    assert_eq!(map.summary().failed, 0);

    map.clear();
    assert_eq!(map.summary(), RowStatusSummary::default());
}
