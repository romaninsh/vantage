//! A Dio narrowed with `with_condition_eq` must be narrowed on every read
//! path, not only in table sceneries.
//!
//! Today the condition goes into `base_conditions`, which only the table
//! scenery builder reads. The facade, value sceneries, record sceneries and
//! servos all read the whole cache. In 2.0 `with_condition` derives a new Dio
//! whose master carries the condition, so every path sees it.
//!
//! The cache is warmed by the Lens's blocking `on_start`, which lists
//! `dio.master()`. A derived 2.0 Dio runs it against its own narrowed master,
//! so `b` never reaches its cache and the record and servo checks bite.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::RecordStatus;

use super::fixtures::assert_no_failures;
use crate::support::{eager_dio, teams_master, text};

#[tokio::test]
#[ignore = "v2 phase 3: Dio base conditions reach only table sceneries"]
async fn dio_base_conditions_reach_every_path() -> Result<()> {
    // Members a (red), b (blue), c (red); narrowed to red, so b is outside.
    let dio = eager_dio(teams_master())
        .await
        .with_condition_eq("team", text("red"));
    let mut failures = Vec::new();

    let mut facade_ids: Vec<String> = dio.vista().list_values().await?.into_keys().collect();
    facade_ids.sort();
    if facade_ids != ["a", "c"] {
        failures.push(format!(
            "dio.vista().list_values() must list a, c; lists {facade_ids:?}"
        ));
    }

    let count = dio.value_scenery().count().open().await?.value();
    if count != Some(CborValue::Integer(2.into())) {
        failures.push(format!("value scenery count must be 2; is {count:?}"));
    }

    let outside = dio.record_scenery("b").await?;
    if !matches!(outside.status(), RecordStatus::NotFound) || outside.record().is_some() {
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
