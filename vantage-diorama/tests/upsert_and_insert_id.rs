//! `Vista::upsert_value` on a Dio facade upserts through the write-through
//! (default path upserts on the master, see `dio/worker.rs`); `Vista::
//! insert_return_id_value` inserts without an id and reports the one the
//! master assigned.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet};
use vantage_diorama::{ChangeFlash, Dio, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), text(v));
    }
    r
}

fn metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_id_column("id")
}

fn empty_shell() -> MockShell {
    MockShell::new()
        .with_capabilities(VistaCapabilities {
            can_count: true,
            can_insert: true,
            can_update: true,
            can_delete: true,
            ..VistaCapabilities::default()
        })
        .with_metadata(metadata())
}

async fn dio_over(shell: MockShell) -> Result<Dio> {
    let master = Vista::new("items", Box::new(shell));
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));
    lens.make_dio(master).await
}

/// A Dio over an empty in-memory master, cache empty until reads/writes
/// populate it.
async fn dio_over_memory() -> Result<Dio> {
    dio_over(empty_shell()).await
}

/// `upsert_value` enqueues onto the fire-and-forget write queue like every
/// other `TableShell` write (no `on_flash` route here, so the default write
/// path touches only the master, not the cache) — poll the master instead
/// of asserting right after the await.
async fn wait_for_name(dio: &Dio, id: &str, expected: &str) -> Record<CborValue> {
    for _ in 0..50 {
        if let Some(row) = dio.master().get_value(id).await.unwrap()
            && row.get("name") == Some(&text(expected))
        {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("row {id} never settled to name={expected}");
}

#[tokio::test]
async fn upsert_inserts_then_replaces() -> Result<()> {
    let dio = dio_over_memory().await?;
    let vista = dio.vista();

    vista.upsert_value("k", &rec(&[("name", "a")])).await?;
    wait_for_name(&dio, "k", "a").await;

    vista.upsert_value("k", &rec(&[("name", "b")])).await?;
    wait_for_name(&dio, "k", "b").await;
    Ok(())
}

#[tokio::test]
async fn dio_vista_insert_without_id_returns_the_new_id() -> Result<()> {
    let dio = dio_over_memory().await?;
    let vista = dio.vista();

    let id = vista.insert_return_id_value(&rec(&[("name", "a")])).await?;
    assert!(vista.get_value(&id).await?.is_some());
    Ok(())
}

/// `default_write` wraps the driver's error in a "write failed" context
/// (vantage-diorama/src/dio/worker.rs), so the classification only
/// survives on the wrapped source, not on the top-level error.
fn caused_by_not_found(err: &vantage_core::VantageError) -> bool {
    err.is_not_found()
        || std::error::Error::source(err)
            .and_then(|s| s.downcast_ref::<vantage_core::VantageError>())
            .is_some_and(caused_by_not_found)
}

/// A `Replace` flash stays strict even for a Dio-routed write: on a shell
/// whose native replace requires the row (memory's shape), a replace of a
/// missing row fails rather than silently creating it. `Dio::flash` runs
/// the write-through inline, so the failure surfaces directly instead of
/// only on the event bus.
#[tokio::test]
async fn replace_flash_of_a_missing_row_still_fails() -> Result<()> {
    let dio = dio_over(empty_shell().with_replace_requires_existing()).await?;

    let err = dio
        .flash(ChangeFlash::replace("missing", rec(&[("name", "a")])))
        .await
        .unwrap_err();
    assert!(caused_by_not_found(&err), "{err}");
    Ok(())
}

/// The same missing row accepts an `Upsert` flash instead of failing.
#[tokio::test]
async fn upsert_flash_of_a_missing_row_creates_it() -> Result<()> {
    let dio = dio_over(empty_shell().with_replace_requires_existing()).await?;

    dio.flash(ChangeFlash::upsert("missing", rec(&[("name", "a")])))
        .await?;
    let row = dio.master().get_value("missing").await?;
    assert_eq!(row.and_then(|r| r.get("name").cloned()), Some(text("a")));
    Ok(())
}
