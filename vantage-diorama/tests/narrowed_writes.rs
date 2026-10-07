//! A narrowed Dio facade writes only inside its own set.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{Dio, Lens};
use vantage_types::Record;
use vantage_vista::reference::Reference;
use vantage_vista::source::TableShell;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), CborValue::Text((*v).to_string()));
    }
    r
}

/// Teams 1 red, 2 red, 3 blue; cache warmed from the master. Returns the
/// master's shell so a test can read the store directly.
async fn teams() -> Result<(Dio, MockShell)> {
    let shell = team_shell();
    let dio = warmed_dio(Box::new(shell.clone())).await?;
    Ok((dio, shell))
}

fn team_shell() -> MockShell {
    MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("team", "String"))
                .with_id_column("id"),
        )
        .with_record("1", rec(&[("id", "1"), ("team", "red")]))
        .with_record("2", rec(&[("id", "2"), ("team", "red")]))
        .with_record("3", rec(&[("id", "3"), ("team", "blue")]))
}

async fn warmed_dio(master: Box<dyn TableShell>) -> Result<Dio> {
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));
    let dio = lens.make_dio(Vista::new("teams", master)).await?;
    for (id, row) in dio.master().list_values().await? {
        dio.cache().insert_value(&id, &row).await?;
    }
    Ok(dio)
}

/// A MockShell that refuses every listing once `refuse` is set — any
/// read of the whole set then fails, while point reads and writes pass
/// through.
struct ListRefusingShell {
    inner: MockShell,
    refuse: Arc<AtomicBool>,
}

#[async_trait]
#[allow(clippy::ptr_arg)]
impl TableShell for ListRefusingShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        self.inner.columns()
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        self.inner.references()
    }
    fn id_column(&self) -> Option<&str> {
        self.inner.id_column()
    }
    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(error!("listing refused"));
        }
        self.inner.list_vista_values(vista).await
    }
    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        self.inner.get_vista_value(vista, id).await
    }
    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.inner.get_vista_some_value(vista).await
    }
    async fn patch_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.inner.patch_vista_value(vista, id, partial).await
    }
    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        self.inner.delete_vista_value(vista, id).await
    }
    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.inner.add_eq_condition(field, value)
    }
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(ListRefusingShell {
            inner: self.inner.clone(),
            refuse: self.refuse.clone(),
        }))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        self.inner.capabilities()
    }
    fn preview_query(&self, vista: &Vista) -> serde_json::Value {
        self.inner.preview_query(vista)
    }
}

/// Writes are queued; wait until the store stops shrinking.
async fn settle(shell: &MockShell, expected_len: usize) {
    for _ in 0..200 {
        if shell.len() <= expected_len {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn narrowed_delete_all_keeps_rows_outside_the_set() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.vista();
    red.add_condition_eq("team", CborValue::Text("red".into()))?;

    red.delete_all().await?;
    settle(&shell, 1).await;

    assert_eq!(
        shell.record_ids(),
        vec!["3".to_string()],
        "only the red rows go"
    );
    Ok(())
}

/// Poll `pred(shell)` every 10 ms for up to 2 s.
async fn settle_until(shell: &MockShell, pred: impl Fn(&MockShell) -> bool) {
    for _ in 0..200 {
        if pred(shell) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn narrowed_facade_writes_stay_in_the_set() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.vista();
    red.add_condition_eq("team", CborValue::Text("red".into()))?;

    red.delete("3").await?; // blue: outside the set
    assert!(
        red.patch_value("3", &rec(&[("team", "red")]))
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        red.insert_value("3", &rec(&[("id", "3")]))
            .await
            .unwrap_err()
            .is_conflict()
    );
    red.insert_value("4", &rec(&[("id", "4")])).await?; // team filled
    red.delete("ghost").await?; // idempotent
    settle_until(&shell, |s| s.get_record("4").is_some()).await;

    assert!(shell.get_record("3").is_some(), "blue row untouched");
    assert_eq!(
        shell.get_record("4").unwrap()["team"],
        CborValue::Text("red".into())
    );
    Ok(())
}

/// A narrowed by-id write checks membership with a point read of the one id,
/// never by listing the set: with every master listing refused, writes
/// still resolve — inside the set or outside it.
#[tokio::test]
async fn narrowed_writes_read_one_row_not_the_set() -> Result<()> {
    let refuse = Arc::new(AtomicBool::new(false));
    let dio = warmed_dio(Box::new(ListRefusingShell {
        inner: team_shell(),
        refuse: refuse.clone(),
    }))
    .await?;
    let mut red = dio.vista();
    red.add_condition_eq("team", CborValue::Text("red".into()))?;
    refuse.store(true, Ordering::SeqCst);

    red.patch_value("1", &rec(&[("team", "red")])).await?;
    red.delete("2").await?;
    red.delete("3").await?; // blue, cached: outside, nothing queued
    assert!(
        red.patch_value("3", &rec(&[("team", "red")]))
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        red.insert_value("3", &rec(&[("id", "3")]))
            .await
            .unwrap_err()
            .is_conflict()
    );
    Ok(())
}

/// Rows the cache has not seen are looked up in the master, then judged
/// against the narrowing like cached ones.
#[tokio::test]
async fn narrowed_writes_find_rows_only_the_master_holds() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.vista();
    red.add_condition_eq("team", CborValue::Text("red".into()))?;
    shell.set_record("5", rec(&[("id", "5"), ("team", "red")]));
    shell.set_record("6", rec(&[("id", "6"), ("team", "blue")]));

    red.patch_value("5", &rec(&[("team", "red")])).await?;
    assert!(
        red.patch_value("6", &rec(&[("team", "red")]))
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        red.insert_value("6", &rec(&[("id", "6")]))
            .await
            .unwrap_err()
            .is_conflict()
    );
    red.delete("6").await?; // outside: nothing queued
    red.delete("5").await?;
    settle_until(&shell, |s| s.get_record("5").is_none()).await;

    assert!(shell.get_record("5").is_none(), "red row deleted");
    assert!(shell.get_record("6").is_some(), "blue row untouched");
    Ok(())
}

/// The master moved row 1 out of the set; the cache still shows it red. A
/// narrowed write judges membership from the master, so it leaves row 1 alone.
#[tokio::test]
async fn narrowed_write_ignores_a_stale_cache_row() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.vista();
    red.add_condition_eq("team", CborValue::Text("red".into()))?;
    shell.set_field("1", "team", CborValue::Text("blue".into()));

    assert!(
        red.patch_value("1", &rec(&[("team", "red")]))
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        red.insert_value("1", &rec(&[("id", "1")]))
            .await
            .unwrap_err()
            .is_conflict()
    );
    red.delete("1").await?;
    red.delete("2").await?;
    settle_until(&shell, |s| s.get_record("2").is_none()).await;

    assert_eq!(
        shell.get_record("1").unwrap()["team"],
        CborValue::Text("blue".into())
    );
    Ok(())
}

#[tokio::test]
async fn retried_insert_flash_is_a_no_op() -> Result<()> {
    let (dio, _shell) = teams().await?;
    let mut events = dio.subscribe_events();
    dio.flash_insert("9".to_string(), rec(&[("id", "9"), ("team", "red")]))
        .await?;
    dio.flash_insert("9".to_string(), rec(&[("id", "9"), ("team", "red")]))
        .await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    while let Ok(e) = events.try_recv() {
        assert!(
            !matches!(e, vantage_diorama::DioEvent::WriteFailed { .. }),
            "{e:?}"
        );
    }
    Ok(())
}
