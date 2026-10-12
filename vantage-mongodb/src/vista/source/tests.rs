//! Offline shell tests. The client connects lazily and these tests never
//! issue a command, so no server is needed.

use bson::{Bson, Document, doc};
use vantage_vista::{Capability, FilterOp, ReferenceKind, SortDirection, VistaFactory};

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
async fn search_and_order_replace_and_clear() {
    let mut vista = client_vista().await;
    vista.set_search(Some("a")).unwrap();
    vista.set_search(Some("b")).unwrap();
    assert!(vista.preview().query.unwrap().contains("<1 conditions>"));
    vista.set_search(None).unwrap();
    assert!(vista.preview().query.unwrap().contains("find({})"));

    let mut shell = shell_of(CLIENT).await;
    shell
        .set_order(Some(("name", SortDirection::Descending)))
        .unwrap();
    shell
        .set_order(Some(("_id", SortDirection::Ascending)))
        .unwrap();
    assert_eq!(
        keyset::effective_sort(&shell.table),
        vec![("_id".into(), 1)]
    );
    shell.set_order(None).unwrap();
    assert!(keyset::effective_sort(&shell.table).is_empty());
}

#[tokio::test]
async fn an_unknown_column_is_a_normal_error() {
    let mut shell = shell_of(CLIENT).await;
    let err = shell
        .add_condition(&Condition::new("nope", FilterOp::Gt, 1))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    let err = shell
        .set_order(Some(("nope", SortDirection::Ascending)))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    assert!(shell.table.conditions().next().is_none());
}

#[tokio::test]
async fn a_dotted_path_under_a_declared_column_filters_by_dot_notation() {
    let spec =
        "{ name: client, columns: { _id: { flags: [id] }, address: { mongo: { field: addr } } } }";
    let mut shell = shell_of(spec).await;
    shell
        .add_condition(&Condition::new("address.city", FilterOp::Eq, "Hill Valley"))
        .unwrap();
    let filter = format!("{:?}", shell.table.conditions().next().unwrap());
    assert!(filter.contains("addr.city"), "{filter}");

    let err = shell
        .add_condition(&Condition::new("nope.city", FilterOp::Eq, "x"))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    assert_eq!(shell.table.conditions().count(), 1);
}

#[tokio::test]
async fn a_shell_over_stored_documents_takes_equality_on_any_field() {
    let spec = serde_yaml_ng::from_str("{ name: bare, columns: {} }").unwrap();
    let bare = factory().await.build_from_spec(spec).unwrap();
    let narrowed = bare
        .clone()
        .with_condition("client_id", FilterOp::Eq, "c1")
        .unwrap();
    assert!(narrowed.is_narrowed());
    let err = bare
        .with_condition("client_id", FilterOp::Gt, "c1")
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn native_conditions_are_recorded_by_key() {
    let vista = client_vista().await;
    let narrowed = vista
        .clone()
        .with_native_condition(NativeCondition::new(doc! { "n": 1 }, "n=1"))
        .unwrap();
    assert_eq!(narrowed.query().native, vec!["n=1".to_string()]);
    let err = vista
        .with_native_condition(NativeCondition::new(1_u8, "u8"))
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
}

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

#[tokio::test]
async fn watch_is_refused_without_subscribe() {
    let vista = client_vista().await;
    let Err(err) = vista.watch().await else {
        panic!("watch without change streams must be refused");
    };
    assert!(err.is_unsupported(), "{err:?}");
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
        false,
        None,
    )
}
