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
