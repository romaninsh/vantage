//! Minimum useful Diorama: CSV master, redb cache, an eager start populates,
//! `dio.cached().list_values()` reads from cache.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example csv_walkthrough

use tempfile::TempDir;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, Lens, Loader, LoaderCx};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::Vista;

fn build_products_vista() -> Result<Vista> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let csv = Csv::new(dir);
    let table = Table::<Csv, EmptyEntity>::new("products", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    csv.vista_factory().from_table(table)
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");
    let cache_path = tmp.path().join("cache.redb");

    let lens = Lens::builder()
        .cache(Cache::redb(&cache_path))
        .loader(Loader::eager(|cx: LoaderCx| async move {
            let rows = cx.master().list_values().await?;
            println!("eager start: copying {} rows into cache", rows.len());
            cx.cache().insert_values(rows).await
        }))
        .build()?;

    let dio = lens.dio(build_products_vista()?).build().await?;

    println!("\nfacade reads (served from redb cache):");
    for (id, row) in dio.cached().list_values().await? {
        let name = row.get("name").and_then(|v| match v {
            ciborium::Value::Text(s) => Some(s.as_str()),
            _ => None,
        });
        let price = row.get("price").and_then(|v| match v {
            ciborium::Value::Integer(i) => Some(i128::from(*i)),
            _ => None,
        });
        println!("  {id:>14}  {:<24} ${:?}", name.unwrap_or(""), price);
    }
    Ok(())
}
