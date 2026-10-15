//! Test 4: Conditions on Table<MongoDB, EmptyEntity> against seeded v2 data,
//! and the `MongoCondition` / `MongoOperation` filter builders.

use bson::{Bson, doc};
use vantage_dataset::prelude::*;
use vantage_expressions::{DeferredFn, ExpressiveEnum};
use vantage_mongodb::condition::resolve_conditions;
use vantage_mongodb::operation::MongoOperation;
use vantage_mongodb::{AnyMongoType, MongoCondition, MongoDB, MongoId};
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn get_db() -> MongoDB {
    MongoDB::connect(&mongo_url(), "vantage").await.unwrap()
}

fn product_table(db: MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::new("product", db)
        .with_id_column("_id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("calories")
        .with_column_of::<i64>("price")
        .with_column_of::<String>("bakery_id")
        .with_column_of::<bool>("is_deleted")
        .with_column_of::<i64>("inventory_stock")
}

#[tokio::test]
async fn test_multiple_conditions() {
    let db = get_db().await;
    let mut table = product_table(db);
    table.add_condition(doc! { "price": { "$gt": 130 } });
    table.add_condition(doc! { "$expr": { "$gt": ["$price", "$calories"] } });

    let products = table.list_values().await.unwrap();
    // price > 130 AND price > calories:
    // delorean_donut: 135 > 250? no
    // time_tart: 220 > 200? yes
    // sea_pie: 299 > 350? no
    // hover_cookies: 199 > 150? yes
    assert_eq!(products.len(), 2);
    assert!(products.contains_key(&MongoId::from("time_tart")));
    assert!(products.contains_key(&MongoId::from("hover_cookies")));
}

fn as_doc(condition: MongoCondition) -> bson::Document {
    match condition {
        MongoCondition::Doc(doc) => doc,
        other => panic!("expected an immediate filter document, got {other:?}"),
    }
}

#[test]
fn column_comparisons_build_one_operator_filter_each() {
    let price = Column::<i64>::new("price");
    assert_eq!(
        as_doc(price.eq(100i64)),
        doc! { "price": { "$eq": 100_i64 } }
    );
    assert_eq!(
        as_doc(price.ne(100i64)),
        doc! { "price": { "$ne": 100_i64 } }
    );
    assert_eq!(
        as_doc(price.gt(100i64)),
        doc! { "price": { "$gt": 100_i64 } }
    );
    assert_eq!(
        as_doc(price.gte(100i64)),
        doc! { "price": { "$gte": 100_i64 } }
    );
    assert_eq!(
        as_doc(price.lt(100i64)),
        doc! { "price": { "$lt": 100_i64 } }
    );
    assert_eq!(
        as_doc(price.lte(100i64)),
        doc! { "price": { "$lte": 100_i64 } }
    );
}

#[test]
fn column_comparison_takes_a_string_operand() {
    let name = Column::<String>::new("name");
    assert_eq!(
        as_doc(name.eq("Alice")),
        doc! { "name": { "$eq": "Alice" } }
    );
}

#[test]
fn column_in_builds_an_in_list() {
    let status = Column::<String>::new("status");
    assert_eq!(
        as_doc(status.in_(vec!["active", "pending"])),
        doc! { "status": { "$in": ["active", "pending"] } }
    );
}

#[test]
fn null_checks_match_null_or_missing_and_non_null() {
    let deleted_at = Column::<String>::new("deleted_at");
    assert_eq!(
        as_doc(deleted_at.is_null()),
        doc! { "deleted_at": Bson::Null }
    );
    assert_eq!(
        as_doc(deleted_at.is_not_null()),
        doc! { "deleted_at": { "$ne": Bson::Null } }
    );
}

#[test]
fn eq_bool_false_wraps_the_condition_in_not_and_true_keeps_it() {
    let price = Column::<i64>::new("price");
    assert_eq!(
        as_doc(price.gt(10i64).eq_bool(false)),
        doc! { "price": { "$not": { "$gt": 10_i64 } } }
    );
    assert_eq!(
        as_doc(price.gt(10i64).eq_bool(true)),
        doc! { "price": { "$gt": 10_i64 } }
    );
}

#[test]
fn ne_bool_true_negates_and_false_keeps_the_condition() {
    let price = Column::<i64>::new("price");
    assert_eq!(
        as_doc(price.gt(10i64).ne_bool(true)),
        doc! { "price": { "$not": { "$gt": 10_i64 } } }
    );
    assert_eq!(
        as_doc(price.gt(10i64).ne_bool(false)),
        doc! { "price": { "$gt": 10_i64 } }
    );
}

#[test]
fn negating_a_plain_value_filter_negates_its_equality() {
    let active = MongoCondition::Doc(doc! { "active": true });
    assert_eq!(
        as_doc(active.eq_bool(false)),
        doc! { "active": { "$not": { "$eq": true } } }
    );
}

#[tokio::test]
async fn a_document_condition_resolves_to_itself() {
    let condition: MongoCondition = doc! { "price": { "$gt": 100 } }.into();
    assert_eq!(
        condition.resolve().await.unwrap(),
        doc! { "price": { "$gt": 100 } }
    );
}

#[tokio::test]
async fn and_combines_two_conditions_under_dollar_and() {
    let condition = MongoCondition::And(vec![doc! { "a": 1 }.into(), doc! { "b": 2 }.into()]);
    assert_eq!(
        condition.resolve().await.unwrap(),
        doc! { "$and": [{ "a": 1 }, { "b": 2 }] }
    );
}

#[tokio::test]
async fn and_of_one_condition_is_that_condition_and_of_none_matches_everything() {
    let single = MongoCondition::And(vec![doc! { "x": 1 }.into()]);
    assert_eq!(single.resolve().await.unwrap(), doc! { "x": 1 });
    assert_eq!(
        MongoCondition::And(vec![]).resolve().await.unwrap(),
        doc! {}
    );
}

#[tokio::test]
async fn resolve_conditions_merges_a_list_like_and() {
    let conditions = [
        MongoCondition::Doc(doc! { "a": 1 }),
        MongoCondition::Doc(doc! { "b": 2 }),
    ];
    assert_eq!(
        resolve_conditions(conditions.iter()).await.unwrap(),
        doc! { "$and": [{ "a": 1 }, { "b": 2 }] }
    );
}

fn deferred_resolving_to(value: Bson) -> MongoCondition {
    MongoCondition::Deferred(DeferredFn::new(move || {
        let value = value.clone();
        Box::pin(async move { Ok(ExpressiveEnum::Scalar(AnyMongoType::untyped(value))) })
    }))
}

#[tokio::test]
async fn a_deferred_condition_resolves_to_the_document_it_produces() {
    let condition = deferred_resolving_to(Bson::Document(doc! { "status": "active" }));
    assert_eq!(
        condition.resolve().await.unwrap(),
        doc! { "status": "active" }
    );
}

#[tokio::test]
async fn a_deferred_condition_nests_inside_and() {
    let condition = MongoCondition::And(vec![
        doc! { "active": true }.into(),
        deferred_resolving_to(Bson::Document(doc! { "owner_id": { "$in": ["a", "b"] } })),
    ]);
    assert_eq!(
        condition.resolve().await.unwrap(),
        doc! { "$and": [
            { "active": true },
            { "owner_id": { "$in": ["a", "b"] } }
        ] }
    );
}

#[tokio::test]
async fn a_deferred_condition_that_is_not_a_document_fails_to_resolve() {
    let condition = deferred_resolving_to(Bson::Int32(7));
    assert!(condition.resolve().await.is_err());
}

#[test]
fn debug_output_names_the_variant_and_its_content() {
    let and = MongoCondition::And(vec![doc! { "price": 1 }.into()]);
    let shown = format!("{and:?}");
    assert!(shown.contains("And") && shown.contains("price"), "{shown}");
    let deferred = deferred_resolving_to(Bson::Null);
    assert!(format!("{deferred:?}").contains("Deferred"));
}

async fn throwaway() -> MongoDB {
    let name = format!("vantage_cond_{}", bson::oid::ObjectId::new().to_hex());
    MongoDB::connect(&mongo_url(), &name).await.unwrap()
}

/// Four rows covering every (a, b) pair of {1, 2} x {1, 2}.
async fn grid(db: &MongoDB) -> Table<MongoDB, EmptyEntity> {
    db.doc_collection("grid")
        .insert_many(vec![
            doc! { "_id": "11", "a": 1_i64, "b": 1_i64 },
            doc! { "_id": "12", "a": 1_i64, "b": 2_i64 },
            doc! { "_id": "21", "a": 2_i64, "b": 1_i64 },
            doc! { "_id": "22", "a": 2_i64, "b": 2_i64 },
        ])
        .await
        .unwrap();
    Table::<MongoDB, EmptyEntity>::new("grid", db.clone())
        .with_id_column("_id")
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
}

#[tokio::test]
#[ignore = "possible bug: eq_bool(false) on And negates each part and keeps the $and, so NOT (a AND b) selects only rows failing both parts"]
async fn negating_an_and_selects_every_row_the_and_rejects() {
    let db = throwaway().await;
    let mut table = grid(&db).await;
    let both = MongoCondition::And(vec![doc! { "a": 1_i64 }.into(), doc! { "b": 1_i64 }.into()]);
    table.add_condition(both.eq_bool(false));

    let mut ids: Vec<String> = table
        .list_values()
        .await
        .unwrap()
        .keys()
        .map(|id| id.to_string())
        .collect();
    ids.sort();
    db.database().drop().await.ok();
    assert_eq!(ids, ["12", "21", "22"]);
}

#[tokio::test]
#[ignore = "possible bug: eq_bool(false) on a deferred condition returns it unnegated"]
async fn negating_a_deferred_condition_negates_what_it_resolves_to() {
    let deferred = deferred_resolving_to(Bson::Document(doc! { "a": { "$gt": 1_i64 } }));
    let negated = deferred.eq_bool(false).resolve().await.unwrap();
    assert_ne!(negated, doc! { "a": { "$gt": 1_i64 } });
}
