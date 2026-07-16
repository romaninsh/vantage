use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_table::table::Table;
use vantage_types::entity;

// Referenced by the `#[entity]` macro expansion.
#[allow(unused_imports)]
use vantage_surrealdb::types::AnySurrealType;

/// One drink on the shelf. `created` is an epoch-millis stamp set when the
/// delivery arrives — the shelf is ordered by it, so drinks stay put as they
/// sell and new deliveries append at the end.
///
/// This is the exact same entity as the Postgres chapter, minus the backend
/// name in the attribute: `SurrealType` instead of `PostgresType`.
#[entity(SurrealType)]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
    pub created: i64,
}

impl Product {
    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<i64>("stock")
            .with_column_of::<i64>("created")
    }
}
