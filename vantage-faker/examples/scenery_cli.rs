//! Watch a live faker table *through a Diorama Scenery*.
//!
//! Unlike `fifo_cli` (which polls the Vista on a fixed 1s timer), this reads the
//! cache-backed [`TableScenery`] and only redraws when the Scenery bumps its
//! generation — i.e. when a faker delta actually lands. A viewport of the first
//! 10 rows is declared, and only those are rendered.
//!
//! Pipeline: `FifoEffect` broadcast → a forwarder feeds `dio.handle_event` → the
//! lens `on_event` mirrors the delta into the cache (`patched` / `removed`) →
//! the Scenery reactor reseeds and bumps its generation → we redraw.
//!
//! ```sh
//! cargo run --example scenery_cli
//! ```
//! Ctrl-C to quit.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use tempfile::TempDir;
use vantage_cli_util::render_records;
use vantage_core::Result;
use vantage_diorama::{ChangeEvent, Lens, SortDir, TableScenery};
use vantage_faker::{FakerColumn, FakerTable, FifoEffect};
use vantage_types::Record;

const VIEWPORT: usize = 10;

fn col(name: &str, ty: &str, is_id: bool) -> FakerColumn {
    FakerColumn {
        name: name.into(),
        ty: ty.into(),
        flags: if is_id { vec!["id".into()] } else { vec![] },
    }
}

fn render(scenery: &Arc<dyn TableScenery>) {
    // Clear the screen and home the cursor, then draw the current viewport.
    print!("\x1B[2J\x1B[H");
    let total = scenery.row_count();
    let shown = total.min(VIEWPORT);
    println!(
        "vantage-faker · scenery viewport [0..{VIEWPORT}] — redraws only on Scenery change · Ctrl-C to quit"
    );
    println!("showing {shown} of {total} rows\n");

    // Collect the first `VIEWPORT` rows into an id-keyed map for the table renderer.
    let mut records: IndexMap<String, Record<CborValue>> = IndexMap::new();
    for i in 0..shown {
        if let Some(row) = scenery.row(i) {
            let id = match row.record.get("id") {
                Some(CborValue::Text(s)) => s.clone(),
                _ => i.to_string(),
            };
            records.insert(id, row.record.clone());
        }
    }
    render_records(&records, Some("id"));
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");

    // Lens: cache deltas the faker pushes so the Scenery can read them back.
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_event(|dio, evt| {
                let dio = dio.clone();
                async move {
                    match evt {
                        ChangeEvent::Inserted {
                            id,
                            new: Some(record),
                        }
                        | ChangeEvent::Updated {
                            id,
                            new: Some(record),
                        } => dio.patched(id, record).await?,
                        ChangeEvent::Deleted { id } => dio.removed(id).await?,
                        ChangeEvent::Invalidated => {
                            dio.cache().clear().await?;
                            dio.notify_dataset_changed();
                        }
                        _ => {}
                    }
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );

    let columns = vec![
        col("id", "string", true),
        col("first_name", "string", false),
        col("email", "string", false),
        col("city", "string", false),
        col("amount", "decimal", false),
    ];

    let table = FakerTable::build(
        "events",
        columns,
        "id",
        Box::new(FifoEffect {
            interval: Duration::from_secs(1),
            retention_lo: Duration::from_secs(8),
            retention_hi: Duration::from_secs(15),
        }),
    );
    // Hand the master Vista to the Dio; keep the handle alive for the loop.
    let (vista, handle) = table.split();

    let dio = lens.make_dio(vista).await?;

    // Forward faker broadcast deltas into the Dio's on_event pipeline.
    let mut deltas = handle.events.subscribe();
    let dio_fwd = dio.clone();
    tokio::spawn(async move {
        while let Ok(evt) = deltas.recv().await {
            if let Err(e) = dio_fwd.handle_event(evt).await {
                eprintln!("forward error: {e}");
            }
        }
    });

    // Open the cache-backed scenery, newest-first, with a 10-row viewport.
    let scenery: Arc<dyn TableScenery> =
        dio.table_scenery().sort("id", SortDir::Asc).open().await?;
    scenery.set_viewport(0..VIEWPORT);

    // Redraw only when the Scenery signals a new generation.
    let mut generations = scenery.subscribe();
    loop {
        render(&scenery);
        if generations.changed().await.is_err() {
            break; // scenery dropped
        }
    }

    // Keep the faker loop alive until the render loop ends.
    drop(handle);
    Ok(())
}
