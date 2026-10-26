//! Test 3: MongoSelect builder + SelectableDataSource execution against seeded v2 data.
//!
//! Requires a running MongoDB with v2.js loaded.
//! Builder tests (preview, build_*) are sync. Live tests need the database.

use bson::doc;
use vantage_expressions::{Order, Selectable};
use vantage_mongodb::{AnyMongoType, MongoDB, MongoSelect};
use vantage_table::column::core::Column;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn db() -> MongoDB {
    MongoDB::connect(&mongo_url(), "vantage")
        .await
        .expect("Failed to connect to MongoDB")
}

// ── Builder / preview tests (no database needed) ─────────────────────────

#[test]
fn test_select_all() {
    let s = MongoSelect::new().with_source("product");
    assert_eq!(s.preview(), "db.product.find({})");
}

#[test]
fn test_select_fields() {
    let s = MongoSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price");
    let p = s.preview();
    assert!(p.starts_with("db.product.find({})"));
    assert!(p.contains(".projection("));
}

#[test]
fn test_select_with_condition() {
    let s = MongoSelect::new()
        .with_source("product")
        .with_condition(doc! { "price": { "$gt": 100 } });
    assert!(s.preview().contains("<1 conditions>"));
}

#[test]
fn test_select_order_and_limit() {
    let s = MongoSelect::new()
        .with_source("product")
        .with_order(doc! { "price": 1 }, Order::Desc)
        .with_limit(Some(2), None);
    let p = s.preview();
    assert!(p.contains(".sort("));
    assert!(p.contains(".limit(2)"));
}

#[test]
fn test_select_distinct() {
    let mut s = MongoSelect::new().with_source("product").with_field("name");
    s.set_distinct(true);
    assert!(s.is_distinct());
}

#[test]
fn test_as_count_preview() {
    let s = MongoSelect::new().with_source("product");
    let expr = s.as_count();
    assert_eq!(expr.preview(), "db.product.countDocuments()");
}

#[test]
fn test_select_preview_shows_skip_and_limit() {
    let s = MongoSelect::new()
        .with_source("product")
        .with_limit(Some(10), Some(20));
    let p = s.preview();
    assert!(p.contains(".skip(20)"), "{p}");
    assert!(p.contains(".limit(10)"), "{p}");
}

#[test]
fn test_as_field_preview_names_collection_field_and_condition_count() {
    let base = MongoSelect::new().with_source("product");
    let none = base.as_field("name").preview();
    assert!(none.contains("product") && none.contains("name"), "{none}");

    let one = base
        .clone()
        .with_condition(doc! { "price": { "$gt": 100 } })
        .as_field("name")
        .preview();
    let two = base
        .with_condition(doc! { "price": { "$gt": 100 } })
        .with_condition(doc! { "active": true })
        .as_field("name")
        .preview();
    assert!(one.contains("product") && one.contains("name"), "{one}");
    assert!(two.contains("product") && two.contains("name"), "{two}");
    assert!(one != none && two != one, "{none} / {one} / {two}");
}

#[test]
fn test_build_projection() {
    let s = MongoSelect::new().with_field("name").with_field("price");
    let proj = s.build_projection().unwrap();
    assert_eq!(proj, doc! { "name": 1, "price": 1 });
}

#[test]
fn test_build_projection_empty_means_all() {
    let s = MongoSelect::new();
    assert!(s.build_projection().is_none());
}

#[test]
fn test_build_sort() {
    let s = MongoSelect::new()
        .with_order(doc! { "price": 1 }, Order::Asc)
        .with_order(doc! { "name": 1 }, Order::Desc);
    let sort = s.build_sort().unwrap();
    assert_eq!(sort, doc! { "price": 1, "name": -1 });
}

#[test]
fn test_build_find_options() {
    let s = MongoSelect::new()
        .with_field("name")
        .with_limit(Some(5), Some(10));
    let opts = s.build_find_options();
    assert!(opts.projection.is_some());
    assert_eq!(opts.limit, Some(5));
    assert_eq!(opts.skip, Some(10));
}

#[tokio::test]
async fn test_build_filter_empty() {
    let s = MongoSelect::new();
    let filter = s.build_filter().await.unwrap();
    assert_eq!(filter, doc! {});
}

#[tokio::test]
async fn test_build_filter_single() {
    let s = MongoSelect::new().with_condition(doc! { "active": true });
    let filter = s.build_filter().await.unwrap();
    assert_eq!(filter, doc! { "active": true });
}

#[tokio::test]
async fn test_build_filter_multiple_uses_and() {
    let s = MongoSelect::new()
        .with_condition(doc! { "active": true })
        .with_condition(doc! { "price": { "$gt": 100 } });
    let filter = s.build_filter().await.unwrap();
    assert_eq!(
        filter,
        doc! { "$and": [{ "active": true }, { "price": { "$gt": 100 } }] }
    );
}

#[tokio::test]
async fn test_count_pipeline_empty() {
    let s = MongoSelect::new();
    let pipeline = s.as_count_pipeline().await.unwrap();
    assert_eq!(pipeline.len(), 1);
    assert_eq!(pipeline[0], doc! { "$count": "count" });
}

#[tokio::test]
async fn test_count_pipeline_with_filter() {
    let s = MongoSelect::new().with_condition(doc! { "is_deleted": false });
    let pipeline = s.as_count_pipeline().await.unwrap();
    assert_eq!(pipeline.len(), 2);
    assert_eq!(pipeline[0], doc! { "$match": { "is_deleted": false } });
    assert_eq!(pipeline[1], doc! { "$count": "count" });
}

#[tokio::test]
async fn test_aggregate_pipeline_sum() {
    let s = MongoSelect::new();
    let pipeline = s.as_aggregate_pipeline("$sum", "price").await.unwrap();
    assert_eq!(pipeline.len(), 1);
    assert_eq!(
        pipeline[0],
        doc! { "$group": { "_id": null, "val": { "$sum": "$price" } } }
    );
}

// ── Clear / has methods ──────────────────────────────────────────────────

#[test]
fn test_clear_and_has() {
    let mut s = MongoSelect::new()
        .with_source("product")
        .with_field("name")
        .with_condition(doc! { "a": 1 })
        .with_order(doc! { "price": 1 }, Order::Asc)
        .with_limit(Some(10), None);

    assert!(s.has_fields());
    assert!(s.has_where_conditions());
    assert!(s.has_order_by());
    assert_eq!(s.get_limit(), Some(10));
    assert_eq!(s.get_skip(), None);

    s.clear_fields();
    s.clear_where_conditions();
    s.clear_order_by();

    assert!(!s.has_fields());
    assert!(!s.has_where_conditions());
    assert!(!s.has_order_by());
    // limit untouched
    assert_eq!(s.get_limit(), Some(10));
}

#[test]
fn test_group_by_can_be_added_checked_and_cleared() {
    let mut s = MongoSelect::new().with_source("product");
    assert!(!s.has_group_by());

    s.add_group_by(Column::<AnyMongoType>::new("category"));
    assert!(s.has_group_by());
    // Grouping leaves the other parts of the select alone.
    assert!(!s.has_fields() && !s.has_where_conditions() && !s.has_order_by());

    s.clear_group_by();
    assert!(!s.has_group_by());
}

// ── Live execution via SelectableDataSource (seeded v2 data) ─────────────

#[tokio::test]
async fn test_execute_select_with_fields() {
    use vantage_expressions::SelectableDataSource;

    let db = db().await;
    let select = MongoSelect::new()
        .with_source("product")
        .with_field("name")
        .with_field("price");
    let results = db.execute_select(&select).await.unwrap();

    assert_eq!(results.len(), 5);
    // Each result should be a document with projected fields
    let first: vantage_types::Record<AnyMongoType> = results[0].clone().try_into().unwrap();
    assert!(first.get("name").is_some());
    assert!(first.get("price").is_some());
}

#[tokio::test]
async fn test_execute_select_with_condition() {
    use vantage_expressions::SelectableDataSource;

    let db = db().await;
    let select = MongoSelect::new()
        .with_source("product")
        .with_condition(doc! { "price": { "$gt": 200 } });
    let results = db.execute_select(&select).await.unwrap();

    // sea_pie (299) and time_tart (220) have price > 200
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_execute_select_with_order() {
    use vantage_expressions::SelectableDataSource;

    let db = db().await;
    let select = MongoSelect::new()
        .with_source("product")
        .with_order(doc! { "price": 1 }, Order::Asc);
    let results = db.execute_select(&select).await.unwrap();

    // Verify ascending price order
    let mut prices: Vec<i64> = Vec::new();
    for r in &results {
        let rec: vantage_types::Record<AnyMongoType> = r.clone().try_into().unwrap();
        if let Some(p) = rec
            .get("price")
            .and_then(|v| v.try_get::<i64>().or(v.try_get::<i32>().map(|i| i as i64)))
        {
            prices.push(p);
        }
    }
    assert_eq!(prices.len(), 5);
    for w in prices.windows(2) {
        assert!(w[0] <= w[1], "Expected ascending: {} <= {}", w[0], w[1]);
    }
}

#[tokio::test]
async fn test_execute_select_with_skip_and_limit() {
    use vantage_expressions::SelectableDataSource;

    let db = db().await;
    let select = MongoSelect::new()
        .with_source("product")
        .with_order(doc! { "price": 1 }, Order::Asc)
        .with_limit(Some(2), Some(1));
    let results = db.execute_select(&select).await.unwrap();

    assert_eq!(results.len(), 2);
    // Skipped the cheapest (120), should start from 135
    let rec: vantage_types::Record<AnyMongoType> = results[0].clone().try_into().unwrap();
    let price = rec["price"]
        .try_get::<i64>()
        .or(rec["price"].try_get::<i32>().map(|i| i as i64))
        .unwrap();
    assert_eq!(price, 135);
}

#[tokio::test]
async fn test_execute_select_multiple_conditions() {
    use vantage_expressions::SelectableDataSource;

    let db = db().await;
    let select = MongoSelect::new()
        .with_source("product")
        .with_condition(doc! { "is_deleted": false })
        .with_condition(doc! { "price": { "$gte": 199 } });
    let results = db.execute_select(&select).await.unwrap();

    // Not deleted AND price >= 199: sea_pie(299), time_tart(220), hover_cookies(199)
    assert_eq!(results.len(), 3);
}

// ── table.select() integration ───────────────────────────────────────────
// Verifies that Table<MongoDB, E> can produce a MongoSelect via table.select()

#[tokio::test]
async fn test_table_select_carries_collection_columns_and_conditions() {
    use vantage_expressions::SelectableDataSource;
    use vantage_table::table::Table;
    use vantage_types::EmptyEntity;

    let db = db().await;
    let mut table = Table::<MongoDB, EmptyEntity>::new("product", db.clone())
        .with_id_column("_id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted");
    table.add_condition(doc! { "price": { "$gt": 130 } });

    let select = table.select();
    assert_eq!(select.collection, Some("product".to_string()));
    // _id, name, price, is_deleted
    assert_eq!(select.fields.len(), 4);
    assert!(select.has_where_conditions());

    // Every v2 product but flux_cupcake (120) costs more than 130.
    let results = db.execute_select(&select).await.unwrap();
    assert_eq!(results.len(), 4);
}
