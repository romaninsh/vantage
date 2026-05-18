//! Minimum useful Diorama: CSV master, redb cache, `on_start` populates,
//! `dio.vista().list_values()` reads from cache.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example csv_walkthrough

use std::sync::Arc;

use tempfile::TempDir;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::Lens;
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

    let lens = Arc::new(
        Lens::new()
            .cache_at(&cache_path)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    println!("on_start: copying {} rows into cache", rows.len());
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    );

    let dio = lens.make_dio(build_products_vista()?).await?;

    println!("\nfacade reads (served from redb cache):");
    for (id, row) in dio.vista().list_values().await? {
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
