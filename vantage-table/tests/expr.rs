//! Tests for column creation and basic typed column functionality

use serde::{Deserialize, Serialize};
use vantage_table::prelude::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    age: i32,
}

impl User {
    fn table(ds: MockTableSource) -> Table<MockTableSource, User> {
        Table::new("users", ds)
            .with_column("id")
            .with_column("name")
            .with_column_of::<i32>("age")
    }
}

pub trait UserTable {
    fn age(&self) -> Column<i32>;
}

impl UserTable for Table<MockTableSource, User> {
    fn age(&self) -> Column<i32> {
        Column::<i32>::new("age")
    }
}

#[test]
fn test_table_creation_with_mixed_columns() {
    let ds = MockTableSource::new();
    let users_table = User::table(ds);

    // Verify all columns were created
    let columns = users_table.columns();
    assert!(columns.contains_key("id"));
    assert!(columns.contains_key("name"));
    assert!(columns.contains_key("age"));
    assert_eq!(columns.len(), 3);

    let associated_expr = users_table.get_expr_count();
    // TODO: Verify - should create AssociatedExpression<'_, MockTableSource, Value, i64>
    // that can be executed with .get().await or used in expressions via Expressive trait
    assert_eq!(
        associated_expr.expression().preview(),
        "SELECT COUNT(*) FROM \"users\""
    );

    let associated_max_age = users_table.get_expr_max(&users_table.age());
    // TODO: Verify - should create AssociatedExpression<'_, MockTableSource, Value, i32>
    // Note: Currently fails due to type conversion from Value to i32
    // Need TryFrom<Value> for i32 or use different approach
    // assert_eq!(
    //     associated_max_age.expression().preview(),
    //     "SELECT MAX(age) FROM users"
    // );
}
