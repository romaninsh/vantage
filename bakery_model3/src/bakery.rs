use vantage_aws::dynamodb::{AnyDynamoType, DynamoDB};
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

#[entity(CsvType, SurrealType, SqliteType, PostgresType, MongoType, DynamoType)]
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
        Table::new("bakery", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
            .with_many("products", "bakery", crate::Product::surreal_table)
    }

    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Bakery> {
        Table::new("bakery", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
            .with_many("clients", "bakery_id", crate::Client::sqlite_table)
            .with_many("products", "bakery_id", crate::Product::sqlite_table)
    }

    pub fn postgres_table(db: PostgresDB) -> Table<PostgresDB, Bakery> {
        Table::new("bakery", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
            .with_many("clients", "bakery_id", crate::Client::postgres_table)
            .with_many("products", "bakery_id", crate::Product::postgres_table)
    }

    pub fn mongo_table(db: MongoDB) -> Table<MongoDB, Bakery> {
        Table::new("bakery", db)
            .with_id_column("_id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
            .with_many("clients", "bakery_id", crate::Client::mongo_table)
            .with_many("products", "bakery_id", crate::Product::mongo_table)
    }

    pub fn dynamo_table(db: DynamoDB) -> Table<DynamoDB, Bakery> {
        Table::new("vantage-demo-bakery", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("profit_margin")
            .with_many("products", "bakery_id", crate::Product::dynamo_table)
    }
}
