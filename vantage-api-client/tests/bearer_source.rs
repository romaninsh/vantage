//! A bearer source is asked for the token each time a request goes out, so
//! clones of one API can each send their own caller's token over the shared
//! client.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use support::{mount, mount_n, test_client};
use vantage_api_client::{BearerSource, GraphqlApi, Priority, ResponseShape, RestApi};
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use wiremock::MockServer;

async fn list_rows(api: &RestApi) -> vantage_core::Result<usize> {
    let table: Table<RestApi, EmptyEntity> = Table::new("things", api.clone()).with_id_column("id");
    Ok(table.list_values().await?.len())
}

/// A source answering `t0`, `t1`, … — a new token on every ask.
fn counting_source() -> BearerSource {
    let asked = Arc::new(AtomicUsize::new(0));
    Arc::new(move || Some(format!("t{}", asked.fetch_add(1, Ordering::SeqCst))))
}

fn fixed_source(token: &'static str) -> BearerSource {
    Arc::new(move || Some(token.to_string()))
}

/// Every `Authorization` value of every request the server got, in order;
/// a request may carry none, or (wrongly) more than one.
async fn authorizations(server: &MockServer) -> Vec<Vec<String>> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            r.headers
                .get_all("authorization")
                .iter()
                .map(|v| v.to_str().unwrap().to_string())
                .collect()
        })
        .collect()
}

#[tokio::test]
async fn bearer_source_is_asked_on_every_attempt() {
    let server = MockServer::start().await;
    mount_n(&server, "GET", 503, "", 1).await;
    mount(&server, "GET", 200, "[]").await;
    mount(&server, "POST", 200, "").await;
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .bearer_source(counting_source())
        .http_client(test_client())
        .build();

    tokio::time::timeout(
        Duration::from_secs(5),
        Priority::Essential.scope(list_rows(&api)),
    )
    .await
    .expect("finishes")
    .expect("succeeds after the retry");
    api.http_request(reqwest::Method::POST, "things", &[], None)
        .await
        .unwrap();

    assert_eq!(
        authorizations(&server).await,
        [vec!["Bearer t0"], vec!["Bearer t1"], vec!["Bearer t2"]]
    );
}

/// Clones of one API each send their own caller's bearer; `None` sends
/// none, even over the shared API's fixed `auth`, and the shared API keeps
/// its fixed header.
#[tokio::test]
async fn clones_send_their_own_bearer_and_leave_the_original_alone() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let shared = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .auth("Bearer fixed")
        .http_client(test_client())
        .build();
    let alice = shared.with_bearer_source(fixed_source("alice"));
    let bob = shared.with_bearer_source(fixed_source("bob"));
    let signed_out = shared.with_bearer_source(Arc::new(|| None));

    list_rows(&alice).await.unwrap();
    list_rows(&bob).await.unwrap();
    list_rows(&signed_out).await.unwrap();
    // `shared` has sent nothing yet, so a peak on its client means the
    // clones' requests went through it.
    assert_eq!(shared.client().peak_in_flight(), 1);
    list_rows(&shared).await.unwrap();

    assert_eq!(
        authorizations(&server).await,
        [
            vec!["Bearer alice"],
            vec!["Bearer bob"],
            vec![],
            vec!["Bearer fixed"]
        ]
    );
}

#[tokio::test]
async fn graphql_clones_send_their_own_bearer() {
    let server = MockServer::start().await;
    mount(&server, "POST", 200, r#"{"data":{}}"#).await;
    let shared = GraphqlApi::builder(server.uri())
        .http_client(test_client())
        .build();
    let alice = shared.with_bearer_source(fixed_source("alice"));
    let signed_out = shared.with_bearer_source(Arc::new(|| None));
    let per_request = GraphqlApi::builder(server.uri())
        .bearer_source(counting_source())
        .http_client(test_client())
        .build();

    let vars = serde_json::Map::new();
    alice.post_graphql("{ a }", &vars).await.unwrap();
    signed_out.post_graphql("{ a }", &vars).await.unwrap();
    per_request.post_graphql("{ a }", &vars).await.unwrap();
    per_request.post_graphql("{ a }", &vars).await.unwrap();

    assert_eq!(
        authorizations(&server).await,
        [
            vec!["Bearer alice".to_string()],
            vec![],
            vec!["Bearer t0".to_string()],
            vec!["Bearer t1".to_string()],
        ]
    );
}
