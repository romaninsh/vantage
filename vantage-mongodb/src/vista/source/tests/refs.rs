//! Reference metadata and has-many narrowing.

#[cfg(feature = "rhai")]
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{ReferenceKind, VistaFactory};

use super::{client_vista, factory};
#[cfg(feature = "rhai")]
use crate::vista::spec::MongoVistaSpec;

#[tokio::test]
async fn bare_target_and_has_many_narrowing() {
    let vista = client_vista().await;
    let orders = vista.reference("orders").unwrap();
    assert_eq!(orders.kind, ReferenceKind::HasMany);
    // The spec name a resolver knows, not the collection or a Rust type.
    assert_eq!(orders.target, "order");

    let bare = vista.get_ref("orders", None).unwrap();
    assert!(!bare.is_narrowed());

    let mut row = Record::new();
    row.insert("_id".to_string(), CborValue::Text("c1".into()));
    let target = vista.get_ref("orders", Some(&row)).unwrap();
    assert!(target.is_narrowed());
    let condition = &target.query().conditions[0];
    assert_eq!(condition.column, "client_id");
    assert_eq!(condition.value, CborValue::Text("c1".into()));

    assert!(vista.get_ref("nope", None).is_err());
}

/// A spec reference is named by the spec it targets, not the collection that
/// spec reads, and carries its `mongo: { rhai: }` build script.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn spec_reference_targets_the_spec_name_with_its_script() {
    let order: MongoVistaSpec = serde_yaml_ng::from_str(
        r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
mongo:
  collection: orders
"#,
    )
    .unwrap();
    let client = serde_yaml_ng::from_str(
        r#"
name: client
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: clients
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo: { rhai: 'table("order")' }
"#,
    )
    .unwrap();
    let vista = factory()
        .await
        .with_resolver(Arc::new(move |name: &str| {
            (name == "order").then(|| order.clone())
        }))
        .build_from_spec(client)
        .unwrap();
    let orders = vista.reference("orders").unwrap();
    assert_eq!(orders.target, "order");
    assert_eq!(orders.build_script.as_deref(), Some(r#"table("order")"#));
}

/// A column-level `references:` reaches the metadata as a has-one keyed by
/// its column.
#[tokio::test]
async fn column_reference_reaches_the_metadata() {
    let spec = serde_yaml_ng::from_str(
        r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string, references: client }
mongo:
  collection: orders
"#,
    )
    .unwrap();
    let vista = factory().await.build_from_spec(spec).unwrap();
    let client = vista.reference("client_id").unwrap();
    assert_eq!(client.kind, ReferenceKind::HasOne);
    assert_eq!(client.target, "client");
    assert_eq!(
        client.keys,
        [vantage_vista::JoinKey::to_target_id("client_id")]
    );
}
