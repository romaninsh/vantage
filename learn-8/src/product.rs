use vantage_sql::prelude::*;
use vantage_types::prelude::*;

/// One product on the bar's shelf. `stock` counts the units on hand — a sale
/// decrements it, a delivery tops it up. Nothing here knows it will be cached,
/// watched, or (later) served from PostgreSQL instead of SQLite.
#[entity(SqliteType)]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
}

impl Product {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<i64>("stock")
    }
}
