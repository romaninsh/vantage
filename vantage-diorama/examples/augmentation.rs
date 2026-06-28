//! Generic augmentation: a master Vista is listed cheaply, and each row is
//! enriched one at a time from a *separate* detail Vista resolved through a
//! [`VistaCatalog`]. The detail source can be any backend — here both are
//! in-memory so the example is self-contained.
//!
//! Run with: `cargo run -p vantage-diorama --example augmentation`

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_diorama::{Augmentation, Fetch, Lens, MergeRule, RowStatus, Source};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};
use vantage_vista_factory::VistaCatalog;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), text(v)))
        .collect()
}

fn meta(columns: &[&str]) -> VistaMetadata {
    let mut m = VistaMetadata::new();
    for c in columns {
        let col = if *c == "id" {
            Column::new("id", "String").with_flag("id")
        } else {
            Column::new(*c, "String")
        };
        m = m.with_column(col);
    }
    m.with_id_column("id")
}

/// Master: a bucket listing — cheap columns only (id, size).
fn master() -> Vista {
    let shell = MockShell::new()
        .with_record(
            "us-east-1.tfstate",
            record(&[("id", "us-east-1.tfstate"), ("size", "4kb")]),
        )
        .with_record(
            "eu-west-1.tfstate",
            record(&[("id", "eu-west-1.tfstate"), ("size", "7kb")]),
        )
        .with_metadata(meta(&["id", "size"]));
    Vista::new("buckets", Box::new(shell))
}

/// Detail: the expensive per-file analysis (resources, serial), keyed by id.
fn detail() -> Vista {
    let shell = MockShell::new()
        .with_record(
            "us-east-1.tfstate",
            record(&[
                ("id", "us-east-1.tfstate"),
                ("resources", "42"),
                ("serial", "17"),
            ]),
        )
        .with_record(
            "eu-west-1.tfstate",
            record(&[
                ("id", "eu-west-1.tfstate"),
                ("resources", "8"),
                ("serial", "3"),
            ]),
        )
        .with_metadata(meta(&["id", "resources", "serial"]));
    Vista::new("tfstate-detail", Box::new(shell))
}

fn cell(scenery: &Arc<dyn vantage_diorama::TableScenery>, i: usize, key: &str) -> String {
    scenery
        .row(i)
        .and_then(|r| match r.record.get(key) {
            Some(CborValue::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "—".into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cache = std::env::temp_dir().join("vantage-augmentation-example.redb");
    let _ = std::fs::remove_file(&cache);

    // The catalog resolves the detail source by name (any persistence).
    let mut catalog = VistaCatalog::new();
    catalog.register("tfstate-detail", Arc::new(|| Ok(detail())));

    let lens = Arc::new(
        Lens::new()
            .cache_at(&cache)
            .viewport_debounce(Duration::from_millis(1))
            .build()?,
    );

    // Augmentation is a property of the Dio, not the Lens: configure it after
    // make_dio, before opening any scenery.
    let dio = lens.make_dio(master()).await?.augment(
        Arc::new(catalog),
        vec![Augmentation {
            table: "tfstate-detail".into(), // catalog name of the detail Vista
            source: Source::Id,             // master.id -> detail.id
            fetch: Fetch::PerRow,           // one detail record per master row
            merge: MergeRule {
                columns: vec!["resources".into(), "serial".into()],
            },
        }],
    );
    let scenery = dio.table_scenery().page_size(10).open().await?;
    let n = scenery.row_count();

    println!("after list pass (cheap columns only, rows are Incomplete):");
    for i in 0..n {
        if let Some(r) = scenery.row(i) {
            println!(
                "  {:<18} size={:<4} resources={}  [{:?}]",
                cell(&scenery, i, "id"),
                cell(&scenery, i, "size"),
                cell(&scenery, i, "resources"),
                r.status,
            );
        }
    }

    // Bring the rows on screen → the detail pass hydrates each from the second
    // Vista and merges its columns in place.
    scenery.set_viewport(0..n);
    for _ in 0..100 {
        let all_fresh = (0..n).all(|i| {
            matches!(
                scenery.row(i).map(|r| r.status.clone()),
                Some(RowStatus::Fresh)
            )
        });
        if all_fresh {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    println!("\nafter detail pass (augmented from tfstate-detail):");
    for i in 0..n {
        println!(
            "  {:<18} size={:<4} resources={:<3} serial={}",
            cell(&scenery, i, "id"),
            cell(&scenery, i, "size"),
            cell(&scenery, i, "resources"),
            cell(&scenery, i, "serial"),
        );
    }

    let _ = std::fs::remove_file(&cache);
    Ok(())
}
