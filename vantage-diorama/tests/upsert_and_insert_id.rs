//! `Vista::upsert_value` on a Dio facade upserts through the write-through
//! (default path upserts on the master, see `dio/worker.rs`); an insert
//! without an id (`Vista::insert_return_id_value`, `ChangeFlash::insert_new`)
//! reports the id the backend made.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet};
use vantage_diorama::{Cache, ChangeFlash, Dio, DioEvent, FlashOutcome, Lens};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, TableShell, Vista, VistaMetadata, forward_table_shell, mocks::MockShell,
};

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
        .with_capabilities(
            [Capability::Insert, Capability::Update, Capability::Delete]
                .into_iter()
                .collect(),
        )
        .with_metadata(metadata())
}

/// A shell whose native replace requires the row, the way memory's does.
#[derive(Clone)]
struct StrictReplace {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(StrictReplace, inner, {
    async fn replace(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        if self.inner.get(id).await?.is_none() {
            return Err(error!("Row not found", id = id).mark_not_found());
        }
        self.inner.replace(id, record).await
    }
});

async fn dio_over(master: Vista) -> Result<Dio> {
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    lens.dio(master).build().await
}

/// A Dio over an empty in-memory master, cache empty until reads/writes
/// populate it.
async fn dio_over_memory() -> Result<Dio> {
    dio_over(Vista::new("items", empty_shell())).await
}

/// A Dio over an empty master whose replace requires the row.
async fn dio_over_strict_replace() -> Result<Dio> {
    dio_over(Vista::new("items", empty_shell()).map_shell(|inner| StrictReplace { inner })).await
}

/// The master's `name` for row `id`.
async fn master_name(dio: &Dio, id: &str) -> Result<Option<CborValue>> {
    let row = dio.master().get_value(id).await?;
    Ok(row.and_then(|r| r.get("name").cloned()))
}

/// A facade write returns once it is confirmed, so the master holds it by
/// then.
#[tokio::test]
async fn upsert_inserts_then_replaces() -> Result<()> {
    let dio = dio_over_memory().await?;
    let vista = dio.cached();

    vista.upsert_value("k", &rec(&[("name", "a")])).await?;
    assert_eq!(master_name(&dio, "k").await?, Some(text("a")));

    vista.upsert_value("k", &rec(&[("name", "b")])).await?;
    assert_eq!(master_name(&dio, "k").await?, Some(text("b")));
    Ok(())
}

#[tokio::test]
async fn dio_cached_insert_without_id_returns_the_new_id() -> Result<()> {
    let dio = dio_over_memory().await?;
    let vista = dio.cached();

    let id = vista.insert_return_id_value(&rec(&[("name", "a")])).await?;
    assert!(vista.get_value(&id).await?.is_some());
    Ok(())
}

/// `insert_new` has no id to stage: the master makes one, the outcome
/// carries it, and the row is in the cache under it.
#[tokio::test]
async fn insert_new_reports_the_id_the_master_made() -> Result<()> {
    let dio = dio_over_memory().await?;
    let outcome = dio
        .flash(ChangeFlash::insert_new(rec(&[("name", "a")])))
        .await?;
    let id = outcome.id.expect("the new row's id");
    let stored = outcome.record.expect("the stored row");
    assert_eq!(stored.get("id"), Some(&text(&id)));
    assert_eq!(master_name(&dio, &id).await?, Some(text("a")));
    let cached = dio.cached().get_value(&id).await?.expect("cached");
    assert_eq!(cached.get("name"), Some(&text("a")));
    Ok(())
}

/// A Dio whose Lens writer keeps every row under an id of its own, `w-1`,
/// reporting only that id. Each flash's id lands in `seen`.
async fn writer_dio(seen: Arc<Mutex<Vec<Option<String>>>>) -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(move |_cx, flash| {
            seen.lock().unwrap().push(flash.id().map(str::to_string));
            async move { Ok(FlashOutcome::applied().with_id("w-1")) }
        })
        .build()?;
    lens.dio(Vista::new("items", empty_shell())).build().await
}

/// The id-less insert reaches the writer rather than bypassing it.
#[tokio::test]
async fn insert_new_goes_through_the_writer() -> Result<()> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let dio = writer_dio(seen.clone()).await?;
    let outcome = dio
        .flash(ChangeFlash::insert_new(rec(&[("name", "a")])))
        .await?;
    assert_eq!(outcome.id.as_deref(), Some("w-1"));
    assert_eq!(*seen.lock().unwrap(), [None]);
    assert!(dio.cached().get_value("w-1").await?.is_some());
    Ok(())
}

/// A writer that reports only the id it settled a row under moves the
/// staged row onto that id.
#[tokio::test]
async fn a_writer_reporting_only_an_id_rekeys_the_staged_row() -> Result<()> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let dio = writer_dio(seen.clone()).await?;
    let mut bus = dio.subscribe_events();
    let outcome = dio
        .flash(ChangeFlash::insert(
            "local",
            rec(&[("id", "local"), ("name", "a")]),
        ))
        .await?;
    assert_eq!(outcome.id.as_deref(), Some("w-1"));
    assert_eq!(*seen.lock().unwrap(), [Some("local".to_string())]);

    let facade = dio.cached();
    assert!(
        facade.get_value("local").await?.is_none(),
        "the minted key retires"
    );
    let moved = facade.get_value("w-1").await?.expect("re-keyed");
    assert_eq!(moved.get("id"), Some(&text("w-1")));
    assert_eq!(moved.get("name"), Some(&text("a")));
    let events: Vec<String> = std::iter::from_fn(|| bus.try_recv().ok())
        .filter_map(|e| match e {
            DioEvent::RecordRemoved { id } => Some(format!("removed {id}")),
            DioEvent::RecordInserted { id } => Some(format!("inserted {id}")),
            _ => None,
        })
        .collect();
    assert_eq!(events, ["removed local", "inserted w-1"]);
    Ok(())
}

/// A `Replace` flash stays strict even for a Dio-routed write: on a shell
/// whose native replace requires the row (memory's shape), a replace of a
/// missing row fails rather than silently creating it. `Dio::flash` waits
/// for the write to settle, so the failure surfaces directly.
#[tokio::test]
async fn replace_flash_of_a_missing_row_still_fails() -> Result<()> {
    let dio = dio_over_strict_replace().await?;

    let err = dio
        .flash(ChangeFlash::replace("missing", rec(&[("name", "a")])))
        .await
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");
    Ok(())
}

/// The same missing row accepts an `Upsert` flash instead of failing.
#[tokio::test]
async fn upsert_flash_of_a_missing_row_creates_it() -> Result<()> {
    let dio = dio_over_strict_replace().await?;

    dio.flash(ChangeFlash::upsert("missing", rec(&[("name", "a")])))
        .await?;
    let row = dio.master().get_value("missing").await?;
    assert_eq!(row.and_then(|r| r.get("name").cloned()), Some(text("a")));
    Ok(())
}
