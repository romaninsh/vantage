use bakery_model3::{Client, Product, SurrealConnection, SurrealDB};
use surreal_client::SurrealMockBuilder;
use vantage_expressions::Expressive;
use vantage_surrealdb::SurrealTableExt;
use vantage_surrealdb::surreal_expr;
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
async fn test_build_select_with_condition() {
    let db = get_db().await;
    let table = Product::surreal_table(db).with_condition(surreal_expr!("active = {}", true));

    let select = table.select();
    assert_eq!(
        select.preview(),
        "SELECT id, name, calories, price, is_deleted, sticker, bakery, inventory FROM product WHERE active = true"
    );
}

#[tokio::test]
async fn test_build_select_count_query() {
    let db = get_db().await;
    let table = Product::surreal_table(db);

    let select = table.select();
    let count_query = select.as_count();
    assert_eq!(
        count_query.expr().preview(),
        "RETURN count(SELECT VALUE id FROM product)"
    );
}

#[tokio::test]
async fn test_build_select_reserved_column_names() {
    // Rendering needs no server, so a mock engine stands in for the connection.
    let db = SurrealDB::new(SurrealMockBuilder::new().build());
    let table = Table::<SurrealDB, EmptyEntity>::new("message", db)
        .with_column_of::<String>("from")
        .with_column_of::<String>("return");

    assert_eq!(
        table.select().preview(),
        "SELECT ⟨from⟩, ⟨return⟩ FROM message"
    );
}

fn users_table(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("users", db)
        .with_column_of::<String>("name")
        .with_column_of::<String>("email")
        .with_column_of::<i64>("age")
}

#[tokio::test]
async fn test_ext_select_shapes() {
    let db = get_db().await;
    let table = users_table(db);

    assert_eq!(
        table.select().preview(),
        "SELECT name, email, age FROM users"
    );
    assert_eq!(
        table.select_first().preview(),
        "SELECT name, email, age FROM ONLY users"
    );
    assert_eq!(
        table.select_values("name").unwrap().preview(),
        "SELECT VALUE name FROM users"
    );
    assert_eq!(
        table.select_single("email").unwrap().preview(),
        "SELECT VALUE email FROM ONLY users"
    );
}

#[tokio::test]
async fn test_ext_select_unknown_column_is_error() {
    let db = get_db().await;
    let table = users_table(db);

    let err = table.select_values("nonexistent").unwrap_err();
    assert!(err.to_string().contains("Column not found in table"));
    let err = table.select_single("invalid").unwrap_err();
    assert!(err.to_string().contains("Column not found in table"));
}

#[tokio::test]
async fn test_ext_select_keeps_conditions() {
    let db = get_db().await;
    let table = users_table(db).with_condition(surreal_expr!("age > {}", 18));

    assert!(table.select().preview().contains("WHERE age > 18"));
    assert!(table.select_first().preview().contains("WHERE age > 18"));
    assert!(
        table
            .select_values("name")
            .unwrap()
            .preview()
            .contains("WHERE age > 18")
    );
    assert!(
        table
            .select_single("email")
            .unwrap()
            .preview()
            .contains("WHERE age > 18")
    );
}

#[tokio::test]
async fn test_ext_select_values_ignores_alias() {
    let db = get_db().await;
    let table = Table::<SurrealDB, EmptyEntity>::new("users", db)
        .with_column(Column::<String>::new("name").with_alias("user_name"))
        .with_column(Column::<String>::new("email").with_alias("user_email"));

    let query = table.select().preview();
    assert!(query.contains("name AS user_name"), "{query}");
    assert!(query.contains("email AS user_email"), "{query}");
    assert_eq!(
        table.select_values("name").unwrap().preview(),
        "SELECT VALUE name FROM users"
    );
}

// -- Live DB tests (v2 database, ingested by scripts/ingress.sh) --
// v2.surql defines: 5 products, 3 clients (2 paying), 3 orders

#[tokio::test]
async fn test_get_count_products() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let count = db.get_table_count(&table).await.unwrap();
    assert_eq!(count, 5);
}

#[tokio::test]
async fn test_get_count_with_condition() {
    let db = get_db().await;
    let table = Client::surreal_table(db.clone())
        .with_condition(surreal_expr!("is_paying_client = {}", true));
    let count = db.get_table_count(&table).await.unwrap();
    assert_eq!(count, 2);
}

// -- Aggregation tests (prices: 120, 135, 220, 299, 199) --

#[tokio::test]
async fn test_get_sum_product_prices() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_table_sum(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 973);
}

#[tokio::test]
async fn test_get_max_product_price() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_table_max(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 299);
}

#[tokio::test]
async fn test_get_min_product_price() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone());
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_table_min(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 120);
}

#[tokio::test]
async fn test_get_sum_with_condition() {
    let db = get_db().await;
    // products with calories <= 200: time_tart (price=220), hover_cookies (price=199) → 419
    let table =
        Product::surreal_table(db.clone()).with_condition(surreal_expr!("calories <= {}", 200));
    let col = Column::<AnySurrealType>::new("price");
    let result = db.get_table_sum(&table, &col).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 419);
}

// -- list_table_values tests --

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
    let record = db
        .get_table_value(&table, &id)
        .await
        .unwrap()
        .expect("product exists");
    let product = Product::from_record(record).unwrap();
    assert_eq!(product.name, "DeLorean Doughnut");
    assert_eq!(product.calories, 250);
    assert_eq!(product.price, 135);
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

/// `with_id` narrows the table to the one row with that id.
#[tokio::test]
async fn test_with_id_narrows_to_one_row() {
    let db = get_db().await;
    let table = Product::surreal_table(db.clone())
        .with_id(AnySurrealType::new(Thing::new("product", "flux_cupcake")))
        .unwrap();

    assert_eq!(db.get_table_count(&table).await.unwrap(), 1);
    let (id, record) = db
        .get_table_some_value(&table)
        .await
        .unwrap()
        .expect("the narrowed row");
    assert_eq!(id, Thing::new("product", "flux_cupcake"));
    assert_eq!(
        Product::from_record(record).unwrap().name,
        "Flux Capacitor Cupcake"
    );
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
