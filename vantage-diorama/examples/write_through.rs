//! Write-through demo: a writable in-memory master + redb cache + a Lens
//! writer that sends every flash to the master. Inserts go through the
//! facade Vista, which stages them in the cache, queues them for the writer
//! and returns once they are confirmed; subsequent reads (from the cache)
//! show the result.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example write_through

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{Cache, ChangeFlash, FlashKind, FlashOutcome, Lens, LoaderCx};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new("tasks", MockShell::new().with_metadata(metadata))
}

fn record(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");

    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .writer(|cx: LoaderCx, flash: ChangeFlash| async move {
            let label = flash.id().unwrap_or("*");
            println!("writer: {:?} {label} → master", flash.kind());
            let master = cx.master();
            match flash.kind() {
                FlashKind::Insert => {
                    let id = flash.id().expect("insert has id").to_string();
                    master.insert_value(id, flash.patch()).await?;
                }
                FlashKind::Replace => {
                    let id = flash.id().expect("replace has id").to_string();
                    master.replace_value(id, flash.patch()).await?;
                }
                FlashKind::Upsert => {
                    let id = flash.id().expect("upsert has id").to_string();
                    master.upsert_value(id, flash.patch()).await?;
                }
                FlashKind::Patch => {
                    let id = flash.id().expect("patch has id").to_string();
                    master.patch_value(id, flash.patch()).await?;
                }
                FlashKind::Delete => {
                    let id = flash.id().expect("delete has id").to_string();
                    master.delete(id).await?;
                }
                FlashKind::Clear => master.delete_all().await?,
            }
            // The Dio already staged the change in the cache; a confirmed
            // write keeps it.
            Ok(FlashOutcome::applied())
        })
        .build()?;

    let dio = lens.dio(master()).build().await?;
    let facade = dio.cached();

    // Each facade write returns once the writer has confirmed it.
    facade.insert_value("t1", &record("write docs")).await?;
    facade.insert_value("t2", &record("ship stage 3")).await?;

    println!("\ncache reads after the writes:");
    for (id, row) in facade.list_values().await? {
        let name = row.get("name").and_then(|v| match v {
            CborValue::Text(s) => Some(s.as_str()),
            _ => None,
        });
        println!("  {id}: {}", name.unwrap_or(""));
    }
    Ok(())
}
