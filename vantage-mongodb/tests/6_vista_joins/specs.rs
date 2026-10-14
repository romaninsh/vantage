//! Specs that may not declare a join: a pipeline view, and a dotted column
//! through a relation the spec doesn't declare.

use vantage_vista::VistaFactory;

use super::{TestResult, orders, resolver, setup, spec, teardown};

#[tokio::test]
async fn dotted_column_on_pipeline_view_is_rejected() -> TestResult {
    let (db, name) = setup().await;

    let err = db
        .vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(
            r#"
name: order_view
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string }
  client.name: { type: string }
mongo:
  collection: orders
  pipeline:
    - $match: { item: { $exists: true } }
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#,
        ))
        .map(drop)
        .expect_err("a view cannot join");
    assert!(err.to_string().contains("pipeline view"), "{err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_through_unknown_relation_is_rejected() -> TestResult {
    let (db, name) = setup().await;

    let result = db
        .vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(
            r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  owner.name: { type: string }
mongo:
  collection: orders
"#,
        ));
    let err = result.map(drop).expect_err("no `owner` relation");
    // "No references defined on table" / "Reference not found on table".
    assert!(
        err.to_string().to_lowercase().contains("reference"),
        "{err}"
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn joined_columns_are_for_collections_only() -> TestResult {
    let (db, name) = setup().await;

    assert!(orders(&db).column("client.name").is_some());
    let view = db.vista_factory().build_from_spec(spec(
        r#"
name: totals
columns:
  _id: { type: string, flags: [id] }
  client.name: { type: string }
mongo:
  collection: orders
  pipeline:
    - $group: { _id: "$client_id" }
"#,
    ));
    let Err(err) = view else {
        panic!("a pipeline view cannot join");
    };
    assert!(err.to_string().contains("joined (dotted) column"), "{err}");

    teardown(&db, &name).await;
    Ok(())
}
