//! `Table<Csv, _>` wrapped as a `Vista`: CBOR rows, conditions, windows,
//! traversal, and the capabilities a read-only flat file advertises.

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use vantage_core::{ErrorKind, Result};
use vantage_csv::{AnyCsvType, Csv};
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, FilterOp, SortDirection, Vista};

fn csv() -> Csv {
    Csv::new(format!("{}/data", env!("CARGO_MANIFEST_DIR")))
}

fn clients() -> Vista {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client");
    csv.vista_factory().from_table(table).unwrap()
}

fn products() -> Vista {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("product", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("calories")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted");
    csv.vista_factory().from_table(table).unwrap()
}

#[tokio::test]
async fn vista_carries_the_table_name_and_id_column() {
    let vista = products();

    assert_eq!(vista.name(), "product");
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.driver_name(), "csv");
}

#[tokio::test]
async fn list_values_maps_typed_columns_to_cbor() -> Result<()> {
    let rows = products().list_values().await?;
    assert_eq!(rows.len(), 5);

    let cupcake = &rows["flux_cupcake"];
    assert_eq!(
        cupcake.get("name"),
        Some(&CborValue::Text("Flux Capacitor Cupcake".to_string()))
    );
    assert_eq!(
        cupcake.get("calories"),
        Some(&CborValue::Integer(300.into()))
    );
    assert_eq!(cupcake.get("is_deleted"), Some(&CborValue::Bool(false)));
    Ok(())
}

#[tokio::test]
async fn get_value_returns_the_row_and_none_for_an_unknown_id() -> Result<()> {
    let vista = clients();

    let doc = vista.get_value("doc").await?.expect("doc exists");
    assert_eq!(
        doc.get("name"),
        Some(&CborValue::Text("Doc Brown".to_string()))
    );
    assert!(vista.get_value("nonexistent").await?.is_none());
    Ok(())
}

#[tokio::test]
async fn count_follows_the_applied_conditions() -> Result<()> {
    let mut vista = clients();
    assert_eq!(vista.count().await?, 3);

    vista.add_condition("is_paying_client", FilterOp::Eq, CborValue::Bool(true))?;

    assert_eq!(vista.count().await?, 2);
    let rows = vista.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["marty", "doc"]);
    Ok(())
}

#[tokio::test]
async fn conditions_combine_with_and() -> Result<()> {
    let vista = clients()
        .with_condition("is_paying_client", FilterOp::Eq, CborValue::Bool(true))?
        .with_condition("name", FilterOp::Eq, "Doc Brown")?;

    let rows = vista.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["doc"]);
    Ok(())
}

#[tokio::test]
async fn fetch_window_slices_the_filtered_rows_and_reports_the_total() -> Result<()> {
    let vista =
        clients().with_condition("is_paying_client", FilterOp::Eq, CborValue::Bool(true))?;

    let page = vista.fetch_window(1, 5).await?;

    let ids: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["doc"]);
    assert_eq!(page.total, Some(2));
    Ok(())
}

#[tokio::test]
async fn fetch_window_limit_caps_the_page() -> Result<()> {
    let page = products().fetch_window(0, 2).await?;

    let ids: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["flux_cupcake", "delorean_donut"]);
    assert_eq!(page.total, Some(5));
    Ok(())
}

#[tokio::test]
async fn bare_target_of_a_typed_relation_resolves() -> Result<()> {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_one("bakery", "bakery_id", |csv: Csv| {
            Table::<Csv, Record<AnyCsvType>>::new("bakery", csv)
                .with_id_column("id")
                .with_column_of::<String>("name")
        });
    let vista = csv.vista_factory().from_table(table)?;
    assert_eq!(vista.references()["bakery"].target, "bakery");

    let target = vista.get_ref("bakery", None)?;

    assert_eq!(target.name(), "bakery");
    let rows = target.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["hill_valley"]);
    Ok(())
}

#[tokio::test]
async fn has_one_from_a_row_is_narrowed_by_the_join_key() -> Result<()> {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_one("bakery", "bakery_id", |csv: Csv| {
            Table::<Csv, Record<AnyCsvType>>::new("bakery", csv)
                .with_id_column("id")
                .with_column_of::<String>("name")
        });
    let vista = csv.vista_factory().from_table(table)?;
    let row = |bakery: &str| -> Record<CborValue> {
        [("bakery_id".to_string(), CborValue::from(bakery))]
            .into_iter()
            .collect()
    };

    let target = vista.get_ref("bakery", Some(&row("hill_valley")))?;
    assert!(target.is_narrowed());
    assert_eq!(
        target.list_values().await?.keys().collect::<Vec<_>>(),
        ["hill_valley"]
    );

    let nowhere = vista.get_ref("bakery", Some(&row("nowhere")))?;
    assert!(nowhere.list_values().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn unknown_relation_is_an_error() {
    let vista = clients();

    assert!(vista.get_ref("nope", None).is_err());
}

#[tokio::test]
async fn advertised_capabilities_are_window_and_traverse() {
    let caps: Vec<Capability> = clients().capabilities().iter().collect();

    assert_eq!(caps, [Capability::Window, Capability::Traverse]);
}

#[tokio::test]
async fn writes_are_refused_as_unsupported_naming_the_capability() {
    let vista = clients();
    let empty = Record::new();

    let insert = vista
        .insert_value("x", &empty)
        .await
        .expect_err("insert is unsupported");
    assert_eq!(insert.kind(), ErrorKind::Unsupported);
    assert!(format!("{insert:?}").contains("insert"), "{insert:?}");

    let delete = vista.delete("x").await.expect_err("delete is unsupported");
    assert_eq!(delete.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn sort_search_operators_and_cursors_are_refused_as_unsupported() {
    let mut vista = products();

    let order = vista
        .set_order(Some(("name", SortDirection::Ascending)))
        .expect_err("order");
    assert_eq!(order.kind(), ErrorKind::Unsupported);

    let search = vista.set_search(Some("anything")).expect_err("search");
    assert_eq!(search.kind(), ErrorKind::Unsupported);

    let operator = vista
        .add_condition("name", FilterOp::Ne, "x")
        .expect_err("operator");
    assert_eq!(operator.kind(), ErrorKind::Unsupported);

    let cursor = vista.fetch_cursor(None, 10).await.expect_err("cursor");
    assert_eq!(cursor.kind(), ErrorKind::Unsupported);

    assert!(vista.query().is_empty(), "refused clauses are not recorded");
}

#[tokio::test]
async fn condition_on_an_unknown_column_is_an_error_but_not_unsupported() {
    let mut vista = products();

    let err = vista
        .add_condition("nope", FilterOp::Eq, "x")
        .expect_err("unknown column");

    assert_ne!(err.kind(), ErrorKind::Unsupported);
    assert!(vista.query().is_empty());
}

#[test]
fn preview_names_the_file_and_lists_the_applied_conditions() -> Result<()> {
    let vista = clients();
    let file = format!("{}/data/client.csv", env!("CARGO_MANIFEST_DIR"));

    let before = vista.preview().query.expect("a query preview");
    assert!(before.contains(&file), "{before}");
    assert!(!before.contains("where"), "{before}");

    let narrowed = vista
        .with_condition("is_paying_client", FilterOp::Eq, CborValue::Bool(true))?
        .with_condition("name", FilterOp::Eq, "Doc Brown")?;
    let after = narrowed.preview().query.expect("a query preview");
    assert!(after.contains(&file), "{after}");
    assert!(after.contains("is_paying_client = true"), "{after}");
    assert!(after.contains("name = \"Doc Brown\""), "{after}");
    Ok(())
}
