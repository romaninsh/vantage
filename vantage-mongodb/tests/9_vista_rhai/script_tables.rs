//! `table(name)` in a reference script: an error when a resolver doesn't know
//! the name, a bare collection when no resolver is attached.

use bson::doc;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_mongodb::vista::MongoVistaSpec;
use vantage_types::Record;
use vantage_vista::VistaFactory;

use super::{
    ORDER, TestResult, any_client_row, client_with_script, resolver, setup, spec, teardown, text,
};

/// A client whose `orders` reference script builds `table("nope")`.
fn client_targeting_nope() -> MongoVistaSpec {
    spec(&client_with_script(r#"table("nope")"#))
}

#[tokio::test]
async fn script_table_unknown_to_resolver_errors() -> TestResult {
    let (db, name) = setup().await;
    let client_spec = client_targeting_nope();
    let factory = db
        .vista_factory()
        .with_resolver(resolver(vec![client_spec.clone(), spec(ORDER)]));
    let clients = factory.build_from_spec(client_spec)?;
    let err = clients
        .get_ref("orders", Some(&any_client_row()))
        .map(drop)
        .expect_err("an unknown table must fail with a resolver attached");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown table") && msg.contains("nope"),
        "{msg}"
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn script_table_without_resolver_is_bare_collection() -> TestResult {
    let (db, name) = setup().await;
    db.doc_collection("nope")
        .insert_one(doc! { "_id": "n1", "anything": 1 })
        .await?;
    let clients = db
        .vista_factory()
        .build_from_spec(client_targeting_nope())?;
    let target = clients.get_ref("orders", Some(&any_client_row()))?;
    assert_eq!(target.name(), "nope");
    assert_eq!(target.count().await?, 1);
    // A bare collection reads documents as stored, every field included.
    let rows = target.list_values().await?;
    assert_eq!(
        rows["n1"].get("anything"),
        Some(&CborValue::Integer(1.into()))
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn script_where_on_bare_collection_without_resolver() -> TestResult {
    let (db, name) = setup().await;
    db.doc_collection("orders")
        .insert_many(vec![
            doc! { "_id": "o1", "client_id": "c1", "item": "skateboard" },
            doc! { "_id": "o2", "client_id": "c2", "item": "almanac" },
        ])
        .await?;
    let clients = db.vista_factory().build_from_spec(spec(&client_with_script(
        r#"table("orders").where("client_id", me._id)"#,
    )))?;
    let mut row = Record::new();
    row.insert("_id".to_string(), CborValue::Text("c1".into()));
    let orders = clients.get_ref("orders", Some(&row))?;
    let rows = orders.list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), vec!["o1"]);
    assert_eq!(text(&rows["o1"], "item").as_deref(), Some("skateboard"));

    teardown(&db, &name).await;
    Ok(())
}
