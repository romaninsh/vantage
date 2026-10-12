use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct DummyTable {
    dummy: DummyTableBlock,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct DummyTableBlock {
    path: String,
}

type DummySpec = VistaSpec<DummyTable, NoExtras, NoExtras>;

#[test]
fn parses_minimal_spec() {
    let yaml = r#"
name: clients
columns:
  id:
    type: int
    flags: [id]
  name:
    type: string
    flags: [title, searchable]
dummy:
  path: data/clients.csv
"#;
    let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(spec.name, "clients");
    assert_eq!(spec.columns.len(), 2);
    assert_eq!(spec.columns["id"].col_type.as_deref(), Some("int"));
    assert_eq!(
        spec.columns["name"].flags,
        vec![ColumnFlag::Title, ColumnFlag::Searchable]
    );
    assert_eq!(spec.driver.dummy.path, "data/clients.csv");
}

#[test]
fn rejects_unknown_driver_field() {
    let yaml = r#"
name: clients
columns:
  id: { type: int }
dummy:
  path: x
  bogus: 1
"#;
    let err = serde_yaml_ng::from_str::<DummySpec>(yaml).unwrap_err();
    assert!(
        err.to_string().contains("bogus") || err.to_string().contains("unknown"),
        "expected typo-detecting error, got: {err}"
    );
}

#[test]
fn reference_sugar_round_trip() {
    let yaml = r#"
name: clients
columns:
  shop_id:
    type: int
    references: shops
dummy:
  path: x
"#;
    let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
    match spec.columns["shop_id"].references.as_ref().unwrap() {
        ReferenceSugar::Sugar(s) => assert_eq!(s, "shops"),
        other => panic!("expected sugar, got {other:?}"),
    }
}

#[test]
fn contained_section_round_trip() {
    let yaml = r#"
name: order
columns:
  id: { type: string, flags: [id] }
  lines: { type: string }
contained:
  lines:
    host_column: lines
    kind: contains_many
    id_column: line_id
    columns:
      line_id: { type: string, flags: [id] }
      product: { type: string }
      quantity: { type: int }
dummy:
  path: x
"#;
    let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
    let c = &spec.contained["lines"];
    assert_eq!(c.host_column, "lines");
    assert_eq!(c.kind, ReferenceKind::ContainsMany);
    assert_eq!(c.id_column.as_deref(), Some("line_id"));
    assert_eq!(c.columns.len(), 3);
    assert_eq!(c.columns["quantity"].col_type.as_deref(), Some("int"));
}

#[test]
fn spec_without_contained_still_parses() {
    let yaml = r#"
name: clients
columns:
  id: { type: int, flags: [id] }
dummy:
  path: x
"#;
    let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
    assert!(spec.contained.is_empty());
}

#[test]
fn full_reference_form() {
    let yaml = r#"
name: orders
columns:
  user_id:
    type: int
    references:
      table: users
      kind: has_one
      foreign_key: user_id
dummy:
  path: x
"#;
    let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
    match spec.columns["user_id"].references.as_ref().unwrap() {
        ReferenceSugar::Full(r) => {
            assert_eq!(r.table, "users");
            assert_eq!(r.kind, ReferenceKind::HasOne);
            assert_eq!(r.foreign_key.as_deref(), Some("user_id"));
        }
        other => panic!("expected full, got {other:?}"),
    }
}

#[test]
fn a_new_spec_declares_only_its_name() {
    let spec = DummySpec::new("clients");
    assert_eq!(spec.name, "clients");
    assert!(spec.datasource.is_none() && spec.id_column.is_none());
    assert!(spec.columns.is_empty() && spec.references.is_empty() && spec.contained.is_empty());
    assert_eq!(spec.driver, DummyTable::default());
}

#[cfg(feature = "rhai")]
#[test]
fn a_lazy_column_keeps_its_declared_type_or_none() {
    let yaml = r#"
name: files
columns:
  lines: { type: int, lazy: "1" }
  shout: { lazy: "\"hi\"" }
dummy:
  path: x
"#;
    let spec: DummySpec = serde_yaml_ng::from_str(yaml).unwrap();
    let typed = spec.columns["lines"].lazy_column("lines").unwrap().unwrap();
    assert_eq!(typed.original_type.as_deref(), Some("int"));
    let untyped = spec.columns["shout"].lazy_column("shout").unwrap().unwrap();
    assert_eq!(untyped.original_type, None);
}
