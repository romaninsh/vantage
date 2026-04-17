use std::sync::OnceLock;

use vantage_mongodb::prelude::*;

use crate::product::Product;

#[entity(MongoType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
}

impl Category {
    pub fn table(db: MongoDB) -> &'static Table<MongoDB, Category> {
        static CACHE: OnceLock<Table<MongoDB, Category>> = OnceLock::new();
        CACHE.get_or_init(|| {
            Table::new("category", db)
                .with_id_column("_id")
                .with_column_of::<String>("name")
                .with_many("products", "category_id", |db| Product::table(db).clone())
        })
    }
}

pub trait CategoryTable {
    fn id(&self) -> Column<String>;
    fn ref_products(&self) -> Table<MongoDB, Product>;
}

impl CategoryTable for Table<MongoDB, Category> {
    fn id(&self) -> Column<String> {
        self.get_column("_id").unwrap()
    }
    fn ref_products(&self) -> Table<MongoDB, Product> {
        self.get_ref_as("products").unwrap()
    }
}
