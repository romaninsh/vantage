//! A Dio narrowed with `with_query` must be narrowed on every read path,
//! not only in table sceneries.
//!
//! `with_query` derives a new Dio whose master carries the condition, so
//! every path sees it. The cache is warmed by the Lens's blocking eager
//! start, which lists the derived Dio's own narrowed master, so `b` never
//! reaches its cache and the record and servo checks bite.
//!
//! The aggregate path is checked in vantage-diorama-aggregate, which depends
//! on this crate and so can't be a dependency of its tests:
//! `narrowed_dio_aggregates_the_narrowed_set` in that crate's
//! `tests/narrowed_source.rs`.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::RecordStatus;
use vantage_vista::{FilterOp, VistaQuery};

use super::fixtures::{assert_no_failures, scenery_ids};
use crate::support::{cond, eager_dio, teams_master, text};

#[tokio::test]
async fn dio_base_conditions_reach_every_path() -> Result<()> {
    // Members a (red), b (blue), c (red); narrowed to red, so b is outside.
    let dio = eager_dio(teams_master())
        .await
        .with_query(&cond("team", FilterOp::Eq, text("red")))
        .await?;
    let mut failures = Vec::new();

    let mut facade_ids: Vec<String> = dio.cached().list_values().await?.into_keys().collect();
    facade_ids.sort();
    if facade_ids != ["a", "c"] {
        failures.push(format!(
            "dio.cached().list_values() must list a, c; lists {facade_ids:?}"
        ));
    }

    let fetched = dio.fetch(0..10, &VistaQuery::new()).await?;
    let mut fetched_ids: Vec<&str> = fetched
        .page
        .rows
        .iter()
        .map(|(id, _)| id.as_str())
        .collect();
    fetched_ids.sort();
    if fetched_ids != ["a", "c"] || !fetched.refused.is_empty() {
        failures.push(format!(
            "dio.fetch(0..10) must return a, c with nothing refused; returns {fetched_ids:?}, \
             refused {:?}",
            fetched.refused
        ));
    }

    let table_ids = scenery_ids(&dio.table_scenery().open().await?);
    if table_ids != ["a", "c"] {
        failures.push(format!("table scenery must show a, c; shows {table_ids:?}"));
    }

    let count = dio.value_scenery().count().open().await?.value();
    if count != Some(CborValue::Integer(2.into())) {
        failures.push(format!("value scenery count must be 2; is {count:?}"));
    }

    let outside = dio.record_scenery("b").open().await?;
    if outside.status() != RecordStatus::NotFound || outside.record().is_some() {
        failures.push(format!(
            "record_scenery(\"b\") must be NotFound; is {:?}",
            outside.status()
        ));
    }

    let servo = dio.servo("b").await?;
    if let Some(baseline) = servo.baseline() {
        failures.push(format!(
            "servo(\"b\") must start without a baseline; seeded {baseline:?}"
        ));
    }

    assert_no_failures(&failures);
    Ok(())
}
