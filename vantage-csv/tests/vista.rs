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
use vantage_vista::{Capability, FilterOp};

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
    assert_eq!(vista.id_column(), Some("id"));

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

    let doc = vista.get_value("doc").await?.expect("doc exists");
    assert_eq!(
        doc.get("name"),
        Some(&CborValue::Text("Doc Brown".to_string()))
    );

    let missing = vista.get_value("nonexistent").await?;
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

    assert_eq!(vista.count().await?, 3);

    vista.add_condition("is_paying_client", FilterOp::Eq, CborValue::Bool(true))?;
    assert_eq!(vista.count().await?, 2);

    let rows = vista.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("marty"));
    assert!(rows.contains_key("doc"));
    assert!(!rows.contains_key("biff"));
    Ok(())
}

/// A relation's target is built by a caller-supplied constructor, and that
/// constructor usually returns a table typed with the target's own entity.
/// Resolving the bare target must not depend on the entity type — it is not
/// known at the traversal site.
#[tokio::test]
async fn vista_resolves_bare_target_of_typed_relation() -> Result<()> {
    use vantage_csv::AnyCsvType;
    use vantage_types::Record;

    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        // Target entity is `Record<AnyCsvType>`, not `EmptyEntity`.
        .with_one("bakery", "bakery_id", |csv: Csv| {
            Table::<Csv, Record<AnyCsvType>>::new("bakery", csv)
                .with_id_column("id")
                .with_column_of::<String>("name")
        });
    let vista = csv.vista_factory().from_table(table)?;

    let target = vista.get_ref("bakery", None)?;
    assert_eq!(target.name(), "bakery");

    let rows = target.list_values().await?;
    assert_eq!(rows.len(), 1);
    assert!(rows.contains_key("hill_valley"));
    Ok(())
}

/// From a parent row, a has-one target is narrowed by its id column through
/// the Vista, so the join is recorded in the target's query.
#[tokio::test]
async fn vista_traverses_has_one_from_a_row() -> Result<()> {
    use vantage_csv::AnyCsvType;
    use vantage_types::Record;

    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_one("bakery", "bakery_id", |csv: Csv| {
            Table::<Csv, Record<AnyCsvType>>::new("bakery", csv)
                .with_id_column("id")
                .with_column_of::<String>("name")
        });
    let vista = csv.vista_factory().from_table(table)?;

    let row: Record<CborValue> = [("bakery_id".to_string(), CborValue::from("hill_valley"))]
        .into_iter()
        .collect();
    let target = vista.get_ref("bakery", Some(&row))?;
    assert!(target.is_narrowed());
    let rows = target.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["hill_valley"]);

    let row: Record<CborValue> = [("bakery_id".to_string(), CborValue::from("nowhere"))]
        .into_iter()
        .collect();
    let empty = vista.get_ref("bakery", Some(&row))?.list_values().await?;
    assert!(empty.is_empty());
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

    // CSV advertises no write capability, so the Vista refuses as
    // Unsupported, naming the capability.
    let insert_err = vista
        .insert_value("x", &empty)
        .await
        .expect_err("insert should be unsupported");
    assert_eq!(insert_err.kind(), ErrorKind::Unsupported);
    assert_eq!(
        insert_err.context.get("capability").map(String::as_str),
        Some("\"insert\""),
        "expected the refusal to name the capability: {insert_err:?}"
    );

    let delete_err = vista
        .delete("x")
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

    let caps: Vec<Capability> = vista.capabilities().iter().collect();
    assert_eq!(caps, [Capability::Window, Capability::Traverse]);
    Ok(())
}

#[tokio::test]
async fn vista_opts_out_of_query_primitives() -> Result<()> {
    use vantage_core::ErrorKind;
    use vantage_vista::SortDirection;

    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("product", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name");
    let mut vista = csv.vista_factory().from_table(table)?;

    // A flat file has no native sort, search, operator filter or cursor;
    // each refuses as Unsupported so callers can evaluate it locally.
    let order_err = vista
        .set_order(Some(("name", SortDirection::Ascending)))
        .expect_err("CSV should reject set_order");
    assert_eq!(order_err.kind(), ErrorKind::Unsupported);

    let search_err = vista
        .set_search(Some("anything"))
        .expect_err("CSV should reject set_search");
    assert_eq!(search_err.kind(), ErrorKind::Unsupported);

    let op_err = vista
        .add_condition("name", FilterOp::Ne, "x")
        .expect_err("CSV should reject operator conditions");
    assert_eq!(op_err.kind(), ErrorKind::Unsupported);

    let cursor_err = vista
        .fetch_cursor(None, 10)
        .await
        .expect_err("CSV should reject fetch_cursor");
    assert_eq!(cursor_err.kind(), ErrorKind::Unsupported);

    let unknown = vista
        .add_condition("nope", FilterOp::Eq, "x")
        .expect_err("an unknown column is an error");
    assert_ne!(unknown.kind(), ErrorKind::Unsupported);
    assert!(vista.query().is_empty(), "refused clauses are not recorded");
    Ok(())
}

#[tokio::test]
async fn vista_window_slices_the_filtered_rows() -> Result<()> {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client");
    let vista = csv.vista_factory().from_table(table)?.with_condition(
        "is_paying_client",
        FilterOp::Eq,
        CborValue::Bool(true),
    )?;

    let page = vista.fetch_window(1, 5).await?;
    let ids: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["doc"]);
    assert_eq!(page.total, Some(2));
    Ok(())
}
