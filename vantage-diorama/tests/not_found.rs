//! `DioShell`'s `TableShell` writes check a row's existence — cache first,
//! master on a cache miss — before enqueueing a patch or delete. Without
//! that check, an id absent everywhere would still enqueue: the write
//! queue is fire-and-forget, so `patch_value`/`delete` would return `Ok`
//! to the immediate caller, and the write's eventual failure would only
//! reach `DioEvent::WriteFailed` on the event bus.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{Dio, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

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

/// A Dio over one seeded row, cache warmed from the master.
async fn dio_with_one_row() -> Result<Dio> {
    let shell = MockShell::new()
        .with_capabilities(VistaCapabilities {
            can_count: true,
            can_insert: true,
            can_update: true,
            can_delete: true,
            ..VistaCapabilities::default()
        })
        .with_metadata(metadata())
        .with_record("1", rec(&[("id", "1"), ("name", "a")]));
    let master = Vista::new("items", Box::new(shell));

    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));
    let dio = lens.make_dio(master).await?;
    for (id, row) in dio.master().list_values().await? {
        dio.cache().insert_value(&id, &row).await?;
    }
    Ok(dio)
}

#[tokio::test]
async fn patch_and_delete_of_a_missing_row_are_not_found() -> Result<()> {
    let dio = dio_with_one_row().await?;
    let vista = dio.vista();

    let err = vista
        .patch_value("nope", &rec(&[("name", "x")]))
        .await
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");

    let err = vista.delete("nope").await.unwrap_err();
    assert!(err.is_not_found(), "{err}");

    Ok(())
}
