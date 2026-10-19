//! `modify:` scripts narrowing a built vista.

use bson::doc;
use bson::oid::ObjectId;
use ciborium::Value as CborValue;
use vantage_dataset::prelude::*;
use vantage_vista::{Capability, VistaFactory};

use super::{PLAIN_PRODUCT, TestResult, seed_products, setup, spec, teardown, text};

const PRODUCT: &str = r#"
name: product
columns:
  _id: { type: object_id, flags: [id] }
  category: { type: string }
  price: { type: int }
mongo:
  collection: products
  modify: |
    self.with_condition(#{price: #{"$gt": 100}})
"#;

#[tokio::test]
async fn modify_script_adds_condition() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;

    let v = db.vista_factory().build_from_spec(spec(PRODUCT))?;
    let rows = v.list_values().await?;
    let mut categories: Vec<String> = rows.values().filter_map(|r| text(r, "category")).collect();
    categories.sort();
    assert_eq!(categories, vec!["cake", "candy"]);
    assert_eq!(v.count().await?, 2);
    // A condition narrows; the collection stays writable.
    assert!(v.capabilities().has(Capability::Insert));
    // The script's filter is recorded under its canonical JSON.
    assert_eq!(
        v.query().native,
        vec![r#"{"price":{"$gt":{"$numberLong":"100"}}}"#.to_string()]
    );

    teardown(&db, &name).await;
    Ok(())
}

#[tokio::test]
async fn modify_script_converts_oid_and_date() -> TestResult {
    let (db, name) = setup().await;
    let keep = ObjectId::new();
    db.doc_collection("events")
        .insert_many(vec![
            doc! { "_id": keep, "at": bson::DateTime::parse_rfc3339_str("2026-03-01T00:00:00Z")? },
            doc! { "_id": ObjectId::new(), "at": bson::DateTime::parse_rfc3339_str("2025-01-01T00:00:00Z")? },
        ])
        .await?;
    let yaml = format!(
        r#"
name: events
columns:
  _id: {{ type: object_id, flags: [id] }}
  at: {{ type: datetime }}
mongo:
  modify: |
    self.with_condition(#{{_id: mongo::oid("{hex}"), at: #{{"$gte": mongo::date("2026-01-01T00:00:00Z")}}}})
"#,
        hex = keep.to_hex()
    );
    let v = db.vista_factory().build_from_spec(spec(&yaml))?;
    let ids: Vec<String> = v.list_values().await?.into_keys().collect();
    assert_eq!(ids, vec![keep.to_hex()]);

    teardown(&db, &name).await;
    Ok(())
}

/// Each verb on `self` starts from the built vista, and the script's value is
/// the result: the discarded `cheap` handle (which would leave no row) must
/// not narrow the `pricey` one built from `self` after it.
#[tokio::test]
async fn modify_script_resolves_self_twice() -> TestResult {
    let (db, name) = setup().await;
    seed_products(&db).await?;
    let yaml = PLAIN_PRODUCT.to_string()
        + r#"  modify: |
    let cheap = self.where("price", "<", 10);
    let pricey = self.with_condition(#{price: #{"$gt": 100}});
    pricey.where("category", "cake")
"#;
    let v = db.vista_factory().build_from_spec(spec(&yaml))?;

    let all = db.vista_factory().build_from_spec(spec(PLAIN_PRODUCT))?;
    let expected: Vec<String> = all
        .list_values()
        .await?
        .into_iter()
        .filter(|(_, r)| {
            text(r, "category").as_deref() == Some("cake")
                && r.get("price") == Some(&CborValue::Integer(150.into()))
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(expected.len(), 1);
    let ids: Vec<String> = v.list_values().await?.into_keys().collect();
    assert_eq!(ids, expected);
    assert_eq!(v.query().conditions.len(), 1);
    assert_eq!(v.query().native.len(), 1);

    teardown(&db, &name).await;
    Ok(())
}

/// A rule script narrows the table itself, before any vista exists, with
/// the same `with_condition` a `modify:` script has. The client never
/// connects: the filter is built offline.
#[tokio::test]
async fn rule_with_condition_narrows_the_table() -> TestResult {
    use vantage_mongodb::types::cbor::{bson_to_cbor, cbor_to_bson};
    use vantage_mongodb::vista::register_rule_conditions;
    use vantage_mongodb::{AnyMongoType, MongoDB, MongoId};
    use vantage_rhai::{Env, Host, Limits};
    use vantage_table::rule::{RuleCodec, TableRuleTarget};
    use vantage_table::table::Table;
    use vantage_types::EmptyEntity;
    use vantage_vista::rhai::{RuleHandle, RuleVerb, eval_rule_script, register_rule_vocab};

    let client = mongodb::Client::with_uri_str("mongodb://127.0.0.1:1").await?;
    let table = Table::<MongoDB, EmptyEntity>::new("client", MongoDB::new(client, "offline"))
        .with_id_column("_id")
        .with_column_of::<i64>("bakery_id");
    let target = TableRuleTarget::new(
        table,
        RuleCodec {
            to_cbor: |v: AnyMongoType| bson_to_cbor(v.value()),
            from_cbor: |v: CborValue| AnyMongoType::untyped(cbor_to_bson(&v)),
            id_cbor: |id: &MongoId| bson_to_cbor(&bson::Bson::from(id.clone())),
        },
    );
    let handle = RuleHandle::new(Box::new(target.clone()));
    let host = Host::builder(Limits::background())
        .vocab_fn(register_rule_vocab)
        .vocab_fn(register_rule_conditions)
        .build();
    eval_rule_script(
        &host,
        "table.with_condition(#{bakery_id: 3})",
        Env::new(),
        &handle,
    )?;

    let table = target.into_table();
    assert_eq!(
        table.select().build_filter().await?,
        doc! { "bakery_id": 3_i64 }
    );
    assert!(matches!(
        handle.verbs().as_slice(),
        [RuleVerb::Condition { .. }]
    ));
    Ok(())
}
