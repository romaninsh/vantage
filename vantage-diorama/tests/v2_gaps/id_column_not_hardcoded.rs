//! A facade insert must put the id into the vista's own id column.
//!
//! Today the facade synthesises the inserted record with
//! `with_injected_id`, which writes the id under a literal `"id"` key
//! whatever the metadata declares. In 2.0 the id column is resolved once,
//! when the Dio is built, and every write path uses it.

use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::WritableValueSet;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

use crate::support::{eager_dio, record, text};

/// A store whose id column is `key`; it has no `id` column at all. The
/// returned handle shares the rows of the shell boxed into the master.
fn keyed_shell() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("key", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("key");
    MockShell::new().with_metadata(metadata)
}

/// The record's fields holding `value`, as `(column, value)` pairs.
fn fields_holding<'a>(
    record: &'a vantage_types::Record<CborValue>,
    value: &CborValue,
) -> Vec<(&'a str, &'a CborValue)> {
    record
        .iter()
        .filter(|(_, v)| *v == value)
        .map(|(k, v)| (k.as_str(), v))
        .collect()
}

#[tokio::test]
#[ignore = "v2 phase 3: facade insert hardcodes the id column as \"id\""]
async fn id_column_not_hardcoded() -> Result<()> {
    let shell = keyed_shell();
    let dio = eager_dio(Vista::new("keyed", Box::new(shell.clone()))).await;
    let worker = dio.take_write_worker_handle().await.expect("worker handle");

    let inserted = dio
        .vista()
        .insert_value("k1", &record(&[("name", "first")]))
        .await?;

    // Let the queued write land, so the master can be checked and nothing
    // outlives the test.
    drop(dio);
    tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .expect("write worker did not drain")
        .expect("write worker task completed");

    let k1 = text("k1");
    let stored = shell
        .get_record("k1")
        .expect("the insert reached the master");
    assert_eq!(
        (fields_holding(&inserted, &k1), fields_holding(&stored, &k1)),
        (vec![("key", &k1)], vec![("key", &k1)]),
        "the injected id must land in `key`, the declared id column \
         (each side: the facade's returned record, then the master row)"
    );
    Ok(())
}
