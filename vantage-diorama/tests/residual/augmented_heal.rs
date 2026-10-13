//! A settled view sweeping on an augmented value heals when a write puts a
//! row back without its augmented values: it reads as loading, the row is
//! fetched again, and the view settles with it hydrated. And a write to a
//! row the cache doesn't hold is judged on the master's row augmented in
//! memory, so it can't reach a row outside the set.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{ChangeEvent, ChangeFlash, Dio, LoadState, TableScenery};
use vantage_types::Record;
use vantage_vista::{FilterOp, Vista};

use super::augmented_views::{name_of, settle};
use super::big_bucket::{DetailProbe, ROWS, john_ids, master_shell, probed_dio};

fn bare(id: &str) -> Record<CborValue> {
    [
        ("id".to_string(), CborValue::Text(id.into())),
        ("kind".to_string(), CborValue::Text("blue".into())),
    ]
    .into_iter()
    .collect()
}

/// Every row shown is a John that has hydrated.
fn only_johns(scenery: &std::sync::Arc<dyn TableScenery>) {
    for i in 0..scenery.row_count() {
        assert_eq!(name_of(scenery, i), Some(CborValue::Text("John".into())));
    }
}

/// A settled `johns` view over a Dio reading its detail through `probe`, and
/// how many detail reads settling it took.
async fn settled_johns(
    probe: &DetailProbe,
) -> Result<(Dio, std::sync::Arc<dyn TableScenery>, usize)> {
    let dio = probed_dio(
        Vista::new("bucket", master_shell()),
        probe.clone(),
        &["name", "size"],
    )
    .await;
    let johns = dio
        .with_query(&crate::support::cond("name", FilterOp::Eq, "John"))
        .await?;
    let view = johns.table_scenery().page_size(20).open().await?;
    view.set_viewport(0..20);
    assert_eq!(settle(&view, only_johns).await, john_ids());
    // Let a stray re-read land before counting.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let reads = probe.gets();
    assert!(reads >= ROWS, "every row read");
    Ok((johns, view, reads))
}

/// The view settles again, still every John, once `o007` was read again.
async fn healed(probe: &DetailProbe, view: &std::sync::Arc<dyn TableScenery>, reads: usize) {
    let refetched = async {
        while probe.gets() <= reads {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), refetched)
        .await
        .expect("o007 was never read again");
    assert_eq!(settle(view, only_johns).await, john_ids());
    assert_eq!(view.load_state(), LoadState::Complete);
}

#[tokio::test]
async fn a_sweeping_view_heals_after_an_update_drops_a_rows_detail() -> Result<()> {
    let probe = DetailProbe::default();
    let (johns, view, reads) = settled_johns(&probe).await?;
    johns
        .apply_change(ChangeEvent::Updated {
            id: "o007".into(),
            new: Some(bare("o007")),
        })
        .await?;
    healed(&probe, &view, reads).await;
    Ok(())
}

#[tokio::test]
async fn a_sweeping_view_heals_after_a_replace_drops_a_rows_detail() -> Result<()> {
    let probe = DetailProbe::default();
    let (johns, view, reads) = settled_johns(&probe).await?;
    johns
        .flash(ChangeFlash::replace("o007", bare("o007")))
        .await?;
    healed(&probe, &view, reads).await;
    Ok(())
}

/// Nothing is cached: no view has listed the Dio.
async fn cold_johns() -> Result<(Dio, Dio)> {
    let dio = probed_dio(
        Vista::new("bucket", master_shell()),
        DetailProbe::default(),
        &["name", "size"],
    )
    .await;
    let johns = dio
        .with_query(&crate::support::cond("name", FilterOp::Eq, "John"))
        .await?;
    Ok((dio, johns))
}

fn kind_of(row: &Record<CborValue>) -> Option<&CborValue> {
    row.get("kind")
}

#[tokio::test]
async fn a_flash_to_an_uncached_row_outside_the_set_is_refused() -> Result<()> {
    let (_, johns) = cold_johns().await?;
    let patch = johns
        .flash(ChangeFlash::partial("o002", bare("o002")))
        .await
        .expect_err("a Jane is outside the set");
    assert!(patch.is_conflict(), "{patch}");
    let delete = johns
        .flash(ChangeFlash::delete("o002"))
        .await
        .expect_err("a Jane is outside the set");
    assert!(delete.is_conflict(), "{delete}");
    let held = johns
        .master()
        .get_value("o002")
        .await?
        .expect("never deleted");
    assert_eq!(
        kind_of(&held),
        Some(&CborValue::Text("red".into())),
        "never patched"
    );
    assert!(held.get("name").is_none(), "{held:?}");
    Ok(())
}

#[tokio::test]
async fn a_facade_write_to_an_uncached_row_outside_the_set_writes_nothing() -> Result<()> {
    let (dio, _) = cold_johns().await?;
    let johns = dio.cached().with_condition("name", FilterOp::Eq, "John")?;
    let patch = johns
        .patch_value("o002", &bare("o002"))
        .await
        .expect_err("a Jane is outside the set");
    assert!(patch.is_not_found(), "{patch}");
    johns.delete("o002").await?;
    let held = dio
        .master()
        .get_value("o002")
        .await?
        .expect("never deleted");
    assert_eq!(
        kind_of(&held),
        Some(&CborValue::Text("red".into())),
        "never patched"
    );
    Ok(())
}
