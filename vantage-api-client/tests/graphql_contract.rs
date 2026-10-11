//! `vantage_vista::contract` over a Hasura-dialect GraphQL endpoint served by
//! wiremock: a paging `client` / `order` pair that reference each other.

mod support;

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_api_client::{GraphqlApi, GraphqlApiVistaFactory};
use vantage_vista::contract;
use wiremock::matchers::{body_string_contains, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT: &str = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
graphql:
  root_field: clients
  dialect: hasura
  paginate: true
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
"#;

const ORDER: &str = r#"
name: order
columns:
  id: { type: string, flags: [id] }
  client_id: { type: string }
  item: { type: string, flags: [title] }
graphql:
  root_field: orders
  dialect: hasura
  paginate: true
"#;

fn reply(root: &str, rows: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "data": { (root): rows } }))
}

/// Clients `c1`, `c2`; orders `o1`, `o2` (c1) and `o3` (c2). An `orders`
/// query naming `c1` gets c1's orders only.
async fn serve() -> MockServer {
    let server = MockServer::start().await;
    let (o1, o2, o3) = (
        json!({ "id": "o1", "client_id": "c1", "item": "skateboard" }),
        json!({ "id": "o2", "client_id": "c1", "item": "almanac" }),
        json!({ "id": "o3", "client_id": "c2", "item": "flux capacitor" }),
    );
    Mock::given(method("POST"))
        .and(body_string_contains("orders"))
        .and(body_string_contains("c1"))
        .respond_with(reply("orders", json!([o1, o2])))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_string_contains("orders"))
        .respond_with(reply("orders", json!([o1, o2, o3])))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_string_contains("clients"))
        .respond_with(reply(
            "clients",
            json!([{ "id": "c1", "name": "Marty" }, { "id": "c2", "name": "Doc" }]),
        ))
        .mount(&server)
        .await;
    server
}

fn factory(server: &MockServer) -> GraphqlApiVistaFactory {
    let mut factory = GraphqlApi::builder(server.uri())
        .http_client(support::test_client())
        .build()
        .vista_factory();
    factory.register_yaml(CLIENT).unwrap();
    factory.register_yaml(ORDER).unwrap();
    factory
}

#[tokio::test]
async fn graphql_contract_advertised() {
    let server = serve().await;
    let factory = factory(&server);
    contract::check_advertised(&factory.build("client").unwrap()).await;
    contract::check_advertised(&factory.build("order").unwrap()).await;
}

#[tokio::test]
async fn graphql_contract_clone() {
    let server = serve().await;
    let orders = factory(&server).build("order").unwrap();
    contract::check_clone(&orders, "client_id", CborValue::Text("c1".into())).await;
}
