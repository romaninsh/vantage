use vantage_csv::{AnyCsvType, Csv};
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_types::entity;

use crate::Bakery;

#[entity(CsvType, SurrealType, SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Product {
    pub name: String,
    pub calories: i64,
    pub price: i64,
    pub is_deleted: bool,
}

impl Product {
    pub fn csv_table(csv: Csv) -> Table<Csv, Product> {
        Table::new("product", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
        let db2 = db.clone();
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_one("bakery", "bakery_id", move || {
                Bakery::sqlite_table(db2.clone())
            })
    }
}
