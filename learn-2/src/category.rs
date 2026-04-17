use vantage_sql::concat_;
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

use crate::product::Product;

#[entity(SqliteType)]
#[derive(Debug, Clone, Default)]
pub struct Category {
    pub name: String,
    pub title: Option<String>,
}

impl Category {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Category> {
        Table::new("category", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_many("products", "category_id", Product::table)
            .with_expression("product_count", |t| {
                t.get_subquery_as::<Product>("products")
                    .unwrap()
                    .get_count_query()
            })
            .with_expression("title", |t| {
                let name = t.get_column_expr("name").unwrap();
                let count = t.get_column_expr("product_count").unwrap();
                concat_!(name, " (", count, ")").expr()
            })
    }
}
