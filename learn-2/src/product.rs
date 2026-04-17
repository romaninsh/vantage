use vantage_sql::prelude::*;
use vantage_types::prelude::*;

use crate::category::Category;

#[entity(SqliteType)]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub category: Option<String>,
}

impl Product {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
        let is_deleted = Column::<bool>::new("is_deleted");
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_condition(is_deleted.eq(false))
            .with_one("category", "category_id", Category::table)
            .with_expression("category", |t| {
                t.get_subquery_as::<Category>("category")
                    .unwrap()
                    .select_column("title")
            })
    }
}

pub trait ProductTable {
    async fn print(&self) -> VantageResult<()>;
}

impl ProductTable for Table<SqliteDB, Product> {
    async fn print(&self) -> VantageResult<()> {
        vantage_cli_util::print_table(self).await
    }
}
