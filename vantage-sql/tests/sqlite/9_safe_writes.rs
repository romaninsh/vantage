//! The write contract on SQLite: confined, idempotent, invariant-filling.

use vantage_sql::sqlite::operation::SqliteOperation;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

/// A fresh in-memory database per test, so `_test` needs no table of its own.
async fn item(_test: &str) -> Table<SqliteDB, EmptyEntity> {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query(
        "CREATE TABLE item (id TEXT PRIMARY KEY, name TEXT, parent TEXT, price INTEGER, email TEXT UNIQUE)",
    )
    .execute(db.pool())
    .await
    .unwrap();
    Table::new("item", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price")
        .with_column_of::<String>("email")
}

safe_writes_tests!(SqliteDB, AnySqliteType);

/// A UNIQUE column declared `ON CONFLICT REPLACE` in the schema: a write that
/// clashes on it fails instead of deleting the row that holds the value.
#[tokio::test]
async fn a_schema_replace_policy_never_deletes_the_clashing_row() {
    use vantage_dataset::prelude::*;
    use vantage_types::Record;

    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE item (id TEXT PRIMARY KEY, email TEXT UNIQUE ON CONFLICT REPLACE)")
        .execute(db.pool())
        .await
        .unwrap();
    let all = Table::<SqliteDB, EmptyEntity>::new("item", db)
        .with_id_column("id")
        .with_column_of::<String>("email");
    let email = |s: &str| -> Record<AnySqliteType> {
        [("email".to_string(), AnySqliteType::from(s.to_string()))]
            .into_iter()
            .collect()
    };
    all.insert_value("a", &email("a@x.test")).await.unwrap();
    all.insert_value("b", &email("b@x.test")).await.unwrap();

    assert!(all.insert_value("c", &email("b@x.test")).await.is_err());
    assert!(all.replace_value("c", &email("b@x.test")).await.is_err());
    assert!(all.replace_value("a", &email("b@x.test")).await.is_err());
    assert!(all.patch_value("a", &email("b@x.test")).await.is_err());
    assert!(
        all.insert_return_id_value(&email("b@x.test"))
            .await
            .is_err()
    );

    let b = all.get_value("b").await.unwrap().expect("row b kept");
    assert_eq!(b["email"], AnySqliteType::from("b@x.test".to_string()));
    assert!(all.get_value("c").await.unwrap().is_none());
}

/// A column with a dotted alias (`city`, read as `address.city`) declares
/// that alias in full, not its root: a write keyed `address` names a column
/// the table doesn't declare, which SQLite would otherwise write.
#[tokio::test]
async fn the_root_of_a_dotted_alias_is_not_a_column() {
    use vantage_dataset::prelude::*;
    use vantage_table::column::core::Column;
    use vantage_types::Record;

    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE client (id TEXT PRIMARY KEY, city TEXT, address TEXT)")
        .execute(db.pool())
        .await
        .unwrap();
    let clients = Table::<SqliteDB, EmptyEntity>::new("client", db.clone())
        .with_id_column("id")
        .with_column(Column::<AnySqliteType>::new("city").with_alias("address.city"));
    let one = |key: &str, value: &str| -> Record<AnySqliteType> {
        [(key.to_string(), AnySqliteType::from(value.to_string()))]
            .into_iter()
            .collect()
    };
    clients
        .insert_value("c1", &one("city", "Hill Valley"))
        .await
        .unwrap();

    for err in [
        clients
            .patch_value("c1", &one("address", "x"))
            .await
            .unwrap_err(),
        clients
            .insert_value("c2", &one("address", "x"))
            .await
            .unwrap_err(),
    ] {
        assert!(err.is_forbidden(), "{err}");
    }
    let (address,): (Option<String>,) =
        sqlx::query_as("SELECT address FROM client WHERE id = 'c1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(address, None);
}
