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

use crate::{Bakery, Order};

#[entity(CsvType, SurrealType, SqliteType, PostgresType, MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
    pub bakery_id: Option<String>,
}

impl Client {
    pub fn csv_table(csv: Csv) -> Table<Csv, Client> {
        Table::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::csv_table)
            .with_many("orders", "client_id", Order::csv_table)
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Client> {
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_one("bakery", "bakery", Bakery::surreal_table)
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Client> {
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::sqlite_table)
            .with_many("orders", "client_id", Order::sqlite_table)
    }

    pub fn postgres_table(db: PostgresDB) -> Table<PostgresDB, Client> {
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::postgres_table)
            .with_many("orders", "client_id", Order::postgres_table)
    }

    pub fn mongo_table(db: MongoDB) -> Table<MongoDB, Client> {
        Table::new("client", db)
            .with_id_column("_id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::mongo_table)
            .with_many("orders", "client_id", Order::mongo_table)
    }
}
