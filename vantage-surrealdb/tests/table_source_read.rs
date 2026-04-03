use bakery_model3::{Bakery, Client, Order, Product, SurrealConnection, SurrealDB};
use vantage_expressions::Expressive;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::impls::build_select;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, TryFromRecord};

async fn get_db() -> SurrealDB {
    let client = SurrealConnection::dsn("cbor://root:root@localhost:8000/bakery/v2")
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

// -- Query generation tests --

#[tokio::test]
async fn test_build_select_basic() {
    let db = get_db().await;
    let table = Product::surreal_table(db);

    let select = build_select::build_select(&table);
    assert_eq!(
        select.preview(),
        "SELECT id, name, calories, price, is_deleted FROM product"
    );
}

#[tokio::test]
async fn test_build_select_with_condition() {
    let db = get_db().await;
    let table = Product::surreal_table(db).with_condition(surreal_expr!("active = {}", true));

    let select = build_select::build_select(&table);
    assert_eq!(
        select.preview(),
        "SELECT id, name, calories, price, is_deleted FROM product WHERE active = true"
    );
}

#[tokio::test]
async fn test_build_select_count_query() {
    let db = get_db().await;
    let table = Product::surreal_table(db);

    let select = build_select::build_select(&table);
    let count_query = select.as_count();
    assert_eq!(
        count_query.expr().preview(),
        "RETURN count(SELECT VALUE id FROM product)"
    );
}

// -- Live DB tests (v2 database, ingested by scripts/ingress.sh) --
// v2.surql defines: 5 products, 3 clients (2 paying), 3 orders

#[tokio::test]
async fn test_get_count_products() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let count = db.get_count(&table).await.unwrap();
    assert_eq!(count, 5);
}

#[tokio::test]
async fn test_get_count_clients() {
    let db = get_db().await;
    let table = Client::surreal_table(db.clone());
    let count = db.get_count(&table).await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_get_count_with_condition() {
    let db = get_db().await;
    let table = Client::surreal_table(db.clone())
        .with_condition(surreal_expr!("is_paying_client = {}", true));
    let count = db.get_count(&table).await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_get_count_orders() {
    let db = get_db().await;
    let table = Order::surreal_table(db.clone());
    let count = db.get_count(&table).await.unwrap();
    assert_eq!(count, 3);
}

// -- Aggregation tests (prices: 120, 135, 220, 299, 199) --

#[tokio::test]
async fn test_get_sum_product_prices() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_sum(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 973);
}

#[tokio::test]
async fn test_get_max_product_price() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_max(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 299);
}

#[tokio::test]
async fn test_get_min_product_price() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_min(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 120);
}

#[tokio::test]
async fn test_get_sum_with_condition() {
    let db = get_db().await;
    // products with calories <= 200: time_tart (price=220), hover_cookies (price=199) → 419
    let table =
        Product::surreal_table(db.clone()).with_condition(surreal_expr!("calories <= {}", 200));
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_sum(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 419);
}

// -- list_table_values tests --

#[tokio::test]
async fn test_list_table_values_bakery() {
    let db = get_db().await;
    let table = Bakery::surreal_table(db.clone());

    let values = table.data_source().list_table_values(&table).await.unwrap();
    assert_eq!(values.len(), 1);

    let hill_valley_id = Thing::new("bakery", "hill_valley");
    let record = &values[&hill_valley_id];
    let bakery = Bakery::from_record(record.clone()).unwrap();
    assert_eq!(bakery.name, "Hill Valley Bakery");
    assert_eq!(bakery.profit_margin, 15);
}

#[tokio::test]
async fn test_list_table_values_products() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());

    let values = db.list_table_values(&table).await.unwrap();
    assert_eq!(values.len(), 5);

    let cupcake_id = Thing::new("product", "flux_cupcake");
    let record = &values[&cupcake_id];
    let cupcake = Product::from_record(record.clone()).unwrap();
    assert_eq!(cupcake.name, "Flux Capacitor Cupcake");
    assert_eq!(cupcake.price, 120);
    assert_eq!(cupcake.calories, 300);
    assert!(!cupcake.is_deleted);
}

#[tokio::test]
async fn test_list_table_values_with_condition() {
    let db = get_db().await;
    let table = Client::surreal_table(db.clone())
        .with_condition(surreal_expr!("is_paying_client = {}", true));

    let values = db.list_table_values(&table).await.unwrap();
    assert_eq!(values.len(), 2);

    // Verify we got actual Client structs
    for (_id, record) in &values {
        let client = Client::from_record(record.clone()).unwrap();
        assert!(client.is_paying_client);
    }
}

// -- get_table_value tests --

#[tokio::test]
async fn test_get_table_value_product() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());

    let id = Thing::new("product", "delorean_donut");
    let record = db.get_table_value(&table, &id).await.unwrap();
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "DeLorean Doughnut");
    assert_eq!(product.calories, 250);
    assert_eq!(product.price, 135);
}

#[tokio::test]
async fn test_get_table_value_client() {
    let db = get_db().await;
    let table = Client::surreal_table(db.clone());

    let id = Thing::new("client", "doc");
    let record = db.get_table_value(&table, &id).await.unwrap();
    let client = Client::from_record(record).unwrap();
    assert_eq!(client.name, "Doc Brown");
    assert_eq!(client.email, "doc@brown.com");
    assert!(client.is_paying_client);
}

// -- get_table_some_value tests --

#[tokio::test]
async fn test_get_table_some_value_product() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());

    let result = db.get_table_some_value(&table).await.unwrap();
    assert!(result.is_some());
    let (_id, record) = result.unwrap();
    let product = Product::from_record(record).unwrap();
    assert!(!product.name.is_empty());
    assert!(product.price > 0);
}

#[tokio::test]
async fn test_get_table_some_value_nonexistent() {
    let db = get_db().await;
    // SurrealDB errors on nonexistent tables in strict mode
    let table = Table::<SurrealDB, EmptyEntity>::new("nonexistent_table_xyz", db.clone())
        .with_id_column("id");

    let result = db.get_table_some_value(&table).await;
    assert!(result.is_err());
}
