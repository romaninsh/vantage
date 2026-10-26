//! A Vista over a REST API: response shapes and totals, counting,
//! traversal along `references:`, the capabilities it advertises, and the
//! column types a spec accepts.

mod support;

use std::sync::Arc;

#[cfg(feature = "rhai")]
use ciborium::Value as CborValue;
use serde_json::json;
use support::references::{row, texts};
use support::{mount, test_client};
use vantage_api_client::{
    PaginationParams, ResponseShape, RestApi, RestApiBuilder, RestApiVistaFactory, RestApiVistaSpec,
};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_vista::{Capability, Vista, VistaFactory, VistaResolver};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn builder(server: &MockServer) -> RestApiBuilder {
    RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
}

fn vista(api: RestApi, yaml: &str) -> Vista {
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    RestApiVistaFactory::new(api).build_from_spec(spec).unwrap()
}

const PRODUCTS: &str = "name: products\ncolumns:\n  id: { type: int, flags: [id] }\n";

#[tokio::test]
async fn a_response_wrapped_by_the_table_name_is_read_with_its_total() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products"))
        .and(query_param("skip", "0"))
        .and(query_param("limit", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "products": [{ "id": 1 }, { "id": 2 }],
            "total": 30, "skip": 0, "limit": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let api = builder(&server)
        .response_shape(ResponseShape::WrappedByTableName)
        .total_key("total")
        .pagination_params(PaginationParams::skip_limit("skip", "limit"))
        .build();

    let page = vista(api, PRODUCTS).fetch_window(0, 2).await.unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.total, Some(30));
}

#[tokio::test]
async fn the_endpoint_defaults_to_the_spec_name_and_an_api_block_replaces_it() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    vista(builder(&server).build(), PRODUCTS)
        .list_values()
        .await
        .unwrap();
    vista(
        builder(&server).build(),
        "name: items\napi:\n  endpoint: catalog/items\ncolumns:\n  id: { type: int, flags: [id] }\n",
    )
    .list_values()
    .await
    .unwrap();

    let received = server.received_requests().await.unwrap();
    let paths: Vec<&str> = received.iter().map(|r| r.url.path()).collect();
    assert_eq!(paths, ["/products", "/catalog/items"]);
}

#[tokio::test]
async fn a_count_asks_for_a_one_row_window_and_reads_the_total() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 7, "results": [{ "id": 1 }]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let api = builder(&server)
        .response_shape(ResponseShape::Wrapped {
            array_key: "results".into(),
        })
        .total_key("count")
        .pagination_params(PaginationParams::skip_limit("offset", "limit"))
        .build();

    assert_eq!(vista(api, PRODUCTS).count().await.unwrap(), 7);
}

#[tokio::test]
async fn without_a_total_key_a_count_counts_what_came_back() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, r#"[{"id":1},{"id":2},{"id":3}]"#).await;
    assert_eq!(
        vista(builder(&server).build(), PRODUCTS)
            .count()
            .await
            .unwrap(),
        3
    );
}

/// The shell's list, count and window call the API directly, so each checks
/// the hook first, before any request goes out.
#[tokio::test]
async fn before_select_refuses_vista_list_get_count_and_window() {
    use vantage_table::table::{Hook, SelectCx, Table, select_hook};
    use vantage_types::EmptyEntity;

    let server = MockServer::start().await;
    let api = builder(&server)
        .total_key("count")
        .pagination_params(PaginationParams::skip_limit("offset", "limit"))
        .build();
    let table = Table::<RestApi, EmptyEntity>::new("products", api.clone())
        .with_id_column("id")
        .with_hook(Hook::BeforeSelect(select_hook(
            |_cx: SelectCx<'_, RestApi>| {
                Box::pin(async { Err(vantage_core::error!("read refused")) })
            },
        )));
    let products = RestApiVistaFactory::new(api).from_table(table).unwrap();

    let refused = |e: vantage_core::VantageError| e.to_string().contains("read refused");
    assert!(products.list_values().await.is_err_and(refused));
    assert!(products.get_value("1").await.is_err_and(refused));
    assert!(products.count().await.is_err_and(refused));
    assert!(products.fetch_window(0, 2).await.is_err_and(refused));
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// The REST shell only reads: it traverses, but advertises no search, no
/// cursor and no write, and refuses them before any request.
#[tokio::test]
async fn a_rest_vista_traverses_and_reads_but_neither_searches_nor_writes() {
    let server = MockServer::start().await;
    let mut products = vista(builder(&server).build(), PRODUCTS);
    let caps = products.capabilities();
    assert!(caps.has(Capability::Traverse));
    assert!(!caps.has(Capability::Search));
    assert!(!caps.has(Capability::Cursor));
    assert!(!caps.has(Capability::Insert));
    assert!(!caps.has(Capability::Update));
    assert!(!caps.has(Capability::Delete));
    assert!(!caps.has(Capability::Import));
    assert!(!caps.has(Capability::ConfineWrites));

    assert!(products.set_search(Some("x")).unwrap_err().is_unsupported());
    let err = products
        .insert_value("1", &Default::default())
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(products.delete("1").await.unwrap_err().is_unsupported());
    assert_eq!(support::requests(&server).await, 0);
}

// ── Traversal ────────────────────────────────────────────────────────

const USERS: &str = r#"
name: users
columns:
  id: { type: int, flags: [id] }
  name: { type: string }
references:
  albums: { table: albums, kind: has_many, foreign_key: userId }
"#;

const ALBUMS: &str = r#"
name: albums
api:
  endpoint: users/{userId}/albums
columns:
  id: { type: int, flags: [id] }
  userId: { type: int }
  title: { type: string }
references:
  user: { table: users, kind: has_one, foreign_key: userId }
"#;

fn registered(api: RestApi) -> RestApiVistaFactory {
    let mut factory = RestApiVistaFactory::new(api);
    factory.register_yaml(USERS).unwrap();
    factory.register_yaml(ALBUMS).unwrap();
    factory
}

#[tokio::test]
async fn a_has_many_fills_the_targets_endpoint_from_the_parent_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/1/albums"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": 10, "userId": 1, "title": "first" },
            { "id": 11, "userId": 1, "title": "second" }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let users = registered(builder(&server).build()).build("users").unwrap();

    let albums = users
        .get_ref("albums", Some(&row(&[("id", "1"), ("name", "Marty")])))
        .unwrap();
    assert_eq!(
        texts(&albums.list_values().await.unwrap(), "title"),
        ["first", "second"]
    );
}

#[tokio::test]
async fn a_has_one_narrows_the_target_id_by_the_parents_foreign_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users"))
        .and(query_param("id", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": 1, "name": "Marty" }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let albums = registered(builder(&server).build())
        .build("albums")
        .unwrap();

    let user = albums
        .get_ref("user", Some(&row(&[("id", "10"), ("userId", "1")])))
        .unwrap();
    assert_eq!(texts(&user.list_values().await.unwrap(), "name"), ["Marty"]);
}

#[test]
fn a_column_level_reference_does_not_become_a_relation() {
    let server_less = RestApi::new("http://x");
    let users = vista(
        server_less,
        "name: albums\ncolumns:\n  id: { type: int, flags: [id] }\n  userId: { type: int, references: users }\n",
    );
    assert!(users.reference("userId").is_none());
}

#[tokio::test]
async fn a_model_resolver_builds_reference_targets_in_place_of_the_registry() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/photos"))
        .and(query_param("albumId", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": 100, "albumId": 10, "title": "sunset" }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let mut elsewhere = RestApiVistaFactory::new(builder(&server).build());
    elsewhere
        .register_yaml(
            "name: photos\ncolumns:\n  id: { type: int, flags: [id] }\n  albumId: { type: int }\n  title: { type: string }\n",
        )
        .unwrap();
    let elsewhere = Arc::new(elsewhere);
    let resolver: Arc<dyn VistaResolver> = Arc::new(move |name: &str| elsewhere.build(name));

    let mut factory =
        RestApiVistaFactory::new(builder(&server).build()).with_model_resolver(resolver);
    factory
        .register_yaml(
            "name: albums\ncolumns:\n  id: { type: int, flags: [id] }\nreferences:\n  photos: { table: photos, kind: has_many, foreign_key: albumId }\n",
        )
        .unwrap();
    let albums = factory.build("albums").unwrap();

    let photos = albums
        .get_ref("photos", Some(&row(&[("id", "10")])))
        .unwrap();
    assert_eq!(
        texts(&photos.list_values().await.unwrap(), "title"),
        ["sunset"]
    );
}

// ── Columns ──────────────────────────────────────────────────────────

#[test]
fn every_documented_column_type_name_builds() {
    let api = RestApi::new("http://x");
    let types = [
        "int", "integer", "i64", "i32", "float", "double", "f64", "f32", "bool", "boolean",
        "string", "text", "str", "json", "any",
    ];
    for ty in types {
        let yaml = format!(
            "name: t\ncolumns:\n  id: {{ type: int, flags: [id] }}\n  other: {{ type: {ty} }}\n"
        );
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(&yaml).unwrap();
        assert!(
            RestApiVistaFactory::new(api.clone())
                .build_from_spec(spec)
                .is_ok(),
            "{ty}"
        );
    }
}

#[tokio::test]
async fn a_row_whose_id_is_not_a_string_or_number_is_keyed_by_position() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        200,
        r#"[{"id":1},{"id":true},{"id":[1]},{"id":""},{"name":"x"}]"#,
    )
    .await;
    let rows = vista(builder(&server).build(), PRODUCTS)
        .list_values()
        .await
        .unwrap();
    let keys: Vec<&str> = rows.keys().map(String::as_str).collect();
    assert_eq!(keys, ["1", "~row:1", "~row:2", "~row:3", "~row:4"]);
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn a_lazy_column_is_computed_from_each_fetched_row() {
    let server = MockServer::start().await;
    mount(&server, "GET", 200, r#"[{"id":1,"name":"ab"}]"#).await;
    let products = vista(
        builder(&server).build(),
        "name: products\ncolumns:\n  id: { type: int, flags: [id] }\n  name: { type: string }\n  shout: { type: string, lazy: \"row.name.to_upper()\" }\n",
    );
    let rows = products.list_values().await.unwrap();
    assert_eq!(rows["1"].get("shout"), Some(&CborValue::Text("AB".into())));
}
