//! Reference keys a traversal can't follow: a nested key, and a renamed key
//! on the target.

use vantage_dataset::prelude::*;
use vantage_vista::VistaFactory;

use super::{CLIENT, ORDER, TestResult, resolver, seed, setup, spec, teardown};

#[tokio::test]
async fn nested_reference_key_is_refused_at_build() -> TestResult {
    let (db, name) = setup().await;
    let nested_order = spec(
        r#"
name: order
columns:
  _id: { type: string, flags: [id] }
  client_id: { type: string, mongo: { nested_path: refs.client } }
mongo:
  collection: orders
references:
  client:
    table: client
    kind: has_one
    foreign_key: client_id
"#,
    );

    let err = db
        .vista_factory()
        .build_from_spec(nested_order.clone())
        .map(drop)
        .expect_err("nested has_one key refused");
    assert!(err.to_string().contains("its own field name"), "{err}");

    // The client's has_many lands on the same nested key, on the target.
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![spec(CLIENT), nested_order]));
    let err = factory
        .build_from_spec(spec(CLIENT))
        .map(drop)
        .expect_err("nested has_many key refused");
    assert!(err.to_string().contains("client_id"), "{err}");

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn traversal_into_target_with_renamed_reference_key_errors() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;
    // The order's own reference to product keys on a renamed field; the
    // client's has_many into order is sound.
    let order = spec(
        &(ORDER.replace(
            "  item: { type: string, flags: [title] }\n",
            "  item: { type: string, flags: [title] }\n  product_id: { type: string, mongo: { field: productId } }\n",
        ) + "  product:\n    table: product\n    kind: has_one\n    foreign_key: product_id\n"),
    );
    let clients = db
        .vista_factory()
        .with_resolver(resolver(vec![spec(CLIENT), order]))
        .build_from_spec(spec(CLIENT))?;

    let row = clients.get_value("c1").await?.expect("client c1");
    let err = clients
        .get_ref("orders", Some(&row))
        .map(drop)
        .expect_err("traversal into the order must fail loudly");
    let msg = err.to_string();
    assert!(msg.contains("its own field name"), "{msg}");
    assert!(msg.contains("product_id"), "{msg}");

    teardown(&db, &name).await;
    Ok(())
}
