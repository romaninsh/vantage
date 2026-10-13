//! `vantage_vista::contract` over a REST API served by wiremock: a paging,
//! orderable `client` / `order` pair that reference each other.

mod support;

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_api_client::{
    OrderingParams, PaginationParams, ResponseShape, RestApi, RestApiVistaFactory,
};
use vantage_vista::contract;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const CLIENT: &str = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title] }
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
"#;

fn page(rows: Value) -> ResponseTemplate {
    let count = rows.as_array().map_or(0, Vec::len);
    ResponseTemplate::new(200).set_body_json(json!({ "count": count, "results": rows }))
}

/// Clients `c1`, `c2`; orders `o1`, `o2` (c1) and `o3` (c2). A request for
/// `client_id=c1` gets c1's orders only, and `ordering=-id` reverses a list.
async fn serve() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/order"))
        .and(query_param("ordering", "-id"))
        .respond_with(page(json!([
            { "id": "o3", "client_id": "c2", "item": "flux capacitor" },
            { "id": "o2", "client_id": "c1", "item": "almanac" },
            { "id": "o1", "client_id": "c1", "item": "skateboard" }
        ])))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/client"))
        .and(query_param("ordering", "-id"))
        .respond_with(page(json!([
            { "id": "c2", "name": "Doc" },
            { "id": "c1", "name": "Marty" }
        ])))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/order"))
        .and(query_param("client_id", "c1"))
        .respond_with(page(json!([
            { "id": "o1", "client_id": "c1", "item": "skateboard" },
            { "id": "o2", "client_id": "c1", "item": "almanac" }
        ])))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/order"))
        .respond_with(paged(json!([
            { "id": "o1", "client_id": "c1", "item": "skateboard" },
            { "id": "o2", "client_id": "c1", "item": "almanac" },
            { "id": "o3", "client_id": "c2", "item": "flux capacitor" }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/client"))
        .respond_with(paged(json!([
            { "id": "c1", "name": "Marty" },
            { "id": "c2", "name": "Doc" }
        ])))
        .mount(&server)
        .await;
    server
}

/// A paging server over `rows`: `offset`/`limit` slice them, and `count` is
/// always the whole set's.
fn paged(rows: Value) -> impl Fn(&Request) -> ResponseTemplate {
    move |request: &Request| {
        let rows = rows.as_array().expect("rows are an array");
        let param = |name: &str| {
            request
                .url
                .query_pairs()
                .find(|(key, _)| key == name)
                .and_then(|(_, value)| value.parse::<usize>().ok())
        };
        let offset = param("offset").unwrap_or(0).min(rows.len());
        let limit = param("limit").unwrap_or(rows.len());
        let window: Vec<&Value> = rows.iter().skip(offset).take(limit).collect();
        ResponseTemplate::new(200).set_body_json(json!({ "count": rows.len(), "results": window }))
    }
}

fn factory(server: &MockServer) -> RestApiVistaFactory {
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::Wrapped {
            array_key: "results".into(),
        })
        .total_key("count")
        .pagination_params(PaginationParams::skip_limit("offset", "limit"))
        .ordering(OrderingParams::new("ordering", "-"))
        .http_client(support::test_client())
        .build();
    let mut factory = RestApiVistaFactory::new(api);
    factory.register_yaml(CLIENT).unwrap();
    factory.register_yaml(ORDER).unwrap();
    factory
}

#[tokio::test]
async fn rest_contract_advertised() {
    let server = serve().await;
    let factory = factory(&server);
    contract::check_advertised(&factory.build("client").unwrap()).await;
    contract::check_advertised(&factory.build("order").unwrap()).await;
}

#[tokio::test]
async fn rest_contract_clone() {
    let server = serve().await;
    let orders = factory(&server).build("order").unwrap();
    contract::check_clone(&orders, "client_id", CborValue::Text("c1".into())).await;
}
