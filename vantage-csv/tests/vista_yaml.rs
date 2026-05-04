//! YAML loader: parse a `VistaSpec`, produce a `Vista`, list rows.

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use vantage_csv::Csv;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_vista::VistaFactory;

fn data_dir() -> String {
    format!("{}/data", env!("CARGO_MANIFEST_DIR"))
}

fn load_yaml(name: &str) -> String {
    let path = format!("{}/{}", data_dir(), name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

#[tokio::test]
async fn yaml_loads_client_vista_and_lists_rows() {
    let csv = Csv::new(data_dir());
    let yaml = load_yaml("client.vista.yaml");

    let vista = csv.vista_factory().from_yaml(&yaml).expect("load yaml");

    assert_eq!(vista.name(), "client");
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);
    assert!(vista.get_column("contact_details").unwrap().is_hidden());
    assert!(!vista.get_column("name").unwrap().is_hidden());

    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.len(), 3);

    let marty = &rows["marty"];
    assert_eq!(
        marty.get("name"),
        Some(&CborValue::Text("Marty McFly".to_string()))
    );
    assert_eq!(marty.get("is_paying_client"), Some(&CborValue::Bool(true)));
}

#[tokio::test]
async fn yaml_eq_condition_filters_typed_column() {
    let csv = Csv::new(data_dir());
    let yaml = load_yaml("client.vista.yaml");

    let mut vista = csv.vista_factory().from_yaml(&yaml).unwrap();
    vista.add_condition_eq("is_paying_client", CborValue::Bool(true));

    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("marty"));
    assert!(rows.contains_key("doc"));
}

#[tokio::test]
async fn yaml_csv_source_renames_record_keys_and_filters() {
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
    let mut vista = csv.vista_factory().from_yaml(yaml).expect("load yaml");

    let rows = vista.list_values().await.unwrap();
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
    vista.add_condition_eq("is_active", CborValue::Bool(true));
    let filtered = vista.list_values().await.unwrap();
    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains_key("marty"));
    assert!(filtered.contains_key("doc"));
    assert!(!filtered.contains_key("biff"));
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
    let err = match csv.vista_factory().from_yaml(yaml) {
        Ok(_) => panic!("unknown field should fail"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("mystery") || msg.contains("unknown"),
        "expected typo-detecting error, got: {msg}"
    );
}
