//! `vantage_vista::contract` over a Hasura-dialect GraphQL endpoint served by
//! wiremock: a paging `client` / `order` pair that reference each other.

mod support;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_api_client::{GraphqlApi, GraphqlApiVistaFactory};
use vantage_vista::contract::{self, TxFixture, TxTables};
use wiremock::matchers::{body_string_contains, method};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

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

/// Answer a query on `root` whose body holds every one of `needles`.
async fn answer(server: &MockServer, root: &str, needles: &[&str], rows: Value) {
    let mut mock = Mock::given(method("POST")).and(body_string_contains(root));
    for needle in needles {
        mock = mock.and(body_string_contains(*needle));
    }
    mock.respond_with(reply(root, rows))
        .with_priority(if needles.is_empty() { 5 } else { 1 })
        .mount(server)
        .await;
}

/// Clients `c1`, `c2`; orders `o1`, `o2` (c1) and `o3` (c2). An `orders`
/// query naming `c1` gets c1's orders only. The contract's probes are
/// answered too: a search for its unmatchable needle finds nothing, a
/// descending order reverses the rows and `id > <first id>` drops the first.
async fn serve() -> MockServer {
    let server = MockServer::start().await;
    let (o1, o2, o3) = (
        json!({ "id": "o1", "client_id": "c1", "item": "skateboard" }),
        json!({ "id": "o2", "client_id": "c1", "item": "almanac" }),
        json!({ "id": "o3", "client_id": "c2", "item": "flux capacitor" }),
    );
    let (c1, c2) = (
        json!({ "id": "c1", "name": "Marty" }),
        json!({ "id": "c2", "name": "Doc" }),
    );
    for root in ["orders", "clients"] {
        answer(&server, root, &["3f9c2a7e"], json!([])).await;
    }
    answer(&server, "orders", &["id: desc"], json!([o3, o2, o1])).await;
    answer(&server, "orders", &["_gt"], json!([o2, o3])).await;
    answer(&server, "orders", &["c1"], json!([o1, o2])).await;
    paged(&server, "orders", json!([o1, o2, o3])).await;
    answer(&server, "clients", &["id: desc"], json!([c2, c1])).await;
    answer(&server, "clients", &["_gt"], json!([c2])).await;
    paged(&server, "clients", json!([c1, c2])).await;
    server
}

/// Answer any other query on `root` as a paging server over `rows`: the
/// `offset`/`limit` variables slice them.
async fn paged(server: &MockServer, root: &'static str, rows: Value) {
    let respond = move |request: &Request| {
        let body: Value = request.body_json().expect("a JSON request body");
        let rows = rows.as_array().expect("rows are an array");
        let variable = |name: &str| body["variables"][name].as_u64().map(|n| n as usize);
        let offset = variable("offset").unwrap_or(0).min(rows.len());
        let limit = variable("limit").unwrap_or(rows.len());
        let window: Vec<&Value> = rows.iter().skip(offset).take(limit).collect();
        reply(root, json!(window))
    };
    Mock::given(method("POST"))
        .and(body_string_contains(root))
        .respond_with(respond)
        .with_priority(5)
        .mount(server)
        .await;
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

/// A GraphQL API has no transactions, and the backend takes no writes.
struct Api(MockServer);

#[async_trait]
impl TxFixture for Api {
    async fn fresh(&self) -> TxTables {
        let factory = factory(&self.0);
        TxTables::new(
            factory.build("client").unwrap(),
            factory.build("order").unwrap(),
        )
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }

    fn detects_deadlocks(&self) -> bool {
        false
    }

    fn supports_transactions(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn graphql_transactions_hold_the_contract() {
    contract::check_transactions(&Api(serve().await)).await
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn graphql_atomic_holds_the_contract() {
    contract::check_atomic_rhai(&Api(serve().await)).await
}
