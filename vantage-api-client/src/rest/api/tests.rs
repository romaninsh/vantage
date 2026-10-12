use super::request::join_query;
use super::*;

/// `build_query_string` with no conditions, exercising only the
/// window → pagination-param mapping.
fn qs(api: &RestApi, window: Option<(i64, i64)>) -> String {
    api.build_query_string(window, &[], &[], None)
}

#[test]
fn endpoint_url_fills_placeholders_from_conditions() {
    let api = RestApi::builder("http://x").build();
    let user = crate::eq_condition("userId", 1i64);
    let title = crate::eq_condition("title", "a b");
    let team = crate::eq_condition("team", "x/y");
    let (url, consumed) = api
        .endpoint_url("teams/{team}/users/{userId}", &[&user, &title, &team])
        .unwrap();
    assert_eq!(url, "http://x/teams/x%2Fy/users/1");
    assert_eq!(consumed, vec![2, 0]);
}

#[test]
fn endpoint_url_rejects_an_unclosed_placeholder() {
    let api = RestApi::builder("http://x").build();
    let err = api.endpoint_url("users/{userId", &[]).unwrap_err();
    assert!(err.to_string().contains("Unclosed"), "{err}");
}

#[test]
fn debug_masks_auth_header() {
    let api = RestApi::builder("http://x")
        .auth("Bearer secret-token")
        .build();
    let text = format!("{api:?}");
    assert!(!text.contains("secret-token"), "{text}");
    assert!(text.contains("<set>"), "{text}");
}

/// The auth header is part of the previewed request, masked, not a note.
#[test]
fn preview_shows_the_masked_auth_header_in_the_query() {
    let api = RestApi::builder("http://x")
        .auth("Bearer secret-token")
        .build();
    let preview = api.preview_request("users", None, None, []);
    assert_eq!(
        preview.query.as_deref(),
        Some("GET http://x/users\nAuthorization: <set>")
    );
    assert_eq!(preview.note, None);

    let open = RestApi::builder("http://x").build();
    let preview = open.preview_request("users", None, None, []);
    assert_eq!(preview.query.as_deref(), Some("GET http://x/users"));
}

#[test]
fn ordering_value_prefixes_only_descending() {
    use vantage_vista::SortDirection;
    let params = OrderingParams::new("sort", "-");
    assert_eq!(params.value("net", SortDirection::Ascending), "net");
    assert_eq!(params.value("net", SortDirection::Descending), "-net");
}

#[test]
fn skip_based_window_uses_offset_verbatim() {
    let api = RestApi::builder("http://x")
        .pagination_params(PaginationParams::skip_limit("skip", "limit"))
        .build();
    assert_eq!(qs(&api, Some((20, 10))), "?skip=20&limit=10");
}

#[test]
fn page_based_window_derives_one_based_page() {
    let api = RestApi::builder("http://x").build(); // default _page/_limit
    // offset 20 / limit 10 → page 3 (1-based).
    assert_eq!(qs(&api, Some((20, 10))), "?_page=3&_limit=10");
}

#[test]
fn no_window_emits_no_pagination_params() {
    let api = RestApi::builder("http://x").build();
    assert_eq!(qs(&api, None), "");
}

#[test]
fn no_pagination_suppresses_window_params() {
    let api = RestApi::builder("http://x").no_pagination().build();
    assert_eq!(qs(&api, Some((20, 10))), "");
}

#[test]
fn query_string_joins_plain_endpoint_with_question_mark() {
    assert_eq!(
        join_query("http://x/launches/", "?_page=1&_limit=10"),
        "http://x/launches/?_page=1&_limit=10"
    );
}

#[test]
fn query_string_joins_templated_endpoint_with_ampersand() {
    // Endpoint already carries `?mode=detailed`; pagination must append
    // with `&`, not a second `?`.
    assert_eq!(
        join_query("http://x/launches/?mode=detailed", "?offset=0&limit=1"),
        "http://x/launches/?mode=detailed&offset=0&limit=1"
    );
}

#[test]
fn empty_query_string_leaves_endpoint_untouched() {
    assert_eq!(
        join_query("http://x/launches/?mode=detailed", ""),
        "http://x/launches/?mode=detailed"
    );
}

/// Live regression for the double-`?` bug: a real fetch against the
/// Launch Library 2 dev API using a table path that already carries a
/// query string (`launches/?mode=detailed`). Before the `join_query`
/// fix the request URL was `…/launches/?mode=detailed?offset=0&limit=1`
/// and the server answered 500. Network-gated, so `#[ignore]`d:
/// `cargo test -p vantage-api-client -- --ignored query_string`.
#[tokio::test]
#[ignore = "hits the live Launch Library 2 dev API"]
async fn live_templated_table_path_fetches_rows() {
    let api = RestApi::builder("https://lldev.thespacedevs.com/2.3.0")
        .pagination_params(PaginationParams::skip_limit("offset", "limit"))
        .response_shape(ResponseShape::Wrapped {
            array_key: "results".into(),
        })
        .total_key("count")
        .build();

    let total = api
        .fetch_total("launches/?mode=detailed", [])
        .await
        .expect("fetch_total");
    assert!(total.is_some_and(|n| n > 0), "expected a positive count");

    let (rows, window_total) = api
        .fetch_window_records_counted("launches/?mode=detailed", Some("id"), &[], (0, 3), None, [])
        .await
        .expect("fetch_window_records_counted");
    assert_eq!(rows.len(), 3, "expected the requested 3-row window");
    // The whole point of the counted window: the same response that
    // carried the rows also carried the count, so `fetch_total`'s extra
    // round trip buys nothing a caller couldn't already have.
    assert_eq!(
        window_total, total,
        "the window's envelope total should match the dedicated count",
    );
}
