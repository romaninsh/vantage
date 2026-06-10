//! Stage 3 demo: a writable in-memory master + redb cache + `on_write`
//! that mirrors every write into both. Inserts go through the facade
//! Vista, get enqueued, the worker drains, and subsequent reads (from
//! the cache) show the result.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example write_through

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{Lens, WriteOp};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new("tasks", Box::new(MockShell::new().with_metadata(metadata)))
}

fn record(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");

    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_write(|dio, op| {
                let dio = dio.clone();
                async move {
                    match &op {
                        WriteOp::Insert { id, .. } => {
                            println!("on_write: Insert {id} → master + cache")
                        }
                        WriteOp::Delete { id } => {
                            println!("on_write: Delete {id} → master + cache")
                        }
                        WriteOp::Replace { id, .. } => {
                            println!("on_write: Replace {id} → master + cache")
                        }
                        WriteOp::Patch { id, .. } => {
                            println!("on_write: Patch {id} → master + cache")
                        }
                        WriteOp::DeleteAll => println!("on_write: DeleteAll → master + cache"),
                    }
                    match op {
                        WriteOp::Insert { id, record } => {
                            dio.master().insert_value(id.clone(), &record).await?;
                            dio.cache().insert_value(&id, &record).await?;
                        }
                        WriteOp::Replace { id, record } => {
                            dio.master().replace_value(id.clone(), &record).await?;
                            dio.cache().insert_value(&id, &record).await?;
                        }
                        WriteOp::Patch { id, partial } => {
                            dio.master().patch_value(id.clone(), &partial).await?;
                            // Patch on cache: read-modify-write.
                            let mut merged = dio.cache().get_value(&id).await?.unwrap_or_default();
                            for (k, v) in &partial {
                                merged.insert(k.clone(), v.clone());
                            }
                            dio.cache().insert_value(&id, &merged).await?;
                        }
                        WriteOp::Delete { id } => {
                            dio.master().delete(id.clone()).await?;
                            dio.cache().delete_value(&id).await?;
                        }
                        WriteOp::DeleteAll => {
                            dio.master().delete_all().await?;
                            dio.cache().clear().await?;
                        }
                    }
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );

    let dio = lens.make_dio(master()).await?;
    let facade = dio.vista();

    facade
        .insert_value(&"t1".to_string(), &record("write docs"))
        .await?;
    facade
        .insert_value(&"t2".to_string(), &record("ship stage 3"))
        .await?;

    // Worker drains the queue.
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\ncache reads after writes drained:");
    for (id, row) in facade.list_values().await? {
        let name = row.get("name").and_then(|v| match v {
            CborValue::Text(s) => Some(s.as_str()),
            _ => None,
        });
        println!("  {id}: {}", name.unwrap_or(""));
    }
    Ok(())
}
