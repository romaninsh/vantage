//! Offline tests for the YAML factory. They cover parsing, the
//! registry path, capability advertisement, and reference metadata.
//! Network-driven behaviour (URI template substitution, deferred FK
//! resolution) is exercised by `examples/jsonplaceholder_yaml.rs`.

mod support;

use vantage_api_client::{ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec};
use vantage_dataset::traits::WritableValueSet;
use vantage_vista::{Capability, JoinKey, ReferenceKind, VistaFactory};

fn factory() -> RestApiVistaFactory {
    let api = RestApi::builder("https://example.com")
        .response_shape(ResponseShape::BareArray)
        .http_client(support::test_client())
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
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.title_columns(), vec!["name"]);
    let columns: Vec<&str> = vista.columns().keys().map(String::as_str).collect();
    assert_eq!(columns, vec!["id", "name", "email"]);
    let references: Vec<&str> = vista.references().keys().map(String::as_str).collect();
    assert_eq!(references, vec!["albums"]);
    let albums = vista.reference("albums").unwrap();
    assert_eq!(albums.target, "albums");
    assert_eq!(albums.keys, vec![JoinKey::new("id", "userId")]);
}

/// A has-one reads its foreign key off the parent row and matches it to the
/// target's id; multi-column `keys:` carry over as given.
#[test]
fn build_from_spec_lowers_join_keys() {
    let factory = factory();
    let yaml = r#"
name: deployments
columns:
  id: { type: int, flags: [id] }
  product_id: { type: int }
  version_id: { type: int }
references:
  product:
    table: products
    kind: has_one
    foreign_key: product_id
  release:
    table: releases
    kind: has_many
    keys:
      - { from: product_id, to: product_id }
      - { from: version_id, to: version_id }
"#;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    let vista = factory.build_from_spec(spec).expect("build");
    assert_eq!(
        vista.reference("product").unwrap().keys,
        vec![JoinKey::to_target_id("product_id")]
    );
    assert_eq!(
        vista.reference("release").unwrap().keys,
        vec![
            JoinKey::new("product_id", "product_id"),
            JoinKey::new("version_id", "version_id")
        ]
    );
}

/// The REST shell only reads: no write capability is advertised, so the
/// Vista refuses writes before any request.
#[tokio::test]
async fn writes_are_refused_by_the_vista() {
    let factory = factory();
    let spec: RestApiVistaSpec =
        serde_yaml_ng::from_str("name: users\ncolumns:\n  id: { type: int, flags: [id] }\n")
            .unwrap();
    let vista = factory.build_from_spec(spec).expect("build");
    for write in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::Import,
        Capability::ConfineWrites,
    ] {
        assert!(!vista.capabilities().has(write), "{write}");
    }
    assert!(!vista.capabilities().has(Capability::Cursor));
    let err = vista
        .insert_value("1", &Default::default())
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(vista.delete("1").await.unwrap_err().is_unsupported());
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
    assert!(err.is_not_found(), "got: {err}");
}

/// A reference to a model the registry doesn't hold fails the traversal as
/// not-found, the resolver contract.
#[test]
fn a_reference_to_an_unregistered_model_is_not_found() {
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(
        r#"
name: users
columns:
  id: { type: int, flags: [id] }
references:
  posts: { table: posts, kind: has_many, foreign_key: userId }
"#,
    )
    .unwrap();
    let vista = factory().build_from_spec(spec).expect("build");
    let err = vista.get_ref("posts", None).unwrap_err();
    assert!(err.is_not_found(), "got: {err}");
    let err = vista.get_ref("comments", None).unwrap_err();
    assert!(err.is_not_found(), "got: {err}");
}
