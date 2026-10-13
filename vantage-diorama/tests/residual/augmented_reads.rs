//! Outside table sceneries, every path that judges cached rows against a
//! residual on an augmented column hydrates them first: the record scenery,
//! the servo's baseline, the value scenery's aggregates and the write
//! rules. A written record never gains an augmented column on its way to
//! the master, which has never heard of it.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{ChangeFlash, Dio, RecordStatus};
use vantage_types::Record;
use vantage_vista::FilterOp;

use super::big_bucket::{copied_dio, john_ids};

/// [`copied_dio`] narrowed to `name <op> John`: every cached row is still
/// unhydrated.
async fn narrowed(op: FilterOp) -> Result<Dio> {
    let dio = copied_dio().await?;
    let narrowed = dio
        .with_query(&crate::support::cond("name", op, "John"))
        .await?;
    assert_eq!(
        narrowed.residual().conditions.len(),
        1,
        "augmented: residual"
    );
    Ok(narrowed)
}

fn kind(value: &str) -> Record<CborValue> {
    [("kind".to_string(), CborValue::Text(value.into()))]
        .into_iter()
        .collect()
}

#[tokio::test]
async fn a_record_scenery_judges_the_hydrated_row() -> Result<()> {
    let johns = narrowed(FilterOp::Eq).await?;
    assert!(matches!(
        johns.record_scenery("o007").await?.status(),
        RecordStatus::Fresh
    ));
    assert!(matches!(
        johns.record_scenery("o001").await?.status(),
        RecordStatus::NotFound
    ));
    let others = narrowed(FilterOp::Ne).await?;
    assert!(
        matches!(
            others.record_scenery("o017").await?.status(),
            RecordStatus::NotFound
        ),
        "an unhydrated John isn't admitted by Ne"
    );
    Ok(())
}

#[tokio::test]
async fn a_servo_seeds_its_baseline_from_the_hydrated_row() -> Result<()> {
    let johns = narrowed(FilterOp::Eq).await?;
    let servo = johns.servo("o007").await?;
    let baseline = servo.baseline().expect("an in-set row has a baseline");
    assert_eq!(baseline.get("name"), Some(&CborValue::Text("John".into())));
    assert!(johns.servo("o001").await?.baseline().is_none());
    Ok(())
}

#[tokio::test]
async fn aggregates_count_and_sum_every_hydrated_john() -> Result<()> {
    let johns = narrowed(FilterOp::Eq).await?;
    let count = johns.value_scenery().count().open().await?;
    assert_eq!(count.value(), Some(CborValue::Integer(20.into())));
    // 7 + 17 + ... + 197.
    let sum = johns.value_scenery().sum("size").open().await?;
    assert_eq!(sum.value(), Some(CborValue::Integer(2040.into())));
    Ok(())
}

#[tokio::test]
async fn a_flash_on_an_augmented_set_judges_hydrated_rows_and_writes_no_augmented_column()
-> Result<()> {
    let johns = narrowed(FilterOp::Eq).await?;
    // An unhydrated cached John is in the set: patching it is no Conflict.
    johns
        .flash(ChangeFlash::partial("o027", kind("blue")))
        .await?;
    // A new record gets no `name = John` filled in: the detail pass fills
    // that column, and the master has never heard of it.
    let mut record = kind("red");
    record.insert("id".into(), CborValue::Text("n1".into()));
    johns.flash(ChangeFlash::insert("n1", record)).await?;
    let stored = johns.master().get_value("n1").await?.expect("inserted");
    assert!(stored.get("name").is_none(), "{stored:?}");
    let patched = johns.master().get_value("o027").await?.expect("held");
    assert_eq!(patched.get("kind"), Some(&CborValue::Text("blue".into())));
    assert!(patched.get("name").is_none(), "{patched:?}");
    Ok(())
}

#[tokio::test]
async fn facade_writes_narrowed_on_an_augmented_column_judge_the_hydrated_row() -> Result<()> {
    let dio = copied_dio().await?;
    let johns = dio.cached().with_condition("name", FilterOp::Eq, "John")?;
    johns.patch_value("o017", &kind("blue")).await?;
    let patched = dio.master().get_value("o017").await?.expect("held");
    assert_eq!(patched.get("kind"), Some(&CborValue::Text("blue".into())));
    assert!(patched.get("name").is_none(), "{patched:?}");

    johns.delete("o007").await?;
    assert!(dio.master().get_value("o007").await?.is_none(), "deleted");
    johns.delete("o001").await?;
    assert!(
        dio.master().get_value("o001").await?.is_some(),
        "a Jane is outside the set: deleting it writes nothing"
    );
    let left: Vec<String> = johns.list_values().await?.into_keys().collect();
    assert_eq!(left.len(), john_ids().len() - 1);
    Ok(())
}
