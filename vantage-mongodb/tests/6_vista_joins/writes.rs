//! Writes over a join: the joined value is stripped before it is stored, and
//! a set confined by a joined column refuses writes.

use bson::doc;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_mongodb::{AnyMongoType, MongoId};
use vantage_types::Record;
use vantage_vista::{FilterOp, VistaFactory};

use super::{ORDER, TestResult, orders, resolver, seed, setup, spec, teardown, text};

#[tokio::test]
async fn dotted_column_stripped_on_write() -> TestResult {
    let (db, name) = setup().await;
    let ids = seed(&db).await?;
    let vista = orders(&db);

    // On a vista, `client.name` in an insert payload is a nested has-one
    // insert, so the table-level write path is where the strip shows.
    let table = db
        .vista_factory()
        .with_resolver(resolver())
        .table_from_spec(&spec(ORDER))?;
    let mut fresh: Record<AnyMongoType> = Record::new();
    fresh.insert("client_id".into(), AnyMongoType::untyped(ids.marty.into()));
    fresh.insert("item".into(), AnyMongoType::untyped("plutonium".into()));
    fresh.insert("client.name".into(), AnyMongoType::untyped("Biff".into()));
    let inserted = table.insert_value(MongoId::from("o5"), &fresh).await?;
    assert_eq!(
        inserted.get("client.name").map(|v| v.value().clone()),
        Some(bson::Bson::String("Marty".into()))
    );
    let raw = db
        .doc_collection("orders")
        .find_one(doc! { "_id": "o5" })
        .await?
        .expect("stored o5");
    assert_eq!(
        raw.keys().collect::<Vec<_>>(),
        vec!["_id", "client_id", "item"],
        "{raw:?}"
    );

    // A full-record round trip carries the joined value back; it is dropped.
    let mut o1 = vista.get_value("o1").await?.expect("o1");
    o1.insert("item".into(), CborValue::Text("longboard".into()));
    vista.replace_value("o1", &o1).await?;
    let raw = db
        .doc_collection("orders")
        .find_one(doc! { "_id": "o1" })
        .await?
        .expect("stored o1");
    assert_eq!(raw.get_str("item")?, "longboard");
    assert!(!raw.contains_key("client.name") && !raw.contains_key("client"));

    // A patch naming the joined column is refused; one without it applies.
    let mut partial = Record::new();
    partial.insert("client.name".to_string(), CborValue::Text("Biff".into()));
    assert!(vista.patch_value("o1", &partial).await.is_err());
    let mut partial = Record::new();
    partial.insert("item".to_string(), CborValue::Text("hoverboard".into()));
    vista.patch_value("o1", &partial).await?;
    let raw = db
        .doc_collection("orders")
        .find_one(doc! { "_id": "o1" })
        .await?
        .expect("stored o1");
    assert_eq!(raw.get_str("item")?, "hoverboard");
    assert!(!raw.contains_key("client.name") && !raw.contains_key("client"));

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_write_under_joined_condition_is_refused() -> TestResult {
    let (db, name) = setup().await;
    seed(&db).await?;

    let mut marty = orders(&db);
    marty.add_condition("client.name", FilterOp::Eq, CborValue::Text("Marty".into()))?;
    let err = marty.delete("o1").await.expect_err("confined by a join");
    assert!(err.to_string().contains("joined column"), "{err}");
    assert!(err.is_unsupported(), "{err}");
    assert_eq!(
        db.doc_collection("orders").count_documents(doc! {}).await?,
        4
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn dotted_column_coexists_with_fk_field() -> TestResult {
    let (db, name) = setup().await;
    let ids = seed(&db).await?;
    db.doc_collection("tickets")
        .insert_one(doc! { "_id": "t1", "client": ids.marty })
        .await?;

    let tickets = db
        .vista_factory()
        .with_resolver(resolver())
        .build_from_spec(spec(
            r#"
name: ticket
columns:
  _id: { type: string, flags: [id] }
  client: { type: string }
  note: { type: string }
  client.name: { type: string }
mongo:
  collection: tickets
references:
  client:
    table: client
    kind: has_one
    foreign_key: client
"#,
        ))?;
    let t1 = tickets.get_value("t1").await?.expect("t1");
    assert_eq!(text(&t1, "client"), Some(ids.marty.to_hex()));
    assert_eq!(text(&t1, "client.name").as_deref(), Some("Marty"));

    // Patching a plain field keeps the stored foreign key (and so the join).
    let mut partial = Record::new();
    partial.insert("note".to_string(), CborValue::Text("late".into()));
    let patched = tickets.patch_value("t1", &partial).await?;
    assert_eq!(text(&patched, "client.name").as_deref(), Some("Marty"));
    let raw = db
        .doc_collection("tickets")
        .find_one(doc! { "_id": "t1" })
        .await?
        .expect("stored t1");
    assert_eq!(raw.get("client"), Some(&bson::Bson::ObjectId(ids.marty)));
    assert_eq!(raw.get_str("note")?, "late");

    teardown(&db, &name).await;
    Ok(())
}
