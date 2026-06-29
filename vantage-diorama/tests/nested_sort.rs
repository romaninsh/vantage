//! Sorting / filtering by a **dotted** column (`obj.field`) whose value lives in
//! a nested CBOR `Map` — the shape a REST `?mode=detailed` response produces for
//! belongs-to objects (e.g. `launch_service_provider.name`). The scenery must
//! resolve the path into the nested map, not look up the literal flat key.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, SortDir, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn nested(provider_name: &str) -> CborValue {
    CborValue::Map(vec![(
        CborValue::Text("name".to_string()),
        CborValue::Text(provider_name.to_string()),
    )])
}

fn record(provider: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("provider".to_string(), nested(provider));
    r
}

fn provider_at(scenery: &Arc<dyn TableScenery>, idx: usize) -> Option<String> {
    let row = scenery.row(idx)?;
    match row.record.get("provider")? {
        CborValue::Map(entries) => entries.iter().find_map(|(k, v)| match (k, v) {
            (CborValue::Text(k), CborValue::Text(s)) if k == "name" => Some(s.clone()),
            _ => None,
        }),
        _ => None,
    }
}

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("provider", "String"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        // Insertion order: SpaceX, Agency, Rocket Lab — NOT alphabetical.
        .with_record("a", record("SpaceX"))
        .with_record("b", record("Agency"))
        .with_record("c", record("Rocket Lab"));
    Vista::new("items", Box::new(shell))
}

async fn eager_lens(cache: std::path::PathBuf) -> Arc<Lens> {
    Arc::new(
        Lens::new()
            .cache_at(cache)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    )
}

#[tokio::test]
async fn sort_by_dotted_nested_column() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("c.redb")).await;
    let dio = lens.make_dio(master()).await?;
    let scenery = dio.table_scenery().open().await?;
    let mut gen_rx = scenery.subscribe();
    let g = u64::from(*gen_rx.borrow_and_update());

    scenery.set_sort(Some("provider.name".to_string()), SortDir::Asc);
    // Wait for the reseed bump.
    tokio::time::timeout(std::time::Duration::from_millis(500), async {
        loop {
            if u64::from(*gen_rx.borrow_and_update()) > g {
                break;
            }
            gen_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("sort bump");

    // Alphabetical by provider.name: Agency, Rocket Lab, SpaceX.
    assert_eq!(provider_at(&scenery, 0).as_deref(), Some("Agency"));
    assert_eq!(provider_at(&scenery, 1).as_deref(), Some("Rocket Lab"));
    assert_eq!(provider_at(&scenery, 2).as_deref(), Some("SpaceX"));
    Ok(())
}
