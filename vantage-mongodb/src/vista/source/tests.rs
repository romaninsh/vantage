//! Offline shell tests. The client connects lazily and these tests never
//! issue a command, so no server is needed.
//!
//! - this file: the fixtures, BSON paths, capabilities, clones and watch;
//! - `narrow`: conditions, search and order;
//! - `refs`: reference metadata and has-many narrowing.

mod narrow;
mod refs;

use bson::{Bson, Document};
use vantage_vista::{Capability, FilterOp, VistaFactory};

use super::*;
use crate::vista::factory::{MongoVistaFactory, mongo_capabilities};

#[test]
fn walk_bson_path_returns_nested_value() {
    let mut inner = Document::new();
    inner.insert("city", "Hill Valley");
    let mut outer = IndexMap::new();
    outer.insert("address".to_string(), Bson::Document(inner));

    let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
    assert_eq!(value, Some(Bson::String("Hill Valley".to_string())));
}

#[test]
fn walk_bson_path_missing_segment_yields_none() {
    let mut outer = IndexMap::new();
    outer.insert("address".to_string(), Bson::Document(Document::new()));

    let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
    assert_eq!(value, None);
}

#[test]
fn walk_bson_path_through_scalar_yields_none() {
    let mut outer = IndexMap::new();
    outer.insert("address".to_string(), Bson::String("123 Main".into()));

    let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
    assert_eq!(value, None);
}

const CLIENT: &str = r#"
name: client
columns:
  _id: { type: string, flags: [id] }
  name: { type: string, flags: [title, searchable] }
mongo:
  collection: clients
references:
  orders: { table: order, kind: has_many, foreign_key: client_id }
"#;

async fn factory() -> MongoVistaFactory {
    let client = mongodb::Client::with_uri_str("mongodb://localhost:27017")
        .await
        .unwrap();
    MongoDB::new(client, "unit")
        .vista_factory()
        .with_change_streams(false)
}

async fn client_vista() -> Vista {
    let spec = serde_yaml_ng::from_str(CLIENT).unwrap();
    factory().await.build_from_spec(spec).unwrap()
}

async fn shell_of(yaml: &str) -> MongoTableShell {
    let factory = factory().await;
    let spec = serde_yaml_ng::from_str(yaml).unwrap();
    let table = factory.table_from_spec(&spec).unwrap();
    let paths = crate::vista::factory::paths_from_table_columns(&table);
    let metadata = table.vista_metadata(Default::default());
    MongoTableShell::new(
        table,
        mongo_capabilities(true, false),
        metadata,
        paths,
        None,
        None,
    )
}

#[test]
fn capabilities_follow_writability_and_live_updates() {
    let caps = mongo_capabilities(true, true);
    for capability in Capability::iter() {
        assert!(caps.has(capability), "{capability} on a live collection");
    }
    let quiet = mongo_capabilities(true, false);
    assert!(quiet.has(Capability::Insert) && quiet.has(Capability::ConfineWrites));
    assert!(!quiet.has(Capability::Subscribe) && !quiet.has(Capability::Import));
    let view = mongo_capabilities(false, false);
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Subscribe,
        Capability::Import,
    ] {
        assert!(!view.has(capability), "{capability} on a view");
    }
    assert!(view.has(Capability::Cursor) && view.has(Capability::Traverse));
}

#[tokio::test]
async fn clone_owns_its_narrowing() {
    let vista = client_vista().await;
    let narrowed = vista
        .clone()
        .with_condition("name", FilterOp::Eq, "Marty")
        .unwrap()
        .with_search("ma")
        .unwrap();
    assert_eq!(narrowed.query().conditions.len(), 1);
    assert!(vista.query().is_empty());
    assert!(!vista.preview().query.unwrap().contains("conditions"));
    let preview = narrowed.preview().query.unwrap();
    assert!(preview.contains("<2 conditions>"), "{preview}");
}

#[tokio::test]
async fn watch_is_refused_without_subscribe() {
    let vista = client_vista().await;
    let Err(err) = vista.watch().await else {
        panic!("watch without change streams must be refused");
    };
    assert!(err.is_unsupported(), "{err:?}");
}
