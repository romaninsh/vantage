use vantage_csv::{AnyCsvType, Csv};
use vantage_mongodb::{AnyMongoType, MongoDB};
#[allow(unused_imports)]
use vantage_sql::postgres::AnyPostgresType;
use vantage_sql::postgres::PostgresDB;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_types::entity;

use crate::{Animal, Bakery};

#[entity(CsvType, SurrealType, SqliteType, PostgresType, MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Product {
    pub name: String,
    pub calories: i64,
    pub price: i64,
    pub is_deleted: bool,
    pub sticker: Option<Animal>,
}

impl Product {
    pub fn csv_table(csv: Csv) -> Table<Csv, Product> {
        Table::new("product", csv)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<Option<Animal>>("sticker")
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<Option<Animal>>("sticker")
            .with_column_of::<String>("bakery")
            .with_one("bakery", "bakery", Bakery::surreal_table)
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<Option<Animal>>("sticker")
            .with_one("bakery", "bakery_id", Bakery::sqlite_table)
    }

    pub fn postgres_table(db: PostgresDB) -> Table<PostgresDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<Option<Animal>>("sticker")
            .with_one("bakery", "bakery_id", Bakery::postgres_table)
    }

    pub fn mongo_table(db: MongoDB) -> Table<MongoDB, Product> {
        Table::new("product", db)
            .with_id_column("_id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<Option<Animal>>("sticker")
    }
}
