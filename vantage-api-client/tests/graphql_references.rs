//! YAML `references:` on the GraphQL Vista driver: reference metadata, the
//! errors for targets that can't be resolved or built, and isolation of a
//! target from its parent's `graphql:` overrides. Traversal itself is in
//! `graphql_traversal.rs`.

mod support;

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::json;
use vantage_api_client::GraphqlApiVistaSpec;
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{ReferenceKind, VistaFactory};
use wiremock::MockServer;

use support::references::{CLIENT, answer, factory, order_yaml, registered, row, texts};

#[tokio::test]
async fn metadata_carries_reference_targets_and_foreign_keys() {
    let server = MockServer::start().await;
    let factory = registered(&server, "");

    let clients = factory.build("client").unwrap();
    let orders = clients.get_reference("orders").unwrap();
    assert_eq!(orders.target, "order");
    assert_eq!(orders.kind, ReferenceKind::HasMany);
    assert_eq!(orders.foreign_key, "client_id");

    let client = factory.build("order").unwrap();
    let client = client.get_reference("client").unwrap();
    assert_eq!(client.target, "client");
    assert_eq!(client.kind, ReferenceKind::HasOne);
    assert_eq!(client.foreign_key, "client_id");
}

#[tokio::test]
async fn reference_to_an_unregistered_model_is_an_error() {
    let server = MockServer::start().await;
    let mut factory = factory(&server);
    factory.register_yaml(&order_yaml("")).unwrap();
    let orders = factory.build("order").unwrap();
    let o3 = row(&[("id", "o3"), ("client_id", "c2")]);

    for err in [
        orders.get_ref("client", &o3).err().expect("get_ref error"),
        orders.get_ref_target("client").err().expect("target error"),
    ] {
        let msg = err.to_string();
        assert!(msg.contains("no registered spec"), "{msg}");
        assert!(msg.contains("client"), "{msg}");
    }
    assert_eq!(support::requests(&server).await, 0);
}

/// A target spec that doesn't lower fails the traversal, rather than
/// yielding a column-less target whose query the server would reject.
#[tokio::test]
async fn reference_to_a_broken_target_spec_is_an_error() {
    let server = MockServer::start().await;
    let mut factory = factory(&server);
    factory.register_yaml(&order_yaml("")).unwrap();
    factory
        .register_yaml("name: client\ncolumns:\n  id: { type: quaternion, flags: [id] }\n")
        .unwrap();
    let orders = factory.build("order").unwrap();

    let err = orders.get_ref_target("client").err().expect("target error");
    assert!(err.to_string().contains("can't be built"), "{err}");
}

/// An explicit resolver replaces the registry, and a target is lowered onto
/// the factory's API: the parent's root arguments and filter argument do
/// not carry over to the target's query.
#[tokio::test]
async fn with_resolver_targets_ignore_the_parent_overrides() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { clients(where: {id: {_eq: \"c2\"}}) { id name } }",
        "clients",
        json!([{ "id": "c2", "name": "Doc" }]),
    )
    .await;
    let specs: IndexMap<String, GraphqlApiVistaSpec> = [CLIENT.to_string(), order_yaml("")]
        .iter()
        .map(|yaml| serde_yaml_ng::from_str::<GraphqlApiVistaSpec>(yaml).unwrap())
        .map(|spec| (spec.name.clone(), spec))
        .collect();
    let factory = factory(&server).with_resolver(Arc::new(move |name| specs.get(name).cloned()));
    let order: GraphqlApiVistaSpec =
        serde_yaml_ng::from_str(&order_yaml("  filter_arg: by\n  args: { input: {} }\n")).unwrap();
    let orders = factory.build_from_spec(order).unwrap();
    let o3 = row(&[("id", "o3"), ("client_id", "c2")]);

    let client = orders.get_ref("client", &o3).unwrap();
    assert_eq!(texts(&client.list_values().await.unwrap(), "name"), ["Doc"]);
}
