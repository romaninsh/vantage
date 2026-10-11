//! Fixtures for the GraphQL reference tests: a `client` / `order` pair of
//! specs referencing each other, and a wiremock mount that only answers the
//! expected query, so a traversal rendering the wrong condition fails the
//! fetch rather than passing on a canned reply.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::{Value, json};
use vantage_api_client::{GraphqlApi, GraphqlApiVistaFactory};
use vantage_types::Record;
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const CLIENT: &str = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
graphql:
  root_field: clients
  dialect: hasura
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
"#;

/// `order` on `orders`, with `extra` lines appended to its `graphql:` block.
pub fn order_yaml(extra: &str) -> String {
    format!(
        r#"
name: order
columns:
  id: {{ type: string, flags: [id] }}
  client_id: {{ type: string }}
  item: {{ type: string, flags: [title] }}
graphql:
  root_field: orders
  dialect: hasura
{extra}references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#
    )
}

pub fn factory(server: &MockServer) -> GraphqlApiVistaFactory {
    GraphqlApi::builder(server.uri())
        .http_client(super::test_client())
        .build()
        .vista_factory()
}

/// A factory with `client` and `order` (built from `order_yaml(order_extra)`)
/// in its registry.
pub fn registered(server: &MockServer, order_extra: &str) -> GraphqlApiVistaFactory {
    let mut factory = factory(server);
    factory.register_yaml(CLIENT).unwrap();
    factory.register_yaml(&order_yaml(order_extra)).unwrap();
    factory
}

/// Answer only requests whose JSON body carries `query`.
pub async fn answer(server: &MockServer, query: &str, root: &str, rows: Value) {
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "query": query })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { (root): rows } })))
        .mount(server)
        .await;
}

/// A parent row as a caller hands it over, every value as text.
pub fn row(fields: &[(&str, &str)]) -> Record<CborValue> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
        .collect()
}

/// The text values of `key` across `rows`, sorted.
pub fn texts(rows: &IndexMap<String, Record<CborValue>>, key: &str) -> Vec<String> {
    let mut out: Vec<String> = rows
        .values()
        .filter_map(|r| match r.get(key) {
            Some(CborValue::Text(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

pub const ORDERS: &str = "orders";
pub const ALL_ORDERS: &str = "query { orders { id client_id item } }";

pub fn all_orders() -> Value {
    json!([
        { "id": "o1", "client_id": "c1", "item": "skateboard" },
        { "id": "o2", "client_id": "c1", "item": "almanac" },
        { "id": "o3", "client_id": "c2", "item": "flux capacitor" }
    ])
}
