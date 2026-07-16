use vantage_sql::postgres::PostgresDB;
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

// Referenced by the `#[entity]` macro expansion.
#[allow(unused_imports)]
use vantage_sql::postgres::AnyPostgresType;

/// One drink on the shelf. `created` is an epoch-millis stamp set when the
/// delivery arrives — the shelf is ordered by it, so drinks stay put as they
/// sell and new deliveries append at the end.
#[entity(PostgresType)]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
    pub created: i64,
}

impl Product {
    pub fn table(db: PostgresDB) -> Table<PostgresDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<i64>("stock")
            .with_column_of::<i64>("created")
    }
}
