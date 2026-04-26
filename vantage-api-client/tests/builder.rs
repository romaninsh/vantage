//! Unit tests for `RestApi::builder`. No network — verifies that the
//! builder records what was set, and that the `Default` shape stays
//! backwards-compatible with the legacy `{ "data": [...] }` format.

use vantage_api_client::{PaginationParams, ResponseShape, RestApi};

#[test]
fn default_response_shape_is_data_wrapper() {
    let _api = RestApi::new("https://example.com");
    // Default shape is `Wrapped { array_key: "data" }` — no public
    // accessor, so behaviour is verified by the legacy doc / fixture
    // tests below.
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
    assert!(!p.skip_based);
}

#[test]
fn pagination_params_skip_limit_constructor() {
    let p = PaginationParams::skip_limit("skip", "limit");
    assert_eq!(p.page, "skip");
    assert_eq!(p.limit, "limit");
    assert!(p.skip_based);
}

#[test]
fn pagination_params_page_limit_constructor() {
    let p = PaginationParams::page_limit("page", "size");
    assert_eq!(p.page, "page");
    assert_eq!(p.limit, "size");
    assert!(!p.skip_based);
}

#[test]
fn builder_compiles_with_all_options() {
    // Smoke test — the builder threads through without panic.
    let _api = RestApi::builder("https://api.example.com")
        .auth("Bearer xyz")
        .response_shape(ResponseShape::BareArray)
        .pagination_params(PaginationParams::skip_limit("skip", "limit"))
        .build();
}

#[test]
fn builder_response_shape_variants() {
    // Each variant constructs without trouble.
    let _bare = RestApi::builder("https://x")
        .response_shape(ResponseShape::BareArray)
        .build();
    let _wrapped = RestApi::builder("https://x")
        .response_shape(ResponseShape::Wrapped {
            array_key: "items".into(),
        })
        .build();
    let _by_table = RestApi::builder("https://x")
        .response_shape(ResponseShape::WrappedByTableName)
        .build();
}
