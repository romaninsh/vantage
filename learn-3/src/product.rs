use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

use crate::category::Category;

#[entity(SqliteType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Product {
    pub name: String,
    pub price: i64,
}

impl Product {
    pub fn table(db: SqliteDB) -> &'static Table<SqliteDB, Product> {
        static CACHE: OnceLock<Table<SqliteDB, Product>> = OnceLock::new();
        CACHE.get_or_init(|| {
            let is_deleted = Column::<bool>::new("is_deleted");
            Table::new("product", db)
                .with_id_column("id")
                .with_column_of::<String>("name")
                .with_column_of::<i64>("price")
                .with_column_of::<bool>("is_deleted")
                .with_condition(is_deleted.eq(false))
                .with_one("category", "category_id", |db| Category::table(db).clone())
            // .with_expression("category", |t| {
            //     t.get_subquery_as::<Category>("category")
            //         .unwrap()
            //         .select_column("name")
            // })
        })
    }
}
