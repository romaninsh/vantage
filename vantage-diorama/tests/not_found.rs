//! The facade's (`dio.cached()`) writes check a row's existence — cache first,
//! master on a cache miss — before enqueueing a patch or delete. A patch of
//! an id absent everywhere is `NotFound` up front rather than a later
//! `DioEvent::WriteFailed`; a delete of one is an idempotent `Ok` that
//! queues nothing.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::WritableValueSet;
use vantage_diorama::Dio;
use vantage_types::Record;
use vantage_vista::{Capability, Column, Vista, VistaMetadata, mocks::MockShell};

mod support;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), text(v));
    }
    r
}

fn metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_id_column("id")
}

/// A Dio over one seeded row, cache warmed from the master. The returned
/// shell is the master's, to make its reads fail.
async fn dio_with_one_row() -> Result<(Dio, MockShell)> {
    let shell = MockShell::new()
        .with_capabilities(
            [Capability::Insert, Capability::Update, Capability::Delete]
                .into_iter()
                .collect(),
        )
        .with_metadata(metadata())
        .with_record("1", rec(&[("id", "1"), ("name", "a")]));
    let dio = support::eager_dio(Vista::new("items", shell.clone())).await;
    Ok((dio, shell))
}

/// A row found in the cache is never looked up in the master: with every
/// master read failing, patch and delete of a cached row still succeed,
/// while an id absent from the cache has to ask the master and so fails.
#[tokio::test]
async fn cached_row_skips_the_master_read() -> Result<()> {
    let (dio, master) = dio_with_one_row().await?;
    let vista = dio.cached();
    master.set_fail_reads(true);

    vista.patch_value("1", &rec(&[("name", "b")])).await?;
    vista.delete("1").await?;

    let err = vista
        .patch_value("nope", &rec(&[("name", "x")]))
        .await
        .unwrap_err();
    assert!(!err.is_not_found(), "a cache miss reads the master: {err}");

    Ok(())
}

#[tokio::test]
async fn patch_missing_is_not_found_delete_missing_succeeds() -> Result<()> {
    let (dio, _master) = dio_with_one_row().await?;
    let vista = dio.cached();

    let err = vista
        .patch_value("nope", &rec(&[("name", "x")]))
        .await
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");

    assert!(vista.delete("nope").await.is_ok());

    Ok(())
}
