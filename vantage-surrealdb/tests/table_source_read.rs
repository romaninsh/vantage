use surreal_client::SurrealConnection;
use vantage_expressions::ExprDataSource;
use vantage_expressions::Expressive;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::surrealdb::impls::build_select;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;

const DB_URL: &str = "cbor://localhost:8000/rpc";
const TEST_NAMESPACE: &str = "bakery";
const TEST_DATABASE: &str = "table_source_read";

async fn get_db() -> SurrealDB {
    let client = SurrealConnection::new()
        .url(DB_URL)
        .namespace(TEST_NAMESPACE)
        .database(TEST_DATABASE)
        .auth_root("root", "root")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

// -- Query generation tests (no execution) --

#[tokio::test]
async fn test_build_select_basic() {
    let db = get_db().await;
    let table = Table::<_, EmptyEntity>::new("product", db);

    let select = build_select::build_select(&table);
    assert_eq!(select.preview(), "SELECT * FROM product");
}

#[tokio::test]
async fn test_build_select_with_columns() {
    let db = get_db().await;
    let table = Table::<_, EmptyEntity>::new("product", db)
        .with_column(Column::<String>::new("name"))
        .with_column(Column::<i64>::new("price"));

    let select = build_select::build_select(&table);
    assert_eq!(select.preview(), "SELECT name, price FROM product");
}

#[tokio::test]
async fn test_build_select_with_condition() {
    let db = get_db().await;
    let table = Table::<_, EmptyEntity>::new("product", db)
        .with_condition(surreal_expr!("active = {}", true));

    let select = build_select::build_select(&table);
    assert_eq!(
        select.preview(),
        "SELECT * FROM product WHERE active = true"
    );
}

#[tokio::test]
async fn test_build_select_count_query() {
    let db = get_db().await;
    let table = Table::<_, EmptyEntity>::new("product", db);

    let select = build_select::build_select(&table);
    let count_query = select.as_count();
    assert_eq!(
        count_query.expr().preview(),
        "RETURN count(SELECT VALUE id FROM product)"
    );
}

// -- Live DB tests --

#[tokio::test]
async fn test_get_count_empty() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE test_count_empty"))
        .await
        .unwrap();

    let table = Table::<_, EmptyEntity>::new("test_count_empty", db.clone());
    let count = db.get_count(&table).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_get_count_with_data() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE test_count_data"))
        .await
        .unwrap();
    for name in ["a", "b", "c"] {
        db.execute(&surreal_expr!("CREATE test_count_data SET name = {}", name))
            .await
            .unwrap();
    }

    let table = Table::<_, EmptyEntity>::new("test_count_data", db.clone());
    let count = db.get_count(&table).await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_get_count_with_condition() {
    let db = get_db().await;
    db.execute(&surreal_expr!("DELETE test_count_cond"))
        .await
        .unwrap();
    db.execute(&surreal_expr!(
        "CREATE test_count_cond SET name = {}, active = {}",
        "x",
        true
    ))
    .await
    .unwrap();
    db.execute(&surreal_expr!(
        "CREATE test_count_cond SET name = {}, active = {}",
        "y",
        false
    ))
    .await
    .unwrap();
    db.execute(&surreal_expr!(
        "CREATE test_count_cond SET name = {}, active = {}",
        "z",
        true
    ))
    .await
    .unwrap();

    let table = Table::<_, EmptyEntity>::new("test_count_cond", db.clone())
        .with_condition(surreal_expr!("active = {}", true));

    let count = db.get_count(&table).await.unwrap();
    assert_eq!(count, 2);
}
