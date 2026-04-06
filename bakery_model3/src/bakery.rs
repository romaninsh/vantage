use vantage_csv::{AnyCsvType, Csv};
#[allow(unused_imports)]
use vantage_sql::postgres::AnyPostgresType;
use vantage_sql::postgres::PostgresDB;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_types::entity;

#[entity(CsvType, SurrealType, SqliteType, PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bakery {
    pub name: String,
    pub profit_margin: i64,
}

impl Bakery {
    pub fn csv_table(csv: Csv) -> Table<Csv, Bakery> {
        Table::new("bakery", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Bakery> {
        let db2 = db.clone();
        Table::new("bakery", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
            .with_many("products", "bakery", move || {
                crate::Product::surreal_table(db2.clone())
            })
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Bakery> {
        Table::new("bakery", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
    }

    pub fn postgres_table(db: PostgresDB) -> Table<PostgresDB, Bakery> {
        Table::new("bakery", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
    }
}
