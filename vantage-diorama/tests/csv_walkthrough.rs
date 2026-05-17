//! Stage 2 end-to-end: a CSV master, a redb cache, an `on_start` that
//! copies master rows into cache, and `dio.vista()` reading from cache.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::Lens;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::Vista;

fn fixture_dir() -> String {
    format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

fn build_products_vista() -> Result<Vista> {
    let csv = Csv::new(fixture_dir());
    let table = Table::<Csv, EmptyEntity>::new("products", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    csv.vista_factory().from_table(table)
}

#[tokio::test]
async fn on_start_loads_cache_and_facade_reads_from_it() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");
    let cache_path = tmp.path().join("cache.redb");

    let lens = Arc::new(
        Lens::new()
            .cache_at(&cache_path)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    );

    let master = build_products_vista()?;
    let dio = lens.make_dio(master).await?;

    // on_start ran (blocking default) — cache is populated.
    let facade = dio.vista();
    let rows = facade.list_values().await?;
    assert_eq!(rows.len(), 4, "facade reads four rows from cache");
    assert!(rows.contains_key("p1"));

    // Schema accessors forward to master.
    assert_eq!(facade.name(), "products");
    assert_eq!(facade.get_id_column(), Some("id"));
    assert_eq!(facade.get_column_names(), vec!["id", "name", "price"]);

    // Aggregate goes through cache, not master.
    assert_eq!(facade.get_count().await?, 4);

    // Single-row lookup.
    let p2 = facade
        .get_value(&"p2".to_string())
        .await?
        .expect("p2 in cache");
    assert_eq!(p2.get("name"), Some(&CborValue::Text("Cappuccino".into())));
    Ok(())
}

#[tokio::test]
async fn dio_without_on_start_reads_empty_cache() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");
    let cache_path = tmp.path().join("cache.redb");

    let lens = Arc::new(
        Lens::new()
            .cache_at(&cache_path)
            .build()
            .expect("build lens"),
    );

    let master = build_products_vista()?;
    let dio = lens.make_dio(master).await?;

    // No callback to fill the cache → facade reads nothing, master is intact.
    let facade = dio.vista();
    assert_eq!(facade.list_values().await?.len(), 0);
    assert_eq!(dio.master().list_values().await?.len(), 4);
    Ok(())
}

#[tokio::test]
async fn cache_persists_across_lens_drops() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");
    let cache_path = tmp.path().join("cache.redb");

    // First lens — populate the cache, then drop everything.
    {
        let lens = Arc::new(
            Lens::new()
                .cache_at(&cache_path)
                .on_start(|dio| {
                    let dio = dio.clone();
                    async move {
                        let rows = dio.master().list_values().await?;
                        dio.cache().insert_values(rows).await
                    }
                })
                .build()
                .expect("build lens"),
        );
        let dio = lens.make_dio(build_products_vista()?).await?;
        assert_eq!(dio.vista().list_values().await?.len(), 4);
    }

    // Second lens — no on_start, just reads from the persisted file.
    let lens = Arc::new(
        Lens::new()
            .cache_at(&cache_path)
            .build()
            .expect("rebuild lens"),
    );
    let dio = lens.make_dio(build_products_vista()?).await?;
    let rows = dio.vista().list_values().await?;
    assert_eq!(rows.len(), 4, "redb file survived lens recreation");
    Ok(())
}
