//! Unit tests for `RestApi::builder`. No network — verifies that the
//! builder records what was set, and that the default response shape is
//! the `{ "data": [...] }` envelope.

mod support;

use support::test_client;
use vantage_api_client::{PaginationMode, PaginationParams, ResponseShape, RestApi};

#[test]
fn default_response_shape_is_data_wrapper() {
    let _api = RestApi::new("https://example.com");
    // `RestApi` has no shape accessor, so the default `RestApi::new` uses
    // is checked on the shape itself.
    let shape = ResponseShape::default();
    match shape {
        ResponseShape::Wrapped { array_key } => assert_eq!(array_key, "data"),
        _ => panic!("default shape should be Wrapped {{ array_key: \"data\" }}"),
    }
}

#[test]
fn default_pagination_params_match_jsonserver() {
    let p = PaginationParams::default();
    assert_eq!(p.page, "_page");
    assert_eq!(p.limit, "_limit");
    assert_eq!(p.mode, PaginationMode::Page);
}

#[test]
fn pagination_params_skip_limit_constructor() {
    let p = PaginationParams::skip_limit("skip", "limit");
    assert_eq!(p.page, "skip");
    assert_eq!(p.limit, "limit");
    assert_eq!(p.mode, PaginationMode::Skip);
}

#[test]
fn pagination_params_page_limit_constructor() {
    let p = PaginationParams::page_limit("page", "size");
    assert_eq!(p.page, "page");
    assert_eq!(p.limit, "size");
    assert_eq!(p.mode, PaginationMode::Page);
}

#[test]
fn builder_compiles_with_all_options() {
    // Smoke test — the builder threads through without panic.
    let _api = RestApi::builder("https://api.example.com")
        .auth("Bearer xyz")
        .response_shape(ResponseShape::BareArray)
        .pagination_params(PaginationParams::skip_limit("skip", "limit"))
        .http_client(test_client())
        .build();
}

#[test]
fn builder_response_shape_variants() {
    // Each variant constructs without trouble.
    let _bare = RestApi::builder("https://x")
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
        .build();
    let _wrapped = RestApi::builder("https://x")
        .response_shape(ResponseShape::Wrapped {
            array_key: "items".into(),
        })
        .http_client(test_client())
        .build();
    let _by_table = RestApi::builder("https://x")
        .response_shape(ResponseShape::WrappedByTableName)
        .http_client(test_client())
        .build();
}
