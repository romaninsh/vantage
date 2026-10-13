//! Traversing YAML `references:` on the GraphQL Vista driver: has_one and
//! has_many push an eq-condition into the request, or filter locally with
//! `filter: false`, and a text value meets an int column as an integer,
//! whether it arrives through a join or a condition.

mod support;

use serde_json::json;
use vantage_api_client::GraphqlApiVistaFactory;
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::FilterOp;
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

    let client = orders.get_ref("client", Some(&o3)).unwrap();
    assert_eq!(client.name(), "client");
    assert!(client.is_narrowed(), "the join is recorded in query()");
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

    let orders = clients.get_ref("orders", Some(&marty)).unwrap();
    assert_eq!(
        texts(&orders.list_values().await.unwrap(), "item"),
        ["almanac", "skateboard"]
    );
    assert_eq!(orders.query().conditions[0].column, "client_id");
    // The traversed Vista is built from its own spec, references included.
    assert_eq!(orders.reference("client").unwrap().target, "client");
}

/// `users` and `tickets`, both keyed by int ids; a ticket has one user.
fn int_keyed(server: &MockServer) -> GraphqlApiVistaFactory {
    let mut factory = factory(server);
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
    factory
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
    let tickets = int_keyed(&server).build("tickets").unwrap();
    let ticket = row(&[("id", "1"), ("user_id", "5")]);

    let user = tickets.get_ref("user", Some(&ticket)).unwrap();
    assert_eq!(texts(&user.list_values().await.unwrap(), "name"), ["Marty"]);
}

/// A condition added directly (a script's `self.where("user_id", "5")`)
/// types its text value by the column, as traversal does.
#[tokio::test]
async fn a_text_condition_on_an_int_column_is_sent_as_an_integer() {
    let server = MockServer::start().await;
    answer(
        &server,
        "query { tickets(where: {user_id: {_eq: 5}}) { id user_id } }",
        "tickets",
        json!([{ "id": 1, "user_id": 5 }]),
    )
    .await;
    let tickets = int_keyed(&server)
        .build("tickets")
        .unwrap()
        .with_condition("user_id", FilterOp::Eq, "5")
        .unwrap();
    assert_eq!(tickets.list_values().await.unwrap().len(), 1);

    let err = int_keyed(&server)
        .build("tickets")
        .unwrap()
        .with_condition("user_id", FilterOp::Eq, "five")
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err}");
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

    let orders = clients.get_ref("orders", Some(&marty)).unwrap();
    assert_eq!(
        texts(&orders.list_values().await.unwrap(), "item"),
        ["almanac", "skateboard"]
    );
    assert_eq!(orders.count().await.unwrap(), 2);
}

/// The bare target of a reference is the target's spec with no condition.
#[tokio::test]
async fn bare_ref_lists_the_whole_target() {
    let server = MockServer::start().await;
    answer(&server, ALL_ORDERS, ORDERS, all_orders()).await;
    let clients = registered(&server, "").build("client").unwrap();

    let orders = clients.get_ref("orders", None).unwrap();
    assert_eq!(orders.name(), "order");
    assert!(!orders.is_narrowed());
    assert_eq!(orders.list_values().await.unwrap().len(), 3);
}
