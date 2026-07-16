use vantage_sql::prelude::*;
use vantage_types::prelude::*;

// The `#[entity]` marker below expands to impls over the backend's value type
// (`AnySqliteType` / `AnyPostgresType`); bring the selected one into scope.
#[cfg(not(feature = "pg"))]
#[allow(unused_imports)]
use vantage_sql::sqlite::AnySqliteType;
#[cfg(feature = "pg")]
#[allow(unused_imports)]
use vantage_sql::postgres::AnyPostgresType;

use crate::db::Db;

/// The same product model as before — but its `#[entity]` marker now lists
/// whichever backend is compiled in, and its `table()` builder is written
/// against the [`Db`](crate::db::Db) alias. Nothing else about the model moves
/// when the database does.
#[cfg_attr(not(feature = "pg"), entity(SqliteType))]
#[cfg_attr(feature = "pg", entity(PostgresType))]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
}

impl Product {
    pub fn table(db: Db) -> Table<Db, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<i64>("stock")
    }
}
