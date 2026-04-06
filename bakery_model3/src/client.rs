use vantage_csv::{AnyCsvType, Csv};
#[allow(unused_imports)]
use vantage_sql::postgres::AnyPostgresType;
use vantage_sql::postgres::PostgresDB;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_types::entity;

use crate::{Bakery, Order};

#[entity(CsvType, SurrealType, SqliteType, PostgresType)]
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
        let csv2 = csv.clone();
        let csv3 = csv.clone();
        Table::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", move || {
                Bakery::csv_table(csv2.clone())
            })
            .with_many("orders", "client_id", move || {
                Order::csv_table(csv3.clone())
            })
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Client> {
        let db2 = db.clone();
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_one("bakery", "bakery", move || {
                Bakery::surreal_table(db2.clone())
            })
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Client> {
        let db2 = db.clone();
        let db3 = db.clone();
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", move || {
                Bakery::sqlite_table(db2.clone())
            })
            .with_many("orders", "client_id", move || {
                Order::sqlite_table(db3.clone())
            })
    }

    pub fn postgres_table(db: PostgresDB) -> Table<PostgresDB, Client> {
        let db2 = db.clone();
        let db3 = db.clone();
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", move || {
                Bakery::postgres_table(db2.clone())
            })
            .with_many("orders", "client_id", move || {
                Order::postgres_table(db3.clone())
            })
    }
}
