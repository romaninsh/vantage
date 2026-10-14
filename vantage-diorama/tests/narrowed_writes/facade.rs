//! A facade narrowed by a condition: deletes, patches and inserts stay in
//! its set, judged against the master's current rows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::WritableValueSet;
use vantage_diorama::ChangeFlash;
use vantage_vista::{FilterOp, Vista};

use crate::support::record;
use crate::{ListRefusingShell, assert_outside_red, residual_shell, team_shell, teams, warmed_dio};

#[tokio::test]
async fn narrowed_delete_all_keeps_rows_outside_the_set() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;

    red.delete_all().await?;

    assert_eq!(
        shell.record_ids(),
        vec!["3".to_string()],
        "only the red rows go"
    );
    Ok(())
}

#[tokio::test]
async fn narrowed_facade_writes_stay_in_the_set() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;

    red.delete("3").await?; // blue: outside the set
    assert_outside_red(&red, "3").await;
    red.insert_value("4", &record(&[("id", "4")])).await?; // team filled
    red.delete("ghost").await?; // idempotent
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
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;
    refuse.store(true, Ordering::SeqCst);

    red.patch_value("1", &record(&[("team", "red")])).await?;
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
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;
    shell.set_record("5", record(&[("id", "5"), ("team", "red")]));
    shell.set_record("6", record(&[("id", "6"), ("team", "blue")]));

    red.patch_value("5", &record(&[("team", "red")])).await?;
    assert_outside_red(&red, "6").await;
    red.delete("6").await?; // outside: nothing queued
    red.delete("5").await?;

    assert!(shell.get_record("5").is_none(), "red row deleted");
    assert!(shell.get_record("6").is_some(), "blue row untouched");
    Ok(())
}

/// The master moved row 1 out of the set; the cache still shows it red. A
/// narrowed write judges membership from the master, so it leaves row 1 alone.
#[tokio::test]
async fn narrowed_write_ignores_a_stale_cache_row() -> Result<()> {
    let (dio, shell) = teams().await?;
    let mut red = dio.cached();
    red.add_condition("team", FilterOp::Eq, "red")?;
    shell.set_field("1", "team", CborValue::Text("blue".into()));

    assert_outside_red(&red, "1").await;
    red.delete("1").await?;
    red.delete("2").await?;

    assert_eq!(
        shell.get_record("1").unwrap()["team"],
        CborValue::Text("blue".into())
    );
    Ok(())
}

/// A handle whose condition the master refuses reads its set from the cache.
/// `delete_all` judges each id as a single delete does, against the master,
/// so a stale cached row the master has moved out of the set is kept.
#[tokio::test]
async fn narrowed_delete_all_skips_a_stale_cached_row() -> Result<()> {
    let shell = residual_shell();
    let dio = warmed_dio(Vista::new("teams", shell.clone())).await?;
    let mut not_blue = dio.cached();
    not_blue.add_condition("team", FilterOp::Ne, "blue")?;
    shell.set_field("1", "team", CborValue::Text("blue".into()));

    not_blue.delete_all().await?;

    assert!(shell.get_record("2").is_none(), "red row 2 deleted");
    assert!(
        shell.get_record("1").is_some(),
        "row 1 left the set upstream"
    );
    assert!(shell.get_record("3").is_some(), "blue row untouched");
    Ok(())
}

#[tokio::test]
async fn retried_insert_flash_is_a_no_op() -> Result<()> {
    let (dio, shell) = teams().await?;
    let insert = || ChangeFlash::insert("9", record(&[("id", "9"), ("team", "red")]));
    dio.flash(insert()).await?;
    dio.flash(insert()).await?;
    assert_eq!(shell.len(), 4);
    Ok(())
}
