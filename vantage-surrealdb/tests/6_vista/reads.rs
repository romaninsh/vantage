//! Reads through a vista: listing, lookup by id, a YAML spec, and the
//! capabilities a table vista advertises.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_vista::{Capability, FilterOp, SortDirection, VistaFactory};

use super::{TestResult, product_table, seed_row, setup};

#[tokio::test]
async fn vista_lists_typed_surreal_as_cbor() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    seed_row(&db, &table_name, "beta", "Beta", 20, true).await?;

    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), table_name);
    assert_eq!(vista.id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);

    let alpha_key = format!("{}:alpha", table_name);
    let alpha = rows.get(&alpha_key).expect("row alpha");
    assert_eq!(
        alpha.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );
    assert_eq!(alpha.get("price"), Some(&CborValue::Integer(10i64.into())));
    assert_eq!(alpha.get("is_deleted"), Some(&CborValue::Bool(false)));

    Ok(())
}

#[tokio::test]
async fn before_select_refuses_vista_list_get_count_and_window() -> TestResult {
    use vantage_surrealdb::surrealdb::SurrealDB;
    use vantage_table::table::{Hook, SelectCx, select_hook};

    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name).with_hook(Hook::BeforeSelect(select_hook(
        |_cx: SelectCx<'_, SurrealDB>| {
            Box::pin(async { Err(vantage_core::error!("read refused")) })
        },
    )));
    let vista = db.vista_factory().from_table(table)?;

    let refused = |e: vantage_core::VantageError| e.to_string().contains("read refused");
    assert!(vista.list_values().await.is_err_and(refused));
    assert!(vista.get_value("alpha").await.is_err_and(refused));
    assert!(vista.count().await.is_err_and(refused));
    assert!(vista.fetch_window(0, 2).await.is_err_and(refused));
    Ok(())
}

#[tokio::test]
async fn vista_get_value_by_id() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;

    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    // Bare id — shell prefixes with table name.
    let row = vista.get_value("alpha").await?.expect("found");
    assert_eq!(row.get("name"), Some(&CborValue::Text("Alpha".to_string())));

    // Full record id also works.
    let full = format!("{}:alpha", table_name);
    let row2 = vista.get_value(&full).await?.expect("found");
    assert_eq!(
        row2.get("name"),
        Some(&CborValue::Text("Alpha".to_string()))
    );

    let missing = vista.get_value("nope").await?;
    assert!(missing.is_none());

    Ok(())
}

#[tokio::test]
async fn vista_yaml_loads_table_and_columns() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "biff", "Biff", 99, false).await?;

    let yaml = format!(
        r#"
name: product_view
columns:
  id:
    type: thing
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
  price:
    type: int
surreal:
  table: {}
"#,
        table_name
    );

    let vista = db.vista_factory().from_yaml(&yaml)?;

    assert_eq!(vista.name(), "product_view");
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.title_columns(), vec!["name"]);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 1);
    let (_, row) = rows.into_iter().next().unwrap();
    assert_eq!(row.get("name"), Some(&CborValue::Text("Biff".to_string())));

    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_write() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.has(Capability::Insert));
    assert!(caps.has(Capability::Update));
    assert!(caps.has(Capability::Delete));
    assert!(caps.has(Capability::ConfineWrites));
    // A read-write SurrealDB Vista is watchable via LIVE queries.
    assert!(caps.has(Capability::Subscribe));
    assert_eq!(vista.driver_name(), "surrealdb");

    Ok(())
}

/// The preview is the SurrealQL SELECT as it stands, with every condition and
/// order inline; a window's `LIMIT`/`START` is added at fetch time.
#[tokio::test]
async fn preview_renders_the_narrowed_select_without_a_window() -> TestResult {
    let (db, table_name) = setup().await;
    seed_row(&db, &table_name, "alpha", "Alpha", 10, false).await?;
    let mut vista = db
        .vista_factory()
        .from_table(product_table(db.clone(), &table_name))?;
    vista.add_condition("price", FilterOp::Gt, CborValue::Integer(5.into()))?;
    vista.set_order(Some(("name", SortDirection::Ascending)))?;
    let _ = vista.fetch_window(0, 2).await?;

    assert_eq!(
        vista.preview().query,
        Some(format!(
            "SELECT id, name, price, is_deleted FROM {table_name} WHERE price > 5 ORDER BY name"
        ))
    );
    Ok(())
}

#[tokio::test]
async fn capabilities_advertise_fetch_window() -> TestResult {
    let (db, table_name) = setup().await;
    let table = product_table(db.clone(), &table_name);
    let vista = db.vista_factory().from_table(table)?;
    assert!(vista.capabilities().has(Capability::Window));

    // Windowing is a read-side capability, so a read-only vista keeps it.
    #[cfg(feature = "rhai")]
    {
        let yaml = r#"
name: expensive_products
columns:
  id:
    type: thing
    flags: [id]
  name:
    type: string
surreal:
  rhai: |
    surreal::select().from("product").field("id").field("name").where(surreal::expr("price > 15"))
"#;
        let read_only = db.vista_factory().from_yaml(yaml)?;
        assert!(!read_only.capabilities().has(Capability::Insert));
        assert!(read_only.capabilities().has(Capability::Window));
    }

    Ok(())
}
