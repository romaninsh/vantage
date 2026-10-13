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

fn address(city: &str) -> CborValue {
    CborValue::Map(vec![(
        CborValue::Text("city".into()),
        CborValue::Text(city.into()),
    )])
}

/// People with a nested `address` map; person 1 lives in Riga. Returns the
/// facade narrowed to `address.city == "Riga"` and the master's shell.
async fn riga_people() -> Result<(Vista, MockShell)> {
    let shell = MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String"))
            .with_column(Column::new("address", "Map"))
            .with_id_column("id"),
    );
    let mut row = rec(&[("id", "1"), ("name", "Ada")]);
    row.insert("address".into(), address("Riga"));
    shell.set_record("1", row);
    let dio = warmed_dio(Vista::new("people", shell.clone())).await?;
    let mut riga = dio.vista();
    riga.add_condition("address.city", FilterOp::Eq, "Riga")?;
    Ok((riga, shell))
}

/// A set narrowed on a dotted path (`address.city`) has no flat cell to fill:
/// a write must hold the value at the path itself, and nothing writes the
/// literal key `"address.city"`.
#[tokio::test]
async fn dotted_path_narrowing_fills_no_literal_key() -> Result<()> {
    let (riga, shell) = riga_people().await?;

    let no_address = riga.insert_value("2", &rec(&[("id", "2")])).await;
    assert!(no_address.unwrap_err().is_conflict(), "nothing to fill");
    let mut elsewhere = rec(&[("id", "3")]);
    elsewhere.insert("address".into(), address("Oslo"));
    assert!(
        riga.insert_value("3", &elsewhere)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let mut moved = Record::new();
    moved.insert("address".into(), address("Oslo"));
    assert!(
        riga.patch_value("1", &moved)
            .await
            .unwrap_err()
            .is_conflict()
    );

    let mut inside = rec(&[("id", "4")]);
    inside.insert("address".into(), address("Riga"));
    riga.insert_value("4", &inside).await?;
    settle_until(&shell, |s| s.get_record("4").is_some()).await;
    let stored = shell.get_record("4").expect("row 4 written");
    assert!(stored.get("address.city").is_none(), "{stored:?}");
    assert_eq!(stored.get("address"), Some(&address("Riga")));
    Ok(())
}

/// A literal `"address.city"` key in a written record can't stand in for the
/// nested value: the set check walks the path.
#[tokio::test]
async fn dotted_path_set_ignores_a_literal_dotted_key() -> Result<()> {
    let (riga, _shell) = riga_people().await?;
    let mut disguised = rec(&[("id", "5"), ("address.city", "Riga")]);
    disguised.insert("address".into(), address("Oslo"));
    assert!(
        riga.insert_value("5", &disguised)
            .await
            .unwrap_err()
            .is_conflict()
    );
    let mut disguised_patch = rec(&[("address.city", "Riga")]);
    disguised_patch.insert("address".into(), address("Oslo"));
    assert!(
        riga.patch_value("1", &disguised_patch)
            .await
            .unwrap_err()
            .is_conflict()
    );
    Ok(())
}

/// A patch that keeps the row in the dotted set goes through.
#[tokio::test]
async fn dotted_path_set_takes_an_in_set_patch() -> Result<()> {
    let (riga, shell) = riga_people().await?;
    riga.patch_value("1", &rec(&[("name", "Ada L.")])).await?;
    let renamed = |s: &MockShell| {
        s.get_record("1")
            .is_some_and(|r| r.get("name") == Some(&CborValue::Text("Ada L.".into())))
    };
    settle_until(&shell, renamed).await;
    let stored = shell.get_record("1").expect("row 1");
    assert_eq!(stored.get("name"), Some(&CborValue::Text("Ada L.".into())));
    assert_eq!(stored.get("address"), Some(&address("Riga")));
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
