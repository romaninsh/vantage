//! `rhai:` pipeline views: arguments, stage validation, and the refusal to
//! combine one with a YAML `pipeline:`.

use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_vista::{Capability, VistaFactory};

use super::{BY_CATEGORY, TestResult, seed_products, setup, spec, teardown};

#[tokio::test]
async fn rhai_pipeline_view_with_args() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;

    let mut s = spec(BY_CATEGORY);
    s.driver.mongo.as_mut().expect("mongo block").args = vec![("cat".into(), "cake".into())];
    let v = db.vista_factory().build_from_spec(s)?;
    let rows = v.list_values().await?;
    let keys: Vec<&str> = rows.keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["cake"]);
    assert_eq!(rows["cake"].get("n"), Some(&CborValue::Integer(3.into())));

    // Same read-only capabilities as a YAML `pipeline:` view.
    let caps = v.capabilities();
    assert!(caps.has(Capability::Order) && caps.has(Capability::Cursor));
    assert!(!caps.has(Capability::Insert) && !caps.has(Capability::Update));
    assert!(!caps.has(Capability::Delete) && !caps.has(Capability::Subscribe));
    let preview = v.preview().query.expect("a rendered query");
    assert!(preview.contains("$group"), "preview: {preview}");

    teardown(&db, &name).await;
    Ok(())
}

/// The error a `rhai:` pipeline script with the given body fails the build with.
async fn script_build_error(code: &str) -> String {
    let (db, name) = setup().await;
    let yaml = format!(
        "name: scripted\ncolumns:\n  _id: {{ type: string, flags: [id] }}\nmongo:\n  collection: products\n  rhai: |\n    {code}\n"
    );
    let Err(err) = db.vista_factory().build_from_spec(spec(&yaml)) else {
        panic!("script `{code}` must fail the build");
    };
    teardown(&db, &name).await;
    format!("{err} {err:?}")
}

#[tokio::test]
async fn rhai_pipeline_script_must_evaluate_to_an_array_of_stages() {
    let msg = script_build_error(r#"#{"$match": #{}}"#).await;
    assert!(msg.contains("array of stages"), "error: {msg}");
}

#[tokio::test]
async fn rhai_pipeline_script_needs_at_least_one_stage() {
    let msg = script_build_error("[]").await;
    assert!(msg.contains("at least one stage"), "error: {msg}");
}

#[tokio::test]
async fn rhai_pipeline_script_stages_must_be_maps_naming_the_bad_index() {
    let msg = script_build_error(r#"[#{"$match": #{}}, 1]"#).await;
    assert!(msg.contains("stage: 1"), "error: {msg}");
}

#[tokio::test]
async fn rhai_pipeline_script_cannot_write_with_out_or_merge() {
    let msg = script_build_error(r#"[#{"$match": #{}}, #{"$out": "copy"}]"#).await;
    assert!(msg.contains("read-only"), "error: {msg}");
    assert!(msg.contains("stage: 1"), "error: {msg}");
}

#[tokio::test]
async fn rhai_pipeline_script_that_fails_or_never_ends_fails_the_build() {
    let msg = script_build_error("this_does_not_exist()").await;
    assert!(msg.contains("script failed"), "error: {msg}");

    let msg = script_build_error("loop {}").await;
    assert!(msg.contains("limit"), "error: {msg}");
}

#[tokio::test]
async fn rhai_pipeline_view_without_args_sees_an_empty_args_map() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;

    // Nothing sets `args`, so the script can still run and branch on it.
    let yaml = r#"
name: totals
columns:
  _id: { type: string, flags: [id] }
  n: { type: int }
mongo:
  collection: products
  rhai: |
    let stages = [];
    if "cat" in args { stages.push(#{"$match": #{category: args.cat}}); }
    stages.push(#{"$group": #{_id: "$category", n: #{"$sum": 1}}});
    stages
"#;
    let v = db.vista_factory().build_from_spec(spec(yaml))?;
    let rows = v.list_values().await?;
    let mut keys: Vec<&str> = rows.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, vec!["bread", "cake", "candy"]);

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn rhai_pipeline_stages_are_validated() -> TestResult {
    let (db, name) = setup().await;
    let yaml = r#"
name: broken_view
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: products
  rhai: |
    [#{"$match": #{}}, #{group: #{_id: "$a"}}]
"#;
    let Err(err) = db.vista_factory().build_from_spec(spec(yaml)) else {
        panic!("a stage without a `$` operator must be rejected");
    };
    let msg = err.to_string();
    assert!(msg.contains("stage: 1"), "error: {msg}");
    assert!(msg.contains("broken_view"), "error names the spec: {msg}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn rhai_and_pipeline_together_rejected() -> TestResult {
    let (db, name) = setup().await;
    let yaml = r#"
name: both
columns:
  _id: { type: string, flags: [id] }
mongo:
  collection: products
  pipeline:
    - $match: {}
  rhai: |
    [#{"$match": #{}}]
"#;
    let Err(err) = db.vista_factory().build_from_spec(spec(yaml)) else {
        panic!("`pipeline:` and `rhai:` together must be rejected");
    };
    let msg = err.to_string();
    assert!(
        msg.contains("pipeline") && msg.contains("rhai"),
        "error: {msg}"
    );

    teardown(&db, &name).await;
    Ok(())
}
