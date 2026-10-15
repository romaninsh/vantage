//! Sugar: display-only columns one scenery reads, never stored.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Dio, Sugar, SugarFn, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, SortDirection, Vista, VistaMetadata, mocks::MockShell};

mod support;
use support::chunk::{Backend, master as chunk_master, paged_lens, redb_lens};
use support::copy_master;

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 30))
        .with_record("b", record("beta", 10))
        .with_record("c", record("gamma", 20));
    Vista::new("items", shell)
}

/// An eager Dio over [`seeded_master`], cached in redb under `cache_path`.
async fn seeded_dio(cache_path: std::path::PathBuf) -> Result<Dio> {
    redb_lens(cache_path, copy_master())
        .dio(seeded_master())
        .build()
        .await
}

async fn wait_for_gen(
    rx: &mut tokio::sync::watch::Receiver<vantage_diorama::Generation>,
    current: u64,
) -> u64 {
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

/// `twice` = price × 2.
fn doubling() -> Sugar {
    let apply: SugarFn = Arc::new(|rows| {
        Box::pin(async move {
            rows.into_iter()
                .map(|r| {
                    let price = r
                        .get("price")
                        .and_then(|v| v.as_integer())
                        .map(i128::from)
                        .unwrap_or(0);
                    let mut out = Record::new();
                    out.insert(
                        "twice".to_string(),
                        CborValue::Integer(((price * 2) as i64).into()),
                    );
                    Ok(out)
                })
                .collect()
        })
    });
    Sugar::new(apply, ["twice"])
}

fn names(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .map(|i| match scenery.row(i).unwrap().record.get("name") {
            Some(CborValue::Text(s)) => s.clone(),
            other => panic!("name: {other:?}"),
        })
        .collect()
}

fn twice_of(scenery: &Arc<dyn TableScenery>, name: &str) -> Option<CborValue> {
    (0..scenery.row_count())
        .map(|i| scenery.row(i).unwrap())
        .find(|r| r.record.get("name") == Some(&cbor_text(name)))
        .and_then(|r| r.record.get("twice").cloned())
}

#[tokio::test]
async fn a_sugared_view_sorts_by_an_output_and_a_plain_view_sees_none() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;

    let sweet = dio
        .table_scenery()
        .sugar(doubling())
        .order("twice", SortDirection::Descending)
        .open()
        .await?;
    let plain = dio.table_scenery().open().await?;

    assert_eq!(names(&sweet), ["alpha", "gamma", "beta"]);
    assert_eq!(
        twice_of(&sweet, "beta"),
        Some(CborValue::Integer(20.into()))
    );
    assert_eq!(twice_of(&plain, "beta"), None);
    Ok(())
}

#[tokio::test]
async fn outputs_never_reach_the_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;

    let sweet = dio.table_scenery().sugar(doubling()).open().await?;
    assert_eq!(sweet.row_count(), 3);
    let cached = vantage_dataset::prelude::ReadableValueSet::get_value(&dio.cached(), "a")
        .await?
        .unwrap();
    assert!(cached.get("twice").is_none());
    Ok(())
}

#[tokio::test]
async fn a_changed_row_is_recomputed() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;

    let sweet = dio.table_scenery().sugar(doubling()).open().await?;
    assert_eq!(
        twice_of(&sweet, "alpha"),
        Some(CborValue::Integer(60.into()))
    );
    let mut gen_rx = sweet.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    dio.apply_change(vantage_diorama::ChangeEvent::Updated {
        id: "a".into(),
        new: Some(record("alpha", 50)),
    })
    .await?;
    wait_for_gen(&mut gen_rx, initial).await;

    assert_eq!(
        twice_of(&sweet, "alpha"),
        Some(CborValue::Integer(100.into()))
    );
    Ok(())
}

#[tokio::test]
async fn a_record_scenery_reads_the_sugared_row() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;

    let sweet = dio.record_scenery("b").sugar(doubling()).open().await?;
    let plain = dio.record_scenery("b").open().await?;
    let twice = |r: Option<Arc<vantage_diorama::EnrichedRecord>>| {
        r.and_then(|r| r.record.get("twice").cloned())
    };
    assert_eq!(twice(sweet.record()), Some(CborValue::Integer(20.into())));
    assert_eq!(twice(plain.record()), None);
    Ok(())
}

/// The handed row shows at once, then gains the sugar columns from the
/// cache's copy.
#[tokio::test]
async fn a_seeded_sugared_scenery_gains_the_sugar_columns() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;

    let sweet = dio
        .record_scenery("b")
        .sugar(doubling())
        .seed(record("beta", 10))
        .open()
        .await?;
    let name = sweet.record().and_then(|r| r.record.get("name").cloned());
    assert_eq!(
        name,
        Some(cbor_text("beta")),
        "the seeded row shows at once"
    );

    wait_for_gen(&mut sweet.subscribe(), 0).await;
    let twice = sweet.record().and_then(|r| r.record.get("twice").cloned());
    assert_eq!(twice, Some(CborValue::Integer(20.into())));
    Ok(())
}

/// [`doubling`], also emitting a column it never declared.
fn doubling_and_more() -> Sugar {
    let apply: SugarFn = Arc::new(|rows| {
        Box::pin(async move {
            rows.into_iter()
                .map(|_| {
                    let mut out = Record::new();
                    out.insert("twice".to_string(), CborValue::Integer(1.into()));
                    out.insert("extra".to_string(), cbor_text("undeclared"));
                    Ok(out)
                })
                .collect()
        })
    });
    Sugar::new(apply, ["twice"])
}

#[tokio::test]
async fn an_undeclared_output_is_dropped() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;

    let sweet = dio
        .table_scenery()
        .sugar(doubling_and_more())
        .open()
        .await?;
    let row = sweet.row(0).expect("a row");
    assert!(row.record.get("twice").is_some());
    assert_eq!(row.record.get("extra"), None);
    Ok(())
}

/// [`doubling`], counting the rows it is handed, and failing every row the
/// first time it runs.
fn counted(calls: Arc<AtomicUsize>, fail_first: bool) -> Sugar {
    let runs = Arc::new(AtomicUsize::new(0));
    let apply: SugarFn = Arc::new(move |rows| {
        let calls = calls.clone();
        let first = runs.fetch_add(1, Ordering::SeqCst) == 0;
        Box::pin(async move {
            calls.fetch_add(rows.len(), Ordering::SeqCst);
            rows.into_iter()
                .map(|r| {
                    if fail_first && first {
                        return Err(vantage_core::error!("sugar script failed"));
                    }
                    let price = r
                        .get("price")
                        .and_then(|v| v.as_integer())
                        .map(i128::from)
                        .unwrap_or(0);
                    let mut out = Record::new();
                    out.insert(
                        "twice".to_string(),
                        CborValue::Integer(((price * 2) as i64).into()),
                    );
                    Ok(out)
                })
                .collect()
        })
    });
    Sugar::new(apply, ["twice"])
}

/// Re-order `sweet` and wait for the re-sorted rows.
async fn resort(sweet: &Arc<dyn TableScenery>, dir: SortDirection) {
    let mut gen_rx = sweet.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());
    sweet.set_order(Some(("name".to_string(), dir)));
    wait_for_gen(&mut gen_rx, initial).await;
}

#[tokio::test]
async fn a_re_sort_serves_unchanged_rows_without_running_the_sugar_again() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;
    let calls = Arc::new(AtomicUsize::new(0));

    let sweet = dio
        .table_scenery()
        .sugar(counted(calls.clone(), false))
        .open()
        .await?;
    assert_eq!(
        twice_of(&sweet, "beta"),
        Some(CborValue::Integer(20.into()))
    );
    let first = calls.load(Ordering::SeqCst);
    assert_eq!(first, 3, "each row once");

    resort(&sweet, SortDirection::Descending).await;
    assert_eq!(names(&sweet), ["gamma", "beta", "alpha"]);
    assert_eq!(
        twice_of(&sweet, "beta"),
        Some(CborValue::Integer(20.into()))
    );
    assert_eq!(calls.load(Ordering::SeqCst), first, "no row ran again");
    Ok(())
}

#[tokio::test]
async fn a_failed_row_reads_without_outputs_and_is_tried_again() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(tmp.path().join("cache.redb")).await?;
    let calls = Arc::new(AtomicUsize::new(0));

    let sweet = dio
        .table_scenery()
        .sugar(counted(calls.clone(), true))
        .open()
        .await?;
    assert_eq!(sweet.row_count(), 3, "a failed sugar keeps the rows");
    assert_eq!(twice_of(&sweet, "alpha"), None);

    resort(&sweet, SortDirection::Ascending).await;
    assert_eq!(
        twice_of(&sweet, "alpha"),
        Some(CborValue::Integer(60.into()))
    );
    Ok(())
}

/// A sugared sheet that loads a missing row from the master shows the sugar
/// columns, and caches the row without them.
#[tokio::test]
async fn a_sugared_load_on_miss_caches_the_row_plain() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 30));
    let dio = redb_lens(tmp.path().join("cache.redb"), copy_master())
        .dio(Vista::new("items", shell.clone()))
        .build()
        .await?;
    shell.set_record("d", record("delta", 7));

    let sheet = dio
        .record_scenery("d")
        .sugar(doubling())
        .load_on_miss()
        .open()
        .await?;
    let twice = || sheet.record().and_then(|r| r.record.get("twice").cloned());
    assert!(
        support::eventually(Duration::from_secs(5), || twice().is_some()).await,
        "the sheet never showed the sugar column: {:?}",
        sheet.record()
    );
    assert_eq!(twice(), Some(CborValue::Integer(14.into())));

    let cached = vantage_dataset::prelude::ReadableValueSet::get_value(&dio.cached(), "d")
        .await?
        .expect("the loaded row is cached");
    assert_eq!(cached.get("name"), Some(&cbor_text("delta")));
    assert!(cached.get("twice").is_none(), "{cached:?}");
    Ok(())
}

/// A sugared paged view over two rows, its first window loaded.
async fn paged_sweet(tmp: &TempDir) -> Result<(Dio, Arc<dyn TableScenery>)> {
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("a".into(), record("alpha", 30)),
        ("b".into(), record("beta", 10)),
    ]));
    let dio = paged_lens(tmp.path().join("cache.redb"), backend)
        .dio(chunk_master(&[("name", "String"), ("price", "i64")]))
        .build()
        .await?;
    let sweet = dio.table_scenery().sugar(doubling()).open().await?;
    let mut gen_rx = sweet.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());
    sweet.set_viewport(0..2, 0..2);
    wait_for_gen(&mut gen_rx, initial).await;
    assert!(
        support::eventually(Duration::from_secs(5), || sweet.row(1).is_some()).await,
        "the window never loaded"
    );
    Ok((dio, sweet))
}

/// Rows a paged loader writes through a sugared view reach the cache plain.
#[tokio::test]
async fn rows_loaded_through_a_sugared_view_reach_the_cache_without_outputs() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let (dio, _sweet) = paged_sweet(&tmp).await?;
    let cached = vantage_dataset::prelude::ReadableValueSet::get_value(&dio.cached(), "a")
        .await?
        .expect("the loaded row is cached");
    assert_eq!(cached.get("name"), Some(&cbor_text("alpha")));
    assert!(cached.get("twice").is_none(), "{cached:?}");
    Ok(())
}

/// `sugar(..)` reads rows with the display-only columns merged in, whatever
/// loads them.
#[tokio::test]
#[ignore = "possible bug: a paged sugared view shows the loaded window without its sugar columns"]
async fn a_paged_sugared_view_shows_the_sugar_columns() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let (_dio, sweet) = paged_sweet(&tmp).await?;
    assert!(
        support::eventually(Duration::from_secs(5), || twice_of(&sweet, "alpha")
            .is_some())
        .await,
        "the view never showed the sugar column: {:?}",
        sweet.row(0)
    );
    assert_eq!(
        twice_of(&sweet, "beta"),
        Some(CborValue::Integer(20.into()))
    );
    Ok(())
}
