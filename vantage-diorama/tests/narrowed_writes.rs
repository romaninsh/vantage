//! A narrowed Dio facade writes only inside its own set.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{Dio, Lens};
use vantage_types::Record;
use vantage_vista::{
    Column, FilterOp, Page, TableShell, Vista, VistaMetadata, forward_table_shell, mocks::MockShell,
};

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
    let dio = warmed_dio(Vista::new("teams", shell.clone())).await?;
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

async fn warmed_dio(master: Vista) -> Result<Dio> {
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));
    let dio = lens.make_dio(master).await?;
    for (id, row) in dio.master().list_values().await? {
        dio.cache().insert_value(&id, &row).await?;
    }
    Ok(dio)
}

/// A shell that refuses every read of the whole set once `refuse` is set,
/// while point reads and writes pass through.
#[derive(Clone)]
struct ListRefusingShell {
    inner: Box<dyn TableShell>,
    refuse: Arc<AtomicBool>,
}

impl ListRefusingShell {
    fn check(&self) -> Result<()> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(error!("listing refused"));
        }
        Ok(())
    }
}

forward_table_shell!(ListRefusingShell, inner, {
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.check()?;
        self.inner.list().await
    }
    async fn count(&self) -> Result<i64> {
        self.check()?;
        self.inner.count().await
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.check()?;
        self.inner.fetch_window(offset, limit).await
    }
});

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
    red.add_condition("team", FilterOp::Eq, "red")?;

    red.delete_all().await?;
    settle(&shell, 1).await;

    assert_eq!(
        shell.record_ids(),
        vec!["3".to_string()],
        "only the red rows go"
    );
    Ok(())
}

/// `id` is outside the red set: a patch into it is NotFound, an insert
/// under the id a Conflict.
async fn assert_outside_red(red: &Vista, id: &str) {
    assert!(
        red.patch_value(id, &rec(&[("team", "red")]))
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        red.insert_value(id, &rec(&[("id", id)]))
            .await
            .unwrap_err()
            .is_conflict()
    );
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
    red.add_condition("team", FilterOp::Eq, "red")?;

    red.delete("3").await?; // blue: outside the set
    assert_outside_red(&red, "3").await;
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
    let master = Vista::new("teams", team_shell()).map_shell(|inner| ListRefusingShell {
        inner,
        refuse: refuse.clone(),
    });
    let dio = warmed_dio(master).await?;
    let mut red = dio.vista();
    red.add_condition("team", FilterOp::Eq, "red")?;
    refuse.store(true, Ordering::SeqCst);

    red.patch_value("1", &rec(&[("team", "red")])).await?;
    red.delete("2").await?;
    red.delete("3").await?; // blue, cached: outside, nothing queued
    assert_outside_red(&red, "3").await;
    Ok(())
}

/// Rows the cache has not seen are looked up in the master, then judged
/// against the narrowing like cached ones.
#[tokio::test]
async fn narrowed_writes_find_rows_only_the_master_holds() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.vista();
    red.add_condition("team", FilterOp::Eq, "red")?;
    shell.set_record("5", rec(&[("id", "5"), ("team", "red")]));
    shell.set_record("6", rec(&[("id", "6"), ("team", "blue")]));

    red.patch_value("5", &rec(&[("team", "red")])).await?;
    assert_outside_red(&red, "6").await;
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
    red.add_condition("team", FilterOp::Eq, "red")?;
    shell.set_field("1", "team", CborValue::Text("blue".into()));

    assert_outside_red(&red, "1").await;
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
