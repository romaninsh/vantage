//! Traversing YAML `references:` on the GraphQL Vista driver: has_one and
//! has_many push an eq-condition into the request, or filter locally with
//! `filter: false`, and a text join value meets an int column as an integer.

mod support;

use serde_json::json;
use vantage_dataset::traits::ReadableValueSet;
use wiremock::MockServer;

use support::references::{
    ALL_ORDERS, ORDERS, all_orders, answer, factory, registered, row, texts,
};

#[tokio::test]
async fn has_one_sends_the_foreign_key_as_an_eq_condition() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { clients(where: {id: {_eq: \"c2\"}}) { id name } }",
        "clients",
        json!([{ "id": "c2", "name": "Doc" }]),
    )
    .await;
    let orders = registered(&server, "").build("order").unwrap();
    let o3 = row(&[("id", "o3"), ("client_id", "c2")]);

    let client = orders.get_ref("client", &o3).unwrap();
    assert_eq!(client.name(), "client");
    assert_eq!(texts(&client.list_values().await.unwrap(), "name"), ["Doc"]);
}

#[tokio::test]
async fn has_many_sends_the_parent_id_as_an_eq_condition() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { orders(where: {client_id: {_eq: \"c1\"}}) { id client_id item } }",
        ORDERS,
        json!([
            { "id": "o1", "client_id": "c1", "item": "skateboard" },
            { "id": "o2", "client_id": "c1", "item": "almanac" }
        ]),
    )
    .await;
    let clients = registered(&server, "").build("client").unwrap();
    let marty = row(&[("id", "c1"), ("name", "Marty")]);

    let orders = clients.get_ref("orders", &marty).unwrap();
    assert_eq!(
        texts(&orders.list_values().await.unwrap(), "item"),
        ["almanac", "skateboard"]
    );
    // The traversed Vista is built from its own spec, references included.
    assert_eq!(orders.get_reference("client").unwrap().target, "client");
}

/// A text foreign key (a row from a form or a script) is sent as an integer
/// literal when the target's id column is an int.
#[tokio::test]
async fn has_one_sends_a_text_key_as_an_integer_for_an_int_id() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { users(where: {id: {_eq: 5}}) { id name } }",
        "users",
        json!([{ "id": 5, "name": "Marty" }]),
    )
    .await;
    let mut factory = factory(&server);
    factory
        .register_yaml(
            "name: users\ncolumns:\n  id: { type: int, flags: [id] }\n  name: { type: string }\ngraphql:\n  dialect: hasura\n",
        )
        .unwrap();
    factory
        .register_yaml(
            "name: tickets\ncolumns:\n  id: { type: int, flags: [id] }\n  user_id: { type: int }\ngraphql:\n  dialect: hasura\nreferences:\n  user:\n    table: users\n    kind: has_one\n    foreign_key: user_id\n",
        )
        .unwrap();
    let tickets = factory.build("tickets").unwrap();
    let ticket = row(&[("id", "1"), ("user_id", "5")]);

    let user = tickets.get_ref("user", &ticket).unwrap();
    assert_eq!(texts(&user.list_values().await.unwrap(), "name"), ["Marty"]);
}

/// With `filter: false` the eq-condition stays out of the request and the
/// listed rows are narrowed locally.
#[tokio::test]
async fn has_many_without_filter_push_down_filters_locally() {
    let server = MockServer::start().await;
    answer(&server, ALL_ORDERS, ORDERS, all_orders()).await;
    let clients = registered(&server, "  filter: false\n")
        .build("client")
        .unwrap();
    let marty = row(&[("id", "c1"), ("name", "Marty")]);

    let orders = clients.get_ref("orders", &marty).unwrap();
    assert_eq!(
        texts(&orders.list_values().await.unwrap(), "item"),
        ["almanac", "skateboard"]
    );
    assert_eq!(orders.get_count().await.unwrap(), 2);
}

/// The bare target of a reference is the target's spec with no condition.
#[tokio::test]
async fn get_ref_target_lists_the_whole_target() {
    let server = MockServer::start().await;
    answer(&server, ALL_ORDERS, ORDERS, all_orders()).await;
    let clients = registered(&server, "").build("client").unwrap();

    let orders = clients.get_ref_target("orders").unwrap();
    assert_eq!(orders.name(), "order");
    assert_eq!(orders.list_values().await.unwrap().len(), 3);
}
