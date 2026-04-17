use std::sync::OnceLock;

use vantage_mongodb::prelude::*;

use crate::category::Category;

#[entity(MongoType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Product {
    pub name: String,
    pub price: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub is_deleted: bool,
}

impl Product {
    pub fn table(db: MongoDB) -> &'static Table<MongoDB, Product> {
        static CACHE: OnceLock<Table<MongoDB, Product>> = OnceLock::new();
        CACHE.get_or_init(|| {
            let is_deleted = Column::<bool>::new("is_deleted");
            Table::new("product", db)
                .with_id_column("_id")
                .with_column_of::<String>("name")
                .with_column_of::<i64>("price")
                .with_column_of::<String>("category_id")
                .with_column_of::<bool>("is_deleted")
                .with_condition(is_deleted.eq(false))
                .with_one("category", "category_id", |db| Category::table(db).clone())
        })
    }
}
