//! Offline tests for the YAML factory. They cover parsing, the
//! registry path, capability advertisement, and reference metadata.
//! Network-driven behaviour (URI template substitution, deferred FK
//! resolution) is exercised by `examples/jsonplaceholder_yaml.rs`.

use vantage_api_client::{ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec};
use vantage_vista::{ReferenceKind, VistaFactory};

fn factory() -> RestApiVistaFactory {
    let api = RestApi::builder("https://example.com")
        .response_shape(ResponseShape::BareArray)
        .build();
    RestApiVistaFactory::new(api)
}

#[test]
fn parses_minimal_spec() {
    let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
  name: { type: string, flags: [title] }
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(spec.name, "users");
    assert_eq!(spec.columns.len(), 2);
    assert_eq!(spec.columns["id"].col_type.as_deref(), Some("int"));
    assert!(spec.references.is_empty());
}

#[test]
fn parses_table_endpoint_block() {
    let yaml = r#"
name: nested
api:
  endpoint: parent/{parentId}/nested
columns:
  id: { type: int, flags: [id] }
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(
        spec.driver.api.unwrap().endpoint.as_deref(),
        Some("parent/{parentId}/nested")
    );
}

#[test]
fn parses_reference_metadata() {
    let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
references:
  albums:
    table: albums
    kind: has_many
    foreign_key: userId
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let albums = &spec.references["albums"];
    assert_eq!(albums.table, "albums");
    assert_eq!(albums.kind, ReferenceKind::HasMany);
    assert_eq!(albums.foreign_key.as_deref(), Some("userId"));
}

#[test]
fn rejects_unknown_field_in_api_block() {
    let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
api:
  endpoint: users
  bogus: 1
"#;
    let err = serde_yaml_ng::from_str::<RestApiVistaSpec>(yaml).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("bogus") || msg.contains("unknown"),
        "expected typo-detecting error, got: {msg}"
    );
}

#[test]
fn build_from_spec_harvests_metadata() {
    let factory = factory();
    let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
  name: { type: string, flags: [title] }
  email: { type: string }
references:
  albums:
    table: albums
    kind: has_many
    foreign_key: userId
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let vista = factory.build_from_spec(spec).expect("build");

    assert_eq!(vista.name(), "users");
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.get_title_columns(), vec!["name"]);
    assert_eq!(vista.get_column_names(), vec!["id", "name", "email"]);
    assert_eq!(vista.get_references(), vec!["albums".to_string()]);
    let albums = vista.get_reference("albums").unwrap();
    assert_eq!(albums.target, "albums");
    assert_eq!(albums.foreign_key, "userId");
}

#[test]
fn endpoint_defaults_to_name_when_block_absent() {
    let factory = factory();
    let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let _vista = factory.build_from_spec(spec).expect("build");
    // No public accessor for the inner table's endpoint; absence of
    // an error is the assertion. URL building is exercised by the
    // YAML example end-to-end.
}

#[test]
fn unknown_column_type_errors() {
    let factory = factory();
    let yaml = r#"
name: users
columns:
  id: { type: int, flags: [id] }
  weird: { type: quaternion }
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let err = match factory.build_from_spec(spec) {
        Ok(_) => panic!("expected unknown-type error"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(msg.contains("Unknown YAML column type"), "got: {msg}");
}

#[test]
fn missing_id_column_errors() {
    let factory = factory();
    let yaml = r#"
name: users
id_column: missing_id
columns:
  name: { type: string }
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let err = match factory.build_from_spec(spec) {
        Ok(_) => panic!("expected missing-id error"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("id column not present"),
        "got: {err}"
    );
}

#[test]
fn register_yaml_then_build_by_name() {
    let mut factory = factory();
    factory
        .register_yaml(
            r#"
name: users
columns:
  id: { type: int, flags: [id] }
  name: { type: string, flags: [title] }
"#,
        )
        .unwrap();
    let vista = factory.build("users").expect("build");
    assert_eq!(vista.name(), "users");
}

#[test]
fn build_unknown_name_errors() {
    let factory = factory();
    let err = match factory.build("ghost") {
        Ok(_) => panic!("expected unknown-name error"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("No registered spec"), "got: {err}");
}
