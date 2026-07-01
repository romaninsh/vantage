//! Client-side sort must survive a LIVE `patched` insert on an eager
//! (single-pass) Dio — the analogue of `chunk_sort.rs`'s refresh guarantee, but
//! for the live push path (`dio.patched`) that a streaming source (faker) drives.
//! A newly inserted row must land at its *sorted* position, not be appended in
//! insertion order. This is the path that decayed the faker dashboard's sorted
//! pie chart: correct at open, drifting as fifo inserts arrived.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Dio, Lens, SortDir, TableScenery};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

mod support;
use support::chunk::wait_for_gen;
use support::eager_dio;

fn amount_rec(id: &str, amount: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("id".to_string(), CborValue::Text(id.to_string()));
    r.insert("amount".to_string(), CborValue::Integer(amount.into()));
    r
}

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("amount", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", amount_rec("a", 30))
        .with_record("b", amount_rec("b", 10))
        .with_record("c", amount_rec("c", 20));
    Vista::new("items", Box::new(shell))
}

fn order(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| {
            scenery.row(i).and_then(|r| match r.record.get("id") {
                Some(CborValue::Text(t)) => Some(t.clone()),
                _ => None,
            })
        })
        .collect()
}

#[tokio::test]
async fn client_sort_survives_live_patched_insert() -> Result<()> {
    let dio = eager_dio(master()).await;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    // Sort by amount descending → a(30), c(20), b(10).
    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("amount".to_string()), SortDir::Desc);
    wait_for_gen(&mut gen_rx, g0).await;
    assert_eq!(order(&scenery), vec!["a", "c", "b"], "sorted desc at open");

    // A live insert of the smallest amount must land at the BOTTOM.
    let g1 = u64::from(*gen_rx.borrow_and_update());
    dio.patched("d", amount_rec("d", 5)).await?;
    wait_for_gen(&mut gen_rx, g1).await;
    assert_eq!(
        order(&scenery),
        vec!["a", "c", "b", "d"],
        "small live insert must sort to the bottom, not append blindly"
    );

    // A live insert of a middle amount must land in its sorted position.
    let g2 = u64::from(*gen_rx.borrow_and_update());
    dio.patched("e", amount_rec("e", 25)).await?;
    wait_for_gen(&mut gen_rx, g2).await;
    assert_eq!(
        order(&scenery),
        vec!["a", "e", "c", "b", "d"],
        "middle live insert must sort into place"
    );

    Ok(())
}

fn float_rec(id: &str, amount: f64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("id".to_string(), CborValue::Text(id.to_string()));
    r.insert("amount".to_string(), CborValue::Float(amount));
    r
}

/// A `Float` sort column must order numerically, not by the debug-string of the
/// value. Amounts are picked so lexicographic order of `"Float(<n>)"` diverges
/// from numeric order (differing digit counts / leading digits) — the faker
/// dashboard's real shape (`amount` is a decimal), and the true cause of the
/// "sorted pie decays" report.
#[tokio::test]
async fn float_column_sorts_numerically_not_lexically() -> Result<()> {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("amount", "f64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("hi", float_rec("hi", 6416.51))
        .with_record("mid", float_rec("mid", 1826.19))
        .with_record("lo", float_rec("lo", 657.96));
    let dio = eager_dio(Vista::new("items", Box::new(shell))).await;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("amount".to_string()), SortDir::Desc);
    wait_for_gen(&mut gen_rx, g0).await;

    // Numeric desc: 6416.51, 1826.19, 657.96 → [hi, mid, lo].
    // A lexical (debug-string) comparator ranks "657.." above "1826.." / "6416.."
    // → [lo, hi, mid] (or similar) — the decay signature.
    assert_eq!(
        order(&scenery),
        vec!["hi", "mid", "lo"],
        "float column must sort by numeric value, not debug string"
    );
    Ok(())
}

// ---- fuller reproduction: shared Dio + on_refresh + interleaved refresh ------

fn amounts(scenery: &Arc<dyn TableScenery>) -> Vec<i64> {
    (0..scenery.row_count())
        .filter_map(|i| {
            scenery.row(i).and_then(|r| match r.record.get("amount") {
                Some(CborValue::Integer(v)) => i64::try_from(*v).ok(),
                _ => None,
            })
        })
        .collect()
}

fn is_desc(scenery: &Arc<dyn TableScenery>) -> bool {
    amounts(scenery).windows(2).all(|w| w[0] >= w[1])
}

/// Eager Dio whose `on_refresh` snapshot-replaces the cache from the master —
/// exactly `vantage-ui`'s `build_eager_lens` shape (the path faker uses).
async fn eager_dio_with_refresh(master: Vista) -> Dio {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .on_refresh(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().clear().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("lens builds"),
    );
    lens.make_dio(master).await.expect("make_dio")
}

/// Mirrors the running faker dashboard: one sorted scenery and one unsorted
/// scenery share a single eager Dio; a fifo stream writes to the master AND
/// patches the Dio (the effect + forwarder), while the list's `request_refresh`
/// fires periodically. The sorted view must stay sorted through all of it.
#[tokio::test]
async fn client_sort_survives_shared_dio_live_stream() -> Result<()> {
    let meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("amount", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(meta)
        .with_record("a", amount_rec("a", 30))
        .with_record("b", amount_rec("b", 10))
        .with_record("c", amount_rec("c", 20));
    let master_handle = shell.clone();
    let dio = eager_dio_with_refresh(Vista::new("items", Box::new(shell))).await;

    // Sorted scenery opened WITH the sort (like vantage-ui's `open_scenery`), plus
    // an unsorted sibling on the same Dio (like the feed-order list / charts).
    let sorted = dio
        .table_scenery()
        .sort("amount", SortDir::Desc)
        .open()
        .await?;
    let _unsorted = dio.table_scenery().open().await?;

    let mut gen_rx = sorted.subscribe();
    sorted.set_viewport(0..100);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(is_desc(&sorted), "sorted at open: {:?}", amounts(&sorted));

    for i in 0..12i64 {
        let id = format!("live{i}");
        let amount = (i * 13 + 5) % 40; // spread across the range
        // Effect writes to the master store...
        master_handle.set_record(&id, amount_rec(&id, amount));
        // ...forwarder patches the Dio cache + broadcasts RecordChanged.
        let gp = u64::from(*gen_rx.borrow_and_update());
        dio.patched(&id, amount_rec(&id, amount)).await?;
        wait_for_gen(&mut gen_rx, gp).await;

        // The list's 2s poll fires a whole-Dio refresh on some ticks.
        if i % 3 == 2 {
            let gr = u64::from(*gen_rx.borrow_and_update());
            sorted.request_refresh();
            wait_for_gen(&mut gen_rx, gr).await;
        }

        assert!(
            is_desc(&sorted),
            "sorted view decayed after tick {i}: {:?}",
            amounts(&sorted)
        );
    }
    Ok(())
}
