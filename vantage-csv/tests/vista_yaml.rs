//! YAML loader: parse a `VistaSpec`, produce a `Vista`, list rows.

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_vista::{ColumnFlag, FilterOp, VistaFactory};

fn data_dir() -> String {
    format!("{}/data", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn yaml_csv_source_renames_record_keys_and_filters() -> Result<()> {
    // The CSV file has a column header `is_paying_client`; the spec exposes
    // it as `is_active` via a `csv.source` override. Listing must surface the
    // spec name, and filtering must match against the spec name too.
    let csv = Csv::new(data_dir());
    let yaml = r#"
name: client
columns:
  id:
    type: string
    flags: [id]
  full_name:
    type: string
    csv:
      source: name
  is_active:
    type: bool
    csv:
      source: is_paying_client
csv:
  path: client.csv
"#;
    let mut vista = csv.vista_factory().from_yaml(yaml)?;

    let rows = vista.list_values().await?;
    let marty = &rows["marty"];
    assert_eq!(
        marty.get("full_name"),
        Some(&CborValue::Text("Marty McFly".to_string())),
        "spec column `full_name` should carry the renamed CSV `name` column",
    );
    assert!(
        marty.get("name").is_none(),
        "raw CSV header `name` must not leak into spec-named records",
    );
    assert_eq!(marty.get("is_active"), Some(&CborValue::Bool(true)));

    // Filtering must match spec names, not raw CSV headers.
    vista.add_condition("is_active", FilterOp::Eq, CborValue::Bool(true))?;
    let filtered = vista.list_values().await?;
    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains_key("marty"));
    assert!(filtered.contains_key("doc"));
    assert!(!filtered.contains_key("biff"));
    Ok(())
}

fn load_yaml(name: &str) -> String {
    let path = format!("{}/{}", data_dir(), name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// The injector narrows a spec-built table; CSV has no resolver, so its
/// table builder refuses to build another table.
#[tokio::test]
async fn injector_condition_applies_to_a_spec_built_table() -> Result<()> {
    use std::sync::{Arc, Mutex};
    use vantage_csv::AnyCsvType;
    use vantage_table::table::{BuildKind, TableInjector};
    use vantage_table::traits::table_source::TableSource;

    let built_other = Arc::new(Mutex::new(None));
    let seen = built_other.clone();
    let only_doc: TableInjector<Csv> = Arc::new(move |cx, table| {
        assert_eq!((cx.key, &cx.kind), ("client", &BuildKind::Root));
        *seen.lock().unwrap() = Some((cx.build)("order").is_err());
        let doc = AnyCsvType::new("doc".to_string());
        table.add_condition(table.data_source().eq_value_condition("id", doc)?);
        Ok(())
    });
    let vista = Csv::new(data_dir())
        .vista_factory()
        .with_injector(only_doc)
        .from_yaml(&load_yaml("client.vista.yaml"))?;

    let rows = vista.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["doc"]);
    assert_eq!(*built_other.lock().unwrap(), Some(true));
    Ok(())
}

#[tokio::test]
async fn yaml_spec_sets_name_id_title_and_hidden_flags() -> Result<()> {
    let csv = Csv::new(data_dir());

    let vista = csv
        .vista_factory()
        .from_yaml(&load_yaml("client.vista.yaml"))?;

    assert_eq!(vista.name(), "client");
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.title_columns(), vec!["name"]);
    let hidden = |name: &str| vista.column(name).unwrap().has_flag(ColumnFlag::Hidden);
    assert!(hidden("contact_details"));
    assert!(!hidden("name"));
    Ok(())
}

#[tokio::test]
async fn yaml_vista_lists_rows_with_declared_types() -> Result<()> {
    let csv = Csv::new(data_dir());
    let vista = csv
        .vista_factory()
        .from_yaml(&load_yaml("client.vista.yaml"))?;

    let rows = vista.list_values().await?;

    assert_eq!(rows.len(), 3);
    let marty = &rows["marty"];
    assert_eq!(
        marty.get("name"),
        Some(&CborValue::Text("Marty McFly".to_string()))
    );
    assert_eq!(marty.get("is_paying_client"), Some(&CborValue::Bool(true)));
    Ok(())
}

#[tokio::test]
async fn yaml_vista_filters_on_a_typed_column() -> Result<()> {
    let csv = Csv::new(data_dir());
    let mut vista = csv
        .vista_factory()
        .from_yaml(&load_yaml("client.vista.yaml"))?;

    vista.add_condition("is_paying_client", FilterOp::Eq, CborValue::Bool(true))?;

    let rows = vista.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["marty", "doc"]);
    Ok(())
}

#[tokio::test]
async fn yaml_column_type_aliases_lower_to_typed_columns() -> Result<()> {
    let csv = Csv::new(data_dir());
    let yaml = r#"
name: product
columns:
  id:
    type: string
    flags: [id]
  calories:
    type: integer
  price:
    type: i64
  is_deleted:
    type: boolean
  inventory:
    type: json
csv:
  path: product.csv
"#;
    let vista = csv.vista_factory().from_yaml(yaml)?;

    let rows = vista.list_values().await?;

    let cupcake = &rows["flux_cupcake"];
    assert_eq!(
        cupcake.get("calories"),
        Some(&CborValue::Integer(300.into()))
    );
    assert_eq!(cupcake.get("price"), Some(&CborValue::Integer(120.into())));
    assert_eq!(cupcake.get("is_deleted"), Some(&CborValue::Bool(false)));
    assert!(
        cupcake.get("inventory").is_some_and(|v| !v.is_text()),
        "json column should not stay text"
    );
    Ok(())
}

#[test]
fn yaml_rejects_unknown_column_type() {
    let csv = Csv::new(data_dir());
    let yaml = r#"
name: client
columns:
  id:
    type: string
    flags: [id]
  name:
    type: quaternion
csv:
  path: client.csv
"#;

    let err = csv
        .vista_factory()
        .from_yaml(yaml)
        .expect_err("unknown column type should fail");

    assert!(err.to_string().contains("quaternion"), "{err}");
}

#[test]
fn yaml_requires_the_csv_block() {
    let csv = Csv::new(data_dir());
    let yaml = r#"
name: client
columns:
  id:
    type: string
    flags: [id]
"#;

    assert!(csv.vista_factory().from_yaml(yaml).is_err());
}

#[tokio::test]
async fn yaml_absolute_path_ignores_the_base_directory() -> Result<()> {
    let csv = Csv::new("/nonexistent-base-dir");
    let yaml = format!(
        r#"
name: bakery
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
csv:
  path: {}/bakery.csv
"#,
        data_dir()
    );

    let vista = csv.vista_factory().from_yaml(&yaml)?;

    let rows = vista.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["hill_valley"]);
    Ok(())
}

#[test]
fn yaml_rejects_unknown_csv_block_field() {
    let csv = Csv::new(data_dir());
    let yaml = r#"
name: client
columns:
  id:
    type: string
    flags: [id]
csv:
  path: client.csv
  mystery: true
"#;

    let err = csv
        .vista_factory()
        .from_yaml(yaml)
        .expect_err("unknown field should fail");

    assert!(err.to_string().contains("mystery"), "{err}");
}
