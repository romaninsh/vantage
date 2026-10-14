//! Generic augmentation: a master Vista is listed cheaply, and each row is
//! enriched one at a time from a *separate* detail Vista resolved through a
//! [`Catalog`]. The detail source can be any backend — here both are
//! in-memory so the example is self-contained.
//!
//! Run with: `cargo run -p vantage-diorama --example augmentation`

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_diorama::{
    Augmentation, Cache, Detail, Fetch, Lens, LoadState, MergeRule, RowStatus, Source, Tuning,
};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Catalog, Column, Vista, VistaMetadata};

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
    Vista::new("buckets", shell)
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
    Vista::new("tfstate-detail", shell)
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
    let catalog = Catalog::new().with("tfstate-detail", |_: &str| Ok(detail()));

    let lens = Lens::builder()
        .cache(Cache::redb(&cache))
        .tuning(Tuning::default().with_viewport_debounce(Duration::from_millis(1)))
        .build()?;

    // Augmentation is a property of the Dio, not the Lens: it is fixed when
    // the Dio is built.
    let dio = lens
        .dio(master())
        .augment(vec![Augmentation::new(
            // the detail Vista, resolved by name through the catalog
            // (`Detail::Fixed` takes a direct handle for get-only sources no
            // resolver knows)
            Detail::Resolved {
                resolver: Arc::new(catalog),
                name: "tfstate-detail".into(),
            },
            Source::Id,    // master.id -> detail.id
            Fetch::PerRow, // one detail record per master row
            MergeRule::lift(["resources", "serial"]),
        )])
        .build()
        .await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;
    // The list pass runs detached: the scenery opens `Loading` and the rows
    // arrive with a generation bump.
    for _ in 0..100 {
        if !matches!(scenery.load_state(), LoadState::Loading) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
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
    scenery.set_viewport(0..n, 0..n);
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
