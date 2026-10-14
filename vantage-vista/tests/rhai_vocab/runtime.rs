use serde_json::json;
use vantage_vista::rhai::{
    DEFAULT_LIMIT, MAX_LIMIT, MIN_LIMIT, Writes, preview_script, run_script,
};

use super::support::{resolver, store};

#[test]
fn a_future_that_never_resolves_errors_without_a_runtime() {
    let err = vantage_vista::rhai::block_on(std::future::pending::<()>()).unwrap_err();
    assert!(err.to_string().contains("tokio runtime"), "{err}");
    assert_eq!(
        vantage_vista::rhai::block_on(std::future::ready(5)).unwrap(),
        5
    );
}

#[test]
fn the_default_limit_sits_between_the_bounds() {
    const { assert!(MIN_LIMIT <= DEFAULT_LIMIT && DEFAULT_LIMIT <= MAX_LIMIT) };
}

#[tokio::test]
async fn a_script_runs_and_its_final_value_comes_back_as_json() {
    let out = run_script(
        r#"table("t").count()"#.into(),
        resolver(&store()),
        MAX_LIMIT,
        Writes::Allowed,
    )
    .await
    .unwrap();
    assert_eq!(out, json!(3));
}

#[tokio::test]
async fn every_list_is_capped_by_the_clamped_limit() {
    let out = run_script(
        r#"table("t").list()"#.into(),
        resolver(&store()),
        0,
        Writes::Allowed,
    )
    .await
    .unwrap();
    assert_eq!(out.as_array().unwrap().len(), MIN_LIMIT.min(3));
}

#[tokio::test]
async fn compile_runtime_and_backend_errors_come_back_as_text() {
    for script in [
        "let x = ;",
        r#"nonsense_function()"#,
        r#"table("t").get("r1").nope.deeper"#,
    ] {
        let err = run_script(script.into(), resolver(&store()), 5, Writes::Allowed)
            .await
            .unwrap_err();
        assert!(!err.is_empty(), "{script}");
    }
}

#[tokio::test]
async fn denied_writes_throw_their_message() {
    let err = run_script(
        r#"table("t").insert(#{ a: 1 })"#.into(),
        resolver(&store()),
        5,
        Writes::Denied("writes are off".into()),
    )
    .await
    .unwrap_err();
    assert!(err.contains("writes are off"), "{err}");
}

#[test]
fn a_preview_renders_the_query_and_names_the_driver() {
    let out = preview_script(r#"table("t").where("a", 1)"#.into(), resolver(&store())).unwrap();
    assert_eq!(out["driver"], json!("memory"));
    assert!(out["query"].is_string(), "{out}");
}

#[test]
fn a_preview_script_must_end_on_the_query_itself() {
    let err = preview_script(r#"table("t").list()"#.into(), resolver(&store())).unwrap_err();
    assert!(!err.is_empty());
    let err = preview_script(r#"42"#.into(), resolver(&store())).unwrap_err();
    assert!(err.contains("query"), "{err}");
    let err = preview_script("let x = ;".into(), resolver(&store())).unwrap_err();
    assert!(!err.is_empty());
}

#[test]
fn a_preview_of_an_unknown_table_is_an_error() {
    let resolver: std::sync::Arc<dyn vantage_vista::VistaResolver> =
        std::sync::Arc::new(|name: &str| {
            Err(vantage_core::error!("no such table", name = name).mark_not_found())
        });
    let err = preview_script(r#"table("nope")"#.into(), resolver).unwrap_err();
    assert!(!err.is_empty());
}
