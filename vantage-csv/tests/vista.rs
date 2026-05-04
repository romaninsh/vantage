//! End-to-end round-trip: typed `Table<Csv, _>` → `Vista` → CBOR rows.
//!
//! Validates the stage 2 contract against a real backend (CSV).

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

fn csv() -> Csv {
    Csv::new(format!("{}/data", env!("CARGO_MANIFEST_DIR")))
}

#[tokio::test]
async fn vista_lists_typed_csv_as_cbor() -> Result<()> {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("product", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("calories")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted");

    let vista = csv.vista_factory().from_table(table)?;

    assert_eq!(vista.name(), "product");
    assert_eq!(vista.get_id_column(), Some("id"));

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 5);

    let cupcake = &rows["flux_cupcake"];
    let name = cupcake.get("name").expect("name field");
    assert_eq!(name, &CborValue::Text("Flux Capacitor Cupcake".to_string()));

    // Numeric round-trip: CSV "300" → AnyCsvType::Int → CBOR Integer
    let calories = cupcake.get("calories").expect("calories field");
    assert!(
        matches!(calories, CborValue::Integer(_)),
        "calories should be a CBOR integer, got {:?}",
        calories
    );

    // Bool round-trip: CSV "false" → AnyCsvType::Bool → CBOR Bool
    let is_deleted = cupcake.get("is_deleted").expect("is_deleted field");
    assert_eq!(is_deleted, &CborValue::Bool(false));
    Ok(())
}

#[tokio::test]
async fn vista_get_value_by_id() -> Result<()> {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client");
    let vista = csv.vista_factory().from_table(table)?;

    let doc = vista
        .get_value(&"doc".to_string())
        .await?
        .expect("doc exists");
    assert_eq!(
        doc.get("name"),
        Some(&CborValue::Text("Doc Brown".to_string()))
    );

    let missing = vista.get_value(&"nonexistent".to_string()).await?;
    assert!(missing.is_none());
    Ok(())
}

#[tokio::test]
async fn vista_count_with_eq_condition() -> Result<()> {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client");
    let mut vista = csv.vista_factory().from_table(table)?;

    assert_eq!(vista.get_count().await?, 3);

    vista.add_condition_eq("is_paying_client", CborValue::Bool(true))?;
    assert_eq!(vista.get_count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("marty"));
    assert!(rows.contains_key("doc"));
    assert!(!rows.contains_key("biff"));
    Ok(())
}

#[tokio::test]
async fn vista_writes_return_read_only_error() -> Result<()> {
    use vantage_core::ErrorKind;
    use vantage_dataset::prelude::WritableValueSet;
    use vantage_types::Record;

    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("bakery", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name");
    let vista = csv.vista_factory().from_table(table)?;

    let empty = Record::new();

    // CSV doesn't advertise can_insert/can_delete → Unsupported, not the
    // Unimplemented placeholder that would mean a driver bug.
    let insert_err = vista
        .insert_value(&"x".to_string(), &empty)
        .await
        .expect_err("insert should be unsupported");
    assert_eq!(insert_err.kind(), ErrorKind::Unsupported);
    assert!(
        insert_err.to_string().contains("can_insert"),
        "expected message to mention capability: {}",
        insert_err
    );

    let delete_err = vista
        .delete(&"x".to_string())
        .await
        .expect_err("delete should be unsupported");
    assert_eq!(delete_err.kind(), ErrorKind::Unsupported);
    Ok(())
}

#[tokio::test]
async fn vista_capabilities_advertise_read_only() -> Result<()> {
    let csv = csv();
    let table =
        Table::<Csv, EmptyEntity>::new("bakery", csv.clone()).with_column_of::<String>("name");
    let vista = csv.vista_factory().from_table(table)?;

    let caps = vista.capabilities();
    assert!(caps.can_count);
    assert!(!caps.can_insert);
    assert!(!caps.can_update);
    assert!(!caps.can_delete);
    assert!(!caps.can_subscribe);
    Ok(())
}
