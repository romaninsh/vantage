//! The URL a REST read sends, and the preview of it: path placeholders,
//! `?field=value` conditions, paging parameters, the sort parameter and
//! what a preview says about conditions it cannot render.

mod support;

use ciborium::Value as CborValue;
use support::{mount, requests, test_client};
use vantage_api_client::{
    FilterStrategy, OrderingParams, PaginationParams, ResponseShape, RestApi, RestApiBuilder,
    RestApiVistaFactory, RestApiVistaSpec, eq_condition,
};
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::Expression;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{FilterOp, SortDirection, Vista, VistaFactory};
use wiremock::MockServer;

fn bare(server: &MockServer) -> RestApiBuilder {
    RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
}

fn table(name: &str, api: RestApi) -> Table<RestApi, EmptyEntity> {
    Table::new(name, api).with_id_column("id")
}

/// The path and query of the one request the server received, the query's
/// parameters sorted so the order they are sent in does not matter.
async fn sent(server: &MockServer) -> (String, Option<String>) {
    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1, "exactly one request");
    let url = &received[0].url;
    let query = url.query().map(|q| {
        let mut params: Vec<&str> = q.split('&').collect();
        params.sort_unstable();
        params.join("&")
    });
    (url.path().to_string(), query)
}

fn vista(api: RestApi, yaml: &str) -> Vista {
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    RestApiVistaFactory::new(api).build_from_spec(spec).unwrap()
}

const USERS: &str =
    "name: users\ncolumns:\n  id: { type: int, flags: [id] }\n  kind: { type: string }\n";

/// An eq-condition with a scalar value becomes one URL-encoded `field=value`
/// param; an array, a null, or a condition that is not an equality can't be
/// one param and is not sent.
#[tokio::test]
async fn eq_conditions_become_query_params() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let mut users = table("users", bare(&server).build());
    users.add_condition(eq_condition("status", "active"));
    users.add_condition(eq_condition("title", "a b&c=d"));
    users.add_condition(eq_condition("done", true));
    users.add_condition(eq_condition("n", 7i64));
    users.add_condition(eq_condition("ratio", 1.5f64));
    users.add_condition(eq_condition("tags", vec![CborValue::Text("a".into())]));
    users.add_condition(eq_condition("gone", CborValue::Null));
    users.add_condition(Expression::<CborValue>::new("CUSTOM SQL", vec![]));
    users.list_values().await.unwrap();
    assert_eq!(
        sent(&server).await,
        (
            "/users".into(),
            Some("done=true&n=7&ratio=1.5&status=active&title=a%20b%26c%3Dd".into())
        )
    );
}

#[test]
fn a_rest_vista_reports_its_driver() {
    let users = vista(RestApi::new("http://x"), USERS);
    assert_eq!(users.driver_name(), "rest-api");
}

#[tokio::test]
async fn placeholders_are_filled_from_conditions_and_those_conditions_are_used_up() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let mut albums = table("teams/{team}/users/{userId}/albums", bare(&server).build());
    albums.add_condition(eq_condition("userId", 1i64));
    albums.add_condition(eq_condition("title", "a b"));
    albums.add_condition(eq_condition("team", "x/y"));
    albums.list_values().await.unwrap();
    assert_eq!(
        sent(&server).await,
        (
            "/teams/x%2Fy/users/1/albums".into(),
            Some("title=a%20b".into())
        )
    );
}

#[tokio::test]
async fn a_placeholder_without_a_condition_is_an_error_and_nothing_is_sent() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let albums = table("users/{userId}/albums", bare(&server).build());
    let err = albums.list_values().await.unwrap_err().to_string();
    assert!(
        err.contains("No eq-condition provided for URI placeholder"),
        "{err}"
    );
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn an_unclosed_placeholder_is_an_error_and_nothing_is_sent() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let users = table("users/{userId", bare(&server).build());
    assert!(users.list_values().await.is_err());
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn a_page_window_is_addressed_by_one_based_page() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let api = bare(&server)
        .pagination_params(PaginationParams::page_limit("_page", "_limit"))
        .build();
    vista(api, USERS).fetch_window(20, 10).await.unwrap();
    assert_eq!(
        sent(&server).await,
        ("/users".into(), Some("_limit=10&_page=3".into()))
    );
}

#[tokio::test]
async fn a_skip_window_sends_the_offset_verbatim() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let api = bare(&server)
        .pagination_params(PaginationParams::skip_limit("skip", "limit"))
        .build();
    vista(api, USERS).fetch_window(20, 10).await.unwrap();
    assert_eq!(
        sent(&server).await,
        ("/users".into(), Some("limit=10&skip=20".into()))
    );
}

#[tokio::test]
async fn paging_the_sort_and_the_conditions_travel_together() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let api = bare(&server)
        .pagination_params(PaginationParams::skip_limit("skip", "limit"))
        .ordering(OrderingParams::new("sort", "-"))
        .build();
    let mut users = vista(api, USERS);
    users.add_condition("kind", FilterOp::Eq, "admin").unwrap();
    users
        .set_order(Some(("id", SortDirection::Ascending)))
        .unwrap();
    users.fetch_window(0, 5).await.unwrap();
    assert_eq!(
        sent(&server).await,
        (
            "/users".into(),
            Some("kind=admin&limit=5&skip=0&sort=id".into())
        )
    );
}

#[tokio::test]
async fn descending_sorts_carry_the_prefix_and_ascending_ones_do_not() {
    for (direction, expected) in [
        (SortDirection::Descending, "sort=-id"),
        (SortDirection::Ascending, "sort=id"),
    ] {
        let server = MockServer::start().await;
        mount(&server, "GET", 200, "[]").await;
        let api = bare(&server)
            .ordering(OrderingParams::new("sort", "-"))
            .build();
        let mut users = vista(api, USERS);
        users.set_order(Some(("id", direction))).unwrap();
        users.list_values().await.unwrap();
        assert_eq!(
            sent(&server).await,
            ("/users".into(), Some(expected.into()))
        );
    }
}

#[tokio::test]
async fn a_table_path_with_a_query_string_joins_the_params_with_an_ampersand() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let api = bare(&server)
        .pagination_params(PaginationParams::skip_limit("offset", "limit"))
        .build();
    let launches = vista(
        api,
        "name: launches\napi:\n  endpoint: \"launches/?mode=detailed\"\ncolumns:\n  id: { type: int, flags: [id] }\n",
    );
    launches.fetch_window(0, 1).await.unwrap();
    assert_eq!(
        sent(&server).await,
        (
            "/launches/".into(),
            Some("limit=1&mode=detailed&offset=0".into())
        )
    );
}

#[tokio::test]
async fn a_table_path_with_a_query_string_is_left_alone_without_params() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let launches = table("launches/?mode=detailed", bare(&server).build());
    launches.list_values().await.unwrap();
    assert_eq!(
        sent(&server).await,
        ("/launches/".into(), Some("mode=detailed".into()))
    );
}

#[tokio::test]
async fn no_pagination_sends_no_paging_params() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let api = bare(&server).no_pagination().build();
    vista(api, USERS).fetch_window(0, 10).await.unwrap();
    assert_eq!(sent(&server).await, ("/users".into(), None));
}

#[tokio::test]
async fn the_client_filter_strategy_keeps_conditions_off_the_wire() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        200,
        r#"[{"id":1,"kind":"a"},{"id":2,"kind":"b"}]"#,
    )
    .await;
    let api = bare(&server)
        .filter_strategy(FilterStrategy::Client)
        .build();
    let mut users = vista(api, USERS);
    users.add_condition("kind", FilterOp::Eq, "b").unwrap();
    let ids: Vec<String> = users.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["2"]);
    assert_eq!(sent(&server).await, ("/users".into(), None));
}

// ── Preview ──────────────────────────────────────────────────────────

fn offline() -> RestApiBuilder {
    RestApi::builder("http://x")
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
}

#[test]
fn the_preview_is_the_get_with_conditions_as_params() {
    let mut users = vista(offline().build(), USERS);
    users.add_condition("kind", FilterOp::Eq, "admin").unwrap();
    let preview = users.preview();
    assert_eq!(
        preview.query.as_deref(),
        Some("GET http://x/users?kind=admin")
    );
    assert_eq!(preview.note, None);
}

#[test]
fn the_preview_masks_the_auth_header() {
    let api = offline().auth("Bearer secret-token").build();
    let preview = vista(api, USERS).preview();
    let query = preview.query.unwrap();
    assert!(
        query.starts_with("GET http://x/users\nAuthorization: "),
        "{query}"
    );
    assert!(!query.contains("secret-token"), "{query}");

    let open = vista(offline().build(), USERS).preview();
    assert_eq!(open.query.as_deref(), Some("GET http://x/users"));
}

#[test]
fn the_preview_fills_placeholders_the_way_the_fetch_does() {
    let mut albums = vista(
        offline().build(),
        "name: albums\napi:\n  endpoint: users/{userId}/albums\ncolumns:\n  id: { type: int, flags: [id] }\n  userId: { type: int }\n  title: { type: string }\n",
    );
    albums.add_condition("userId", FilterOp::Eq, 1i64).unwrap();
    albums.add_condition("title", FilterOp::Eq, "a b").unwrap();
    assert_eq!(
        albums.preview().query.as_deref(),
        Some("GET http://x/users/1/albums?title=a%20b")
    );
}

#[test]
fn the_preview_names_the_filters_applied_after_the_fetch() {
    let api = offline().filter_strategy(FilterStrategy::Client).build();
    let mut users = vista(api, USERS);
    users.add_condition("kind", FilterOp::Eq, "admin").unwrap();
    let preview = users.preview();
    assert_eq!(preview.query.as_deref(), Some("GET http://x/users"));
    let note = preview.note.expect("a note");
    assert!(note.contains("kind=admin"), "{note}");
}

#[test]
fn a_placeholder_nothing_fills_is_reported_not_rendered() {
    let albums = vista(
        offline().build(),
        "name: albums\napi:\n  endpoint: users/{userId}/albums\ncolumns:\n  id: { type: int, flags: [id] }\n",
    );
    let preview = albums.preview();
    assert_eq!(preview.query, None);
    let note = preview.note.expect("a note");
    assert!(
        note.contains("No eq-condition provided for URI placeholder"),
        "{note}"
    );
}

#[test]
fn a_deferred_condition_shows_the_template_and_is_counted() {
    let api = offline().build();
    let users = Table::<RestApi, EmptyEntity>::new("users", api.clone())
        .with_id_column("id")
        .with_many("albums", "userId", |api| {
            Table::<RestApi, EmptyEntity>::new("users/{userId}/albums", api).with_id_column("id")
        });
    let albums = users.get_ref_as::<EmptyEntity>("albums").unwrap();
    let preview = api.vista_factory().from_table(albums).unwrap().preview();

    assert_eq!(
        preview.query.as_deref(),
        Some("GET http://x/users/{userId}/albums")
    );
    let note = preview.note.expect("a note");
    assert!(note.contains("unresolved"), "{note}");
}

#[test]
fn a_deferred_condition_without_a_placeholder_is_counted_in_the_note() {
    let api = offline().build();
    let users = Table::<RestApi, EmptyEntity>::new("users", api.clone())
        .with_id_column("id")
        .with_many("posts", "userId", |api| {
            Table::<RestApi, EmptyEntity>::new("posts", api).with_id_column("id")
        });
    let posts = users.get_ref_as::<EmptyEntity>("posts").unwrap();
    let preview = api.vista_factory().from_table(posts).unwrap().preview();

    assert_eq!(preview.query.as_deref(), Some("GET http://x/posts"));
    let note = preview.note.expect("a note");
    assert!(note.contains("not rendered"), "{note}");
}
