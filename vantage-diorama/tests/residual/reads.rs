//! The record scenery, the servo and the value scenery read the cache, which
//! holds rows outside a derived Dio's residual: each sees only the rows the
//! residual admits.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::aggregate::{Conditions, Count, Sum, Where};
use vantage_diorama::{Dio, RecordStatus};
use vantage_vista::{Capability, FilterOp};

use super::fixtures::items;
use crate::support::eager_dio;

/// An eager Dio over [`items`] narrowed by `n > 1`, which a master without
/// `FilterOps` refuses: the residual holds it, and the cache holds a, b, c.
async fn above_one() -> Result<Dio> {
    let parent = eager_dio(items(&[Capability::Order, Capability::Search])).await;
    let dio = parent
        .derive()
        .with_query(&crate::support::cond("n", FilterOp::Gt, 1))
        .build()
        .await?;
    assert_eq!(dio.residual().conditions.len(), 1, "the master refused it");
    #[cfg(feature = "diagnostics")]
    assert_eq!(dio.diagnostics().await?.cache_rows, 3, "the cache holds a");
    Ok(dio)
}

#[tokio::test]
async fn a_record_scenery_never_finds_a_row_outside_the_residual() -> Result<()> {
    let dio = above_one().await?;
    let outside = dio.record_scenery("a").open().await?;
    assert_eq!(outside.status(), RecordStatus::NotFound);
    assert!(outside.record().is_none());
    let inside = dio.record_scenery("b").open().await?;
    assert_eq!(inside.status(), RecordStatus::Fresh);
    Ok(())
}

#[tokio::test]
async fn a_servo_has_no_baseline_for_a_row_outside_the_residual() -> Result<()> {
    let dio = above_one().await?;
    assert!(dio.servo("a").await?.baseline().is_none());
    assert!(dio.servo("b").await?.baseline().is_some());
    Ok(())
}

#[tokio::test]
async fn value_aggregates_count_only_the_admitted_rows() -> Result<()> {
    let dio = above_one().await?;
    let count = dio.value_scenery(Count::rows()).open().await?;
    assert_eq!(count.value(), Some(CborValue::Integer(2.into())));
    let sum = dio.value_scenery(Sum::of("n")).open().await?;
    assert_eq!(sum.value(), Some(CborValue::Integer(5.into())));
    let where_red = dio
        .value_scenery(Where::new(
            Conditions::new().eq("team", "red"),
            Count::rows(),
        ))
        .open()
        .await?;
    assert_eq!(
        where_red.value(),
        Some(CborValue::Integer(1.into())),
        "c only"
    );
    Ok(())
}
