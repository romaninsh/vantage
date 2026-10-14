use serde_json::json;
use vantage_expressions::expr_any;
use vantage_types::Record;

use super::MockTableSource;
use crate::column::flags::ColumnFlag;
use crate::mocks::mock_column::MockColumn;
use crate::table::Table;
use crate::traits::table_source::TableSource;

#[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq, Default, Clone)]
struct TestUser {
    id: i32,
    name: String,
}

#[tokio::test]
async fn test_mock_table_source_with_data() {
    let mock = MockTableSource::new()
        .with_data(
            "users",
            vec![
                json!({"id": "1", "name": "Alice"}),
                json!({"id": "2", "name": "Bob"}),
            ],
        )
        .await;

    let table = Table::<MockTableSource, TestUser>::new("users", mock).into_entity::<TestUser>();
    let count = table.data_source().get_table_count(&table).await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_count_reflects_writes_and_count_expr_no_block_on() {
    use vantage_expressions::traits::datasource::ExprDataSource;

    let mock = MockTableSource::new()
        .with_data(
            "users",
            vec![
                json!({"id": "1", "name": "Alice"}),
                json!({"id": "2", "name": "Bob"}),
            ],
        )
        .await
        .with_query_source(vantage_expressions::mocks::mock_builder::new());
    let table = Table::<MockTableSource, TestUser>::new("users", mock);

    // Insert a third row; count derives from the single store, so it must
    // reflect the write.
    let mut rec = Record::new();
    rec.insert("name".to_string(), json!("Carol"));
    table
        .data_source()
        .insert_table_value(&table, &"3".to_string(), &rec)
        .await
        .unwrap();
    assert_eq!(
        table.data_source().get_table_count(&table).await.unwrap(),
        3
    );

    // get_expr_count() reads the count synchronously and seeds the query
    // source (no Handle::block_on), so building and executing it under a
    // runtime returns the live count.
    let count_expr = table.get_expr_count();
    let raw = table
        .data_source()
        .execute(count_expr.expression())
        .await
        .unwrap();
    assert_eq!(raw, json!(3));
}

#[tokio::test]
async fn test_list_count_and_get_honour_literal_conditions() {
    let mock = MockTableSource::new()
        .with_data(
            "users",
            vec![
                json!({"id": "1", "name": "Alice"}),
                json!({"id": "2", "name": "Bob"}),
            ],
        )
        .await;
    let all = Table::<MockTableSource, TestUser>::new("users", mock.clone());
    let mut set = all.clone();
    set.add_condition(mock.eq_value_condition("name", json!("Alice")).unwrap());

    let listed = set.data_source().list_table_values(&set).await.unwrap();
    assert_eq!(listed.keys().collect::<Vec<_>>(), ["1"]);
    assert_eq!(set.data_source().get_table_count(&set).await.unwrap(), 1);
    let (src, id) = (set.data_source(), "2".to_string());
    assert!(src.get_table_value(&set, &id).await.unwrap().is_none());
    assert_eq!(all.data_source().get_table_count(&all).await.unwrap(), 2);
}

async fn users_table() -> Table<MockTableSource, TestUser> {
    let mock = MockTableSource::new()
        .with_data(
            "users",
            vec![
                json!({"id": "1", "name": "Alice"}),
                json!({"id": "2", "name": "Bob"}),
            ],
        )
        .await;
    Table::<MockTableSource, TestUser>::new("users", mock)
        .with_column(MockColumn::<String>::new("name"))
}

async fn searched(mut set: Table<MockTableSource, TestUser>, needle: &str) -> Vec<String> {
    let search = set.data_source().search_table_condition(&set, needle);
    set.add_condition(search);
    let listed = set.data_source().list_table_values(&set).await.unwrap();
    listed.into_keys().collect()
}

#[tokio::test]
async fn test_search_condition_filters_by_case_insensitive_substring() {
    let mut set = users_table().await;
    let search = set.data_source().search_table_condition(&set, "LIC");
    set.add_condition(search);

    let listed = set.data_source().list_table_values(&set).await.unwrap();
    assert_eq!(listed.keys().collect::<Vec<_>>(), ["1"]);
    assert_eq!(set.data_source().get_table_count(&set).await.unwrap(), 1);
}

#[tokio::test]
async fn search_reads_only_searchable_columns() {
    let mock = MockTableSource::new()
        .with_data(
            "users",
            vec![
                json!({"id": "1", "name": "Alice", "note": "bob's friend"}),
                json!({"id": "2", "name": "Bob", "note": "quiet"}),
            ],
        )
        .await;
    let plain =
        Table::<MockTableSource, TestUser>::new("users", mock)
            .with_column(MockColumn::<String>::new("note"));
    assert_eq!(searched(plain.clone(), "bob").await, ["1"]);

    let flagged =
        plain.with_column(MockColumn::<String>::new("name").with_flag(ColumnFlag::Searchable));
    assert_eq!(searched(flagged.clone(), "bob").await, ["2"]);
    assert!(searched(flagged, "quiet").await.is_empty());
}

#[tokio::test]
async fn test_unknown_condition_is_unsupported() {
    let mut set = users_table().await;
    set.add_condition(expr_any!("age > {}", 30));

    let err = set.data_source().list_table_values(&set).await.unwrap_err();
    assert_eq!(err.kind(), vantage_core::ErrorKind::Unsupported);
    let err = set.data_source().get_table_count(&set).await.unwrap_err();
    assert_eq!(err.kind(), vantage_core::ErrorKind::Unsupported);
}

#[tokio::test]
#[should_panic(expected = "has no `id` field")]
async fn test_with_data_panics_on_idless_row() {
    // A seed row without an id fails loudly rather than vanishing from reads.
    let _ = MockTableSource::new()
        .with_data("users", vec![json!({"name": "NoId"})])
        .await;
}
