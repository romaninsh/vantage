//! Stage 5: TableScenery — reactive ordered-rows view.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Cache, CacheBackend, ChangeEvent, Lens, Loader, RedbCache, TableScenery};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, FilterOp, SortDirection, Vista, VistaMetadata,
    VistaQuery, mocks::MockShell,
};

mod support;
use support::chunk::redb_lens;
use support::{copy_master, eventually, memory_lens, paged_dio, paged_or_eager_dio};

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

/// A seeded row: `record` plus its `id`, a `status` and an integer `n`.
fn seeded(id: &str, name: &str, price: i64, status: &str, n: i64) -> Record<CborValue> {
    let mut r = record(name, price);
    r.insert("id".to_string(), cbor_text(id));
    r.insert("status".to_string(), cbor_text(status));
    r.insert("n".to_string(), CborValue::Integer(n.into()));
    r
}

/// Build a MockShell-backed master pre-seeded with three rows.
fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Orderable))
        .with_column(Column::new("price", "i64"))
        .with_column(Column::new("status", "String"))
        .with_column(Column::new("n", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", seeded("a", "alpha", 30, "open", 1))
        .with_record("b", seeded("b", "beta", 10, "closed", 5))
        .with_record("c", seeded("c", "gamma", 20, "open", 9));
    Vista::new("items", shell)
}

/// Ids of the rows `scenery` shows, in row order.
fn shown_ids(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|row| match row.record.get("id") {
            Some(CborValue::Text(id)) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Build a Lens whose eager start copies the master into the cache so
/// the Scenery has something to read.
async fn build_lens(cache_path: std::path::PathBuf) -> Result<Arc<Lens>> {
    Ok(redb_lens(cache_path, copy_master()))
}

/// Wait for the Scenery's generation to advance past `current`.
async fn wait_for_gen(
    rx: &mut tokio::sync::watch::Receiver<vantage_diorama::Generation>,
    current: u64,
) -> u64 {
    // Bounds a hang, not a speed — see `support/chunk.rs::wait_for_gen`.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > current {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.expect("watch channel closed");
        }
    })
    .await
    .expect("timed out waiting for generation bump")
}

#[tokio::test]
async fn scenery_loads_rows_from_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    assert_eq!(scenery.row_count(), 3);
    assert!(scenery.row(0).is_some());
    assert!(scenery.row(2).is_some());
    assert!(scenery.row(3).is_none());
    Ok(())
}

#[tokio::test]
async fn set_order_reorders_and_bumps_generation() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    scenery.set_order(Some(("name".into(), SortDirection::Descending)));
    wait_for_gen(&mut gen_rx, initial).await;

    let r0 = scenery.row(0).unwrap();
    let r2 = scenery.row(2).unwrap();
    assert_eq!(r0.record.get("name"), Some(&cbor_text("gamma")));
    assert_eq!(r2.record.get("name"), Some(&cbor_text("alpha")));
    Ok(())
}

#[tokio::test]
async fn a_builder_filter_narrows_the_view() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio
        .table_scenery()
        .filter(Condition::new("status", FilterOp::Eq, "open"))
        .open()
        .await?;
    let mut ids = shown_ids(&scenery);
    ids.sort();
    assert_eq!(ids, ["a", "c"]);
    Ok(())
}

#[tokio::test]
async fn set_filters_takes_any_operator() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;
    let scenery = dio.table_scenery().open().await?;
    let sorted_ids = || {
        let mut ids = shown_ids(&scenery);
        ids.sort();
        ids
    };

    scenery.set_filters(vec![Condition::new("n", FilterOp::Gt, 4)]);
    assert!(
        eventually(Duration::from_secs(5), || sorted_ids() == ["b", "c"]).await,
        "n > 4 never narrowed to b, c; shows {:?}",
        sorted_ids()
    );

    scenery.set_filters(vec![]);
    assert!(
        eventually(Duration::from_secs(5), || sorted_ids() == ["a", "b", "c"]).await,
        "clearing the filters never restored every row; shows {:?}",
        sorted_ids()
    );
    Ok(())
}

/// Every clause a view holds reaches the paged loader as one VistaQuery:
/// the builder filter, the runtime filters, the search and the order.
#[tokio::test]
async fn every_clause_reaches_the_paged_loader_in_one_query() -> Result<()> {
    let seen = Arc::new(Mutex::new(Vec::<VistaQuery>::new()));
    let log = seen.clone();
    let loader = Loader::paged(move |cx, range, query, sink| {
        let log = log.clone();
        async move {
            log.lock().unwrap().push(query.clone());
            let fetched = cx.fetch(range.clone(), &query).await?;
            for (i, (id, row)) in fetched.page.rows.into_iter().enumerate() {
                sink.push(range.start + i, id, row).await?;
            }
            Ok(())
        }
    });
    let dio = memory_lens(loader).dio(seeded_master()).build().await?;
    let view = dio
        .table_scenery()
        .filter(Condition::new("status", FilterOp::Eq, "open"))
        .order("name", SortDirection::Ascending)
        .open()
        .await?;
    view.set_filters(vec![Condition::new("n", FilterOp::Gt, 1)]);
    view.set_search(Some("a".into()));
    view.set_viewport(0..10, 0..0);
    let wanted = |q: &VistaQuery| {
        q.conditions.len() == 2 && q.search.as_deref() == Some("a") && q.order.is_some()
    };
    assert!(
        eventually(Duration::from_secs(5), || seen.lock().unwrap().iter().any(wanted)).await,
        "the loader never received the whole query; saw {:?}",
        seen.lock().unwrap()
    );
    Ok(())
}

#[tokio::test]
async fn external_notify_record_changed_triggers_reload() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend = Arc::new(RedbCache::open(tmp.path().join("cache.redb"))?);
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(copy_master())
        .build()?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // External system writes a new row into the cache, then publishes a
    // notify-only change.
    let table = backend.open_table("items").await?;
    table.insert_value("d", &record("delta", 5)).await?;
    dio.apply_change(ChangeEvent::Updated {
        id: "d".into(),
        new: None,
    })
    .await?;

    wait_for_gen(&mut gen_rx, initial).await;
    assert_eq!(scenery.row_count(), 4);
    Ok(())
}

#[tokio::test]
async fn patched_updates_visible_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // Live stream renamed "alpha" → "alphabet".
    dio.apply_change(ChangeEvent::Updated {
        id: "a".into(),
        new: Some(record("alphabet", 30)),
    })
    .await?;
    wait_for_gen(&mut gen_rx, initial).await;

    let updated = scenery
        .row(0)
        .or_else(|| scenery.row(1))
        .or_else(|| scenery.row(2))
        .expect("row present");
    // Find the renamed row; order may have shifted if sort were set.
    let mut found = None;
    for i in 0..scenery.row_count() {
        let r = scenery.row(i).unwrap();
        if r.record.get("name") == Some(&cbor_text("alphabet")) {
            found = Some(r);
            break;
        }
    }
    let r = found.expect("found renamed row");
    let _ = updated;
    assert_eq!(r.record.get("name"), Some(&cbor_text("alphabet")));
    Ok(())
}

/// An applied `Deleted` change wipes the cache entry AND publishes the bus
/// event so a subscribed TableScenery's reseed actually drops the row. A
/// notify-only change leaves the cache untouched — sceneries would reload
/// the deleted row right back in.
#[tokio::test]
async fn removed_drops_row_from_visible_set() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());
    assert_eq!(scenery.row_count(), 3);

    dio.apply_change(ChangeEvent::Deleted { id: "b".into() })
        .await?;
    wait_for_gen(&mut gen_rx, initial).await;

    assert_eq!(scenery.row_count(), 2);
    let names: Vec<_> = (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|r| r.record.get("name").cloned())
        .collect();
    assert!(names.contains(&cbor_text("alpha")));
    assert!(!names.contains(&cbor_text("beta")));
    Ok(())
}

/// A `Deleted` change is idempotent — applying one for an absent id is a
/// no-op (still publishes the event so any reseed-style scenery re-syncs
/// harmlessly).
#[tokio::test]
async fn removed_is_idempotent_on_missing_id() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let _scenery = dio.table_scenery().open().await?;
    dio.apply_change(ChangeEvent::Deleted {
        id: "does-not-exist".into(),
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn scenery_outlives_dio_handle_drop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    assert_eq!(scenery.row_count(), 3);

    // Drop the external Dio handle. Scenery still has its rows in
    // memory (just no future reloads).
    drop(dio);
    assert_eq!(scenery.row_count(), 3);
    Ok(())
}

/// A limit bounds every read to the cap, and the viewport with it.
#[tokio::test]
async fn a_limit_bounds_the_view_and_its_viewport() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let capped = dio.table_scenery().limit(2).open().await?;
    assert_eq!(capped.row_count(), 2);
    assert!(capped.row(1).is_some());
    assert!(capped.row(2).is_none()); // exists beneath, hidden by the cap
    assert!(!capped.has_more()); // the capped view is complete at the cap

    capped.set_viewport(0..10, 1..10);
    assert_eq!(capped.view_stats().showing, Some(1..2));

    // An order still applies, and the cap then shows the top of the new one.
    let mut gen_rx = capped.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());
    capped.set_order(Some(("price".to_string(), SortDirection::Ascending)));
    wait_for_gen(&mut gen_rx, initial).await;
    let r0 = capped.row(0).unwrap();
    assert_eq!(r0.record.get("name"), Some(&cbor_text("beta"))); // price 10

    // A cap wider than the data doesn't invent rows.
    let wide = dio.table_scenery().limit(10).open().await?;
    assert_eq!(wide.row_count(), 3);
    Ok(())
}

/// A searchable master without `Search`.
fn unsearchable_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Searchable))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_capabilities([Capability::Window].into_iter().collect())
        .with_record("a", seeded("a", "alpha", 30, "open", 1));
    Vista::new("items", shell)
}

#[tokio::test]
async fn search_supported_follows_the_plan() -> Result<()> {
    let paged = paged_dio(unsearchable_master()).await;
    let view = paged.table_scenery().open().await?;
    assert!(!view.search_supported(), "a window can't be searched locally");

    let copied = paged_or_eager_dio(unsearchable_master()).await;
    let view = copied.table_scenery().open().await?;
    assert!(view.search_supported(), "the whole copy can be searched");

    let eager = support::eager_dio(unsearchable_master()).await;
    let view = eager.table_scenery().open().await?;
    assert!(view.search_supported());
    Ok(())
}
