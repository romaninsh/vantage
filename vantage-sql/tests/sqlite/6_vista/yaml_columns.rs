//! YAML column declarations: flags reaching the vista column, and `lazy:`
//! columns the vista computes per record.

#[cfg(feature = "rhai")]
use ciborium::Value as CborValue;
#[cfg(feature = "rhai")]
use vantage_dataset::prelude::*;
#[cfg(feature = "rhai")]
use vantage_types::Record;
use vantage_vista::{ColumnFlag, VistaFactory};

use super::{TestResult, setup};

/// The YAML flags with a typed counterpart reach the vista column through
/// the typed table.
#[tokio::test]
async fn vista_yaml_column_flags_reach_the_vista() -> TestResult {
    let db = setup().await;
    let yaml = r#"
name: product
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [searchable, mandatory] }
  is_deleted: { type: bool, flags: [label] }
"#;
    let vista = db.vista_factory().from_yaml(yaml)?;

    let name = vista.column("name").expect("name");
    assert!(name.has_flag(ColumnFlag::Searchable), "{name:?}");
    assert!(name.has_flag(ColumnFlag::Mandatory), "{name:?}");
    let is_deleted = vista.column("is_deleted").expect("is_deleted");
    assert!(
        is_deleted.has_flag(ColumnFlag::from("label")),
        "{is_deleted:?}"
    );
    Ok(())
}

/// A `lazy:` column runs its Rhai script on each returned record (`row` in
/// scope), in declaration order — the second script reads the first's value.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn vista_yaml_lazy_columns_compute_from_row() -> TestResult {
    let db = setup().await;

    let yaml = r#"
name: product_view
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
  price:
    type: int
  price_label:
    lazy: '`${row.name}: ${row.price}`'
  shouted:
    lazy: 'row.price_label.to_upper()'
sqlite:
  table: product
"#;

    let vista = db.vista_factory().from_yaml(yaml)?;
    let rows = vista.list_values().await?;
    assert_eq!(
        rows["a"].get("price_label"),
        Some(&CborValue::Text("Alpha: 10".into()))
    );
    assert_eq!(
        rows["a"].get("shouted"),
        Some(&CborValue::Text("ALPHA: 10".into()))
    );

    // Computed by the Vista, so every read path fills it and writes drop it.
    assert!(vista.column("shouted").unwrap().is_computed());
    let window = vista.fetch_window(2, 1).await?;
    assert_eq!(
        window.rows[0].1.get("price_label"),
        Some(&CborValue::Text("Gamma: 30".into()))
    );
    let record: Record<CborValue> = [
        ("name", CborValue::Text("Delta".into())),
        ("price", CborValue::Integer(40i64.into())),
        ("price_label", CborValue::Text("stale".into())),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    vista.insert_value("d", &record).await?;
    let fetched = vista.get_value("d").await?.expect("inserted");
    assert_eq!(
        fetched.get("price_label"),
        Some(&CborValue::Text("Delta: 40".into()))
    );
    Ok(())
}
