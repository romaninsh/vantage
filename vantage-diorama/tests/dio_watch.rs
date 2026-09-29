//! A Dio built over another Dio's narrowed facade follows the parent live:
//! `watch()` subscribes to the parent's event bus through `DioShell`, and a
//! parent change that moves a row across the filter shows up downstream.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Dio, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn shipment(status: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("status".to_string(), CborValue::Text(status.to_string()));
    record
}

fn master() -> MockShell {
    MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("status", "String"))
            .with_id_column("id"),
    )
}

async fn eager_dio(master: Vista) -> Result<Dio> {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    );
    lens.make_dio(master).await
}

/// Poll the derived cache until `id` is present (or absent). The watch runs on
/// a background task, so the change lands shortly after the parent write.
async fn wait_for(dio: &Dio, id: &str, present: bool) -> Result<Option<Record<CborValue>>> {
    for _ in 0..200 {
        let row = dio.cache().get_value(&id.to_string()).await?;
        if row.is_some() == present {
            return Ok(row);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{id} never became {}", if present { "present" } else { "absent" });
}

#[tokio::test]
async fn a_narrowed_dio_follows_its_parent_across_the_filter() -> Result<()> {
    let shell = master();
    shell.set_record("s1", shipment("In transit"));
    shell.set_record("s2", shipment("Delivered"));
    let parent = eager_dio(Vista::new("shipment", Box::new(shell))).await?;

    let mut in_transit = parent.vista();
    in_transit.add_condition_eq("status", CborValue::Text("In transit".into()))?;
    let derived = eager_dio(in_transit).await?;

    assert!(derived.master().can_watch());
    derived.watch().await?;
    wait_for(&derived, "s1", true).await?;
    wait_for(&derived, "s2", false).await?;

    // Patched into the filter: appears downstream with the new value.
    parent.patched("s2", shipment("In transit")).await?;
    let s2 = wait_for(&derived, "s2", true).await?.expect("present");
    assert_eq!(
        s2.get("status"),
        Some(&CborValue::Text("In transit".into()))
    );

    // Patched out of the filter: removed downstream.
    parent.patched("s1", shipment("Delivered")).await?;
    wait_for(&derived, "s1", false).await?;

    // Removed from the parent: removed downstream.
    parent.removed("s2").await?;
    wait_for(&derived, "s2", false).await?;
    Ok(())
}
