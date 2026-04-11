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

use crate::Client;

#[entity(CsvType, SurrealType, SqliteType, PostgresType, MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Order {
    pub client_id: Option<String>,
    pub is_deleted: bool,
    pub created_at: Option<String>,
    pub lines: Option<String>, // JSON stored as string in CSV
}

impl Order {
    pub fn csv_table(csv: Csv) -> Table<Csv, Order> {
        Table::new("order", csv)
            .with_column_of::<String>("client_id")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<String>("created_at")
            .with_column_of::<String>("lines")
            .with_one("client", "client_id", Client::csv_table)
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Order> {
        Table::new("order", db)
            .with_id_column("id")
            .with_column_of::<bool>("is_deleted")
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Order> {
        Table::new("client_order", db)
            .with_id_column("id")
            .with_column_of::<String>("client_id")
            .with_column_of::<bool>("is_deleted")
            .with_one("client", "client_id", Client::sqlite_table)
    }

    pub fn postgres_table(db: PostgresDB) -> Table<PostgresDB, Order> {
        Table::new("client_order", db)
            .with_id_column("id")
            .with_column_of::<String>("client_id")
            .with_column_of::<bool>("is_deleted")
            .with_one("client", "client_id", Client::postgres_table)
    }

    pub fn mongo_table(db: MongoDB) -> Table<MongoDB, Order> {
        Table::new("client_order", db)
            .with_id_column("_id")
            .with_column_of::<String>("client_id")
            .with_column_of::<bool>("is_deleted")
            .with_one("client", "client_id", Client::mongo_table)
    }
}
