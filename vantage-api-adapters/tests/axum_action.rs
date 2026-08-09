//! Transport behaviour for [`ActionRouter`] — routing, target mapping,
//! and the error classification that decides the status code.

#![cfg(feature = "axum")]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use vantage_action::{ModelAction, record_action, table_action};
use vantage_api_adapters::axum_action::ActionRouter;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Empty {}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Echo {
    saw: String,
}

/// A record action that reports the id the transport resolved.
fn echo_record(table: &str, name: &str) -> Arc<dyn ModelAction> {
    record_action(name, table, "echo", |id: String, _: Empty| async move {
        Ok(Echo { saw: id })
    })
}

/// A record action whose body always fails with the given classification.
fn failing_record(table: &str, name: &str, kind: &'static str) -> Arc<dyn ModelAction> {
    record_action(
        name,
        table,
        "fails",
        move |_: String, _: Empty| async move {
            let e = vantage_core::error!("the body refused");
            Err::<Echo, _>(match kind {
                "not_found" => e.mark_not_found(),
                "incorrect_usage" => e.mark_incorrect_usage(),
                _ => e,
            })
        },
    )
}

async fn post(app: Router, path: &str, body: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// The regression this file exists for: keying on the name alone made
/// `tag::archive` and `user::archive` collide, and the second silently
/// replaced the first even though their routes never overlap.
#[tokio::test]
async fn same_name_on_two_tables_keeps_both_routes() {
    let actions = ActionRouter::new(vec![
        echo_record("tag", "archive"),
        echo_record("user", "archive"),
    ])
    .expect("distinct tables are not a duplicate");
    let app: Router = actions.into_router();

    let (status, body) = post(app.clone(), "/tag/t1/actions/archive", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"saw":"t1"}"#);

    let (status, body) = post(app, "/user/u1/actions/archive", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"saw":"u1"}"#);
}

#[tokio::test]
async fn a_genuine_duplicate_is_rejected_at_construction() {
    let result = ActionRouter::new(vec![
        echo_record("tag", "archive"),
        echo_record("tag", "archive"),
    ]);
    let err = match result {
        Ok(_) => panic!("same table and name must not register twice"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("share a table and a name"));
}

#[tokio::test]
async fn table_actions_mount_without_an_id_segment() {
    let action = table_action("sweep", "tag", "sweep", |_: Empty| async move {
        Ok(Echo {
            saw: "whole table".to_string(),
        })
    });
    let app: Router = ActionRouter::new(vec![action]).unwrap().into_router();

    let (status, body) = post(app, "/tag/actions/sweep", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"saw":"whole table"}"#);
}

/// Status comes from the error's classification, never from its text —
/// an unclassified failure is the model's problem, so it is a 500.
#[tokio::test]
async fn status_follows_the_error_kind_not_the_message() {
    for (kind, expected) in [
        ("not_found", StatusCode::NOT_FOUND),
        ("incorrect_usage", StatusCode::BAD_REQUEST),
        ("generic", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let app: Router = ActionRouter::new(vec![failing_record("tag", "go", kind)])
            .unwrap()
            .into_router();
        let (status, body) = post(app, "/tag/t1/actions/go", "{}").await;
        assert_eq!(status, expected, "kind {kind}");
        assert!(body.contains("the body refused"), "kind {kind}: {body}");
    }
}

/// An outage must not be reported as the caller's fault. Before the fix
/// every unrecognised message fell through to 400, which tells clients
/// and alerting that nothing is wrong.
#[tokio::test]
async fn an_unclassified_failure_is_never_a_4xx() {
    let app: Router = ActionRouter::new(vec![failing_record("tag", "go", "generic")])
        .unwrap()
        .into_router();
    let (status, _) = post(app, "/tag/t1/actions/go", "{}").await;
    assert!(status.is_server_error(), "got {status}");
}

/// A body that does not match the input type is the caller's fault, so
/// the action layer classifies it and the transport answers 400.
#[tokio::test]
async fn a_wrong_shaped_body_is_a_400() {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct Needs {
        #[allow(dead_code)]
        who: String,
    }
    let action = record_action("go", "tag", "needs input", |_: String, _: Needs| async {
        Ok(Echo { saw: String::new() })
    });
    let app: Router = ActionRouter::new(vec![action]).unwrap().into_router();

    let (status, body) = post(app, "/tag/t1/actions/go", "{}").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid action input"), "{body}");
}

/// Legacy paths may carry captures besides the id. `Path<String>` used
/// to answer those with a 500 quoting axum internals.
#[tokio::test]
async fn an_alias_path_may_carry_more_than_one_capture() {
    let actions = ActionRouter::new(vec![echo_record("tag", "register")]).unwrap();
    let app: Router = Router::new().route(
        "/t/{tenant}/tag/{id}/register",
        actions.alias("tag", "register").unwrap(),
    );

    let (status, body) = post(app, "/t/acme/tag/GH1/register", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"saw":"GH1"}"#);
}

/// An alias mounts onto a path somebody else already named. The legacy
/// chtags contract spells it `/tag/{tag_id}/register`, so requiring the
/// literal `{id}` would make every existing route unmountable.
#[tokio::test]
async fn a_single_capture_is_the_id_whatever_it_is_called() {
    let actions = ActionRouter::new(vec![echo_record("tag", "register")]).unwrap();
    let app: Router = Router::new().route(
        "/tag/{tag_id}/register",
        actions.alias("tag", "register").unwrap(),
    );

    let (status, body) = post(app, "/tag/GH1/register", "{}").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"saw":"GH1"}"#);
}

/// With more than one capture the choice would be arbitrary, so the id
/// has to be named. Saying so beats picking one at random.
#[tokio::test]
async fn several_captures_none_named_id_is_reported_not_guessed() {
    let actions = ActionRouter::new(vec![echo_record("tag", "register")]).unwrap();
    let app: Router = Router::new().route(
        "/t/{tenant}/tag/{tag_id}/register",
        actions.alias("tag", "register").unwrap(),
    );

    let (status, body) = post(app, "/t/acme/tag/GH1/register", "{}").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("has no id segment"), "{body}");
}

#[tokio::test]
async fn alias_rejects_an_unknown_table_or_name() {
    let actions = ActionRouter::new(vec![echo_record("tag", "register")]).unwrap();
    assert!(actions.alias::<()>("tag", "registerr").is_err());
    assert!(actions.alias::<()>("batch", "register").is_err());
}
