use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

use crate::product::Product;

#[entity(SqliteType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
}

impl Category {
    pub fn table(db: SqliteDB) -> &'static Table<SqliteDB, Category> {
        static CACHE: OnceLock<Table<SqliteDB, Category>> = OnceLock::new();
        CACHE.get_or_init(|| {
            Table::new("category", db)
                .with_id_column("id")
                .with_column_of::<String>("name")
                .with_many("products", "category_id", |db| Product::table(db).clone())
            // .with_expression("product_count", |t| {
            //     t.get_subquery_as::<Product>("products")
            //         .unwrap()
            //         .get_count_query()
            // })
            // .with_expression("title", |t| {
            //     let name = t.get_column_expr("name").unwrap();
            //     let count = t.get_column_expr("product_count").unwrap();
            //     concat_!(name, " (", count, ")").expr()
            // })
        })
    }
}

pub trait CategoryTable {
    fn id(&self) -> Column<i64>;
    fn ref_products(&self) -> Table<SqliteDB, Product>;
}

impl CategoryTable for Table<SqliteDB, Category> {
    fn id(&self) -> Column<i64> {
        self.get_column("id").unwrap()
    }
    fn ref_products(&self) -> Table<SqliteDB, Product> {
        self.get_ref_as("products").unwrap()
    }
}
