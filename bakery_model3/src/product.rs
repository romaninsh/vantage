use serde::{Deserialize, Serialize};

use surreal_client::types::Any;
use vantage_surrealdb::{SurrealColumn, SurrealDB};
use vantage_table::{Table, TableLike};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Inventory {
    pub stock: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Product {
    pub name: String,
    pub calories: i64,
    pub price: i64,
    pub bakery: String, // Record ID for bakery
    pub is_deleted: bool,
    pub inventory: Inventory,
}

impl Product {
    pub fn table(db: SurrealDB) -> Table<SurrealDB, Product> {
        Table::new("product", db)
            .with_column("name")
            .with_column("calories")
            .with_column("price")
            .with_column("bakery")
            .with_column("is_deleted")
            .with_column("inventory")
            .into_entity()
    }
}

pub trait ProductTable: TableLike {
    fn name(&self) -> SurrealColumn<String> {
        self.get_column("name")
            .unwrap()
            .as_any()
            .downcast_ref::<SurrealColumn<Any>>()
            .unwrap()
            .clone()
            .into_type()
    }

    fn calories(&self) -> SurrealColumn<i64> {
        self.get_column("calories")
            .unwrap()
            .as_any()
            .downcast_ref::<SurrealColumn<Any>>()
            .unwrap()
            .clone()
            .into_type()
    }

    fn price(&self) -> SurrealColumn<i64> {
        self.get_column("price")
            .unwrap()
            .as_any()
            .downcast_ref::<SurrealColumn<Any>>()
            .unwrap()
            .clone()
            .into_type()
    }

    fn is_deleted(&self) -> SurrealColumn<bool> {
        self.get_column("is_deleted")
            .unwrap()
            .as_any()
            .downcast_ref::<SurrealColumn<Any>>()
            .unwrap()
            .clone()
            .into_type()
    }

    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_bakery(&self) -> Table<SurrealDB, Bakery>;
}

impl ProductTable for Table<SurrealDB, Product> {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vantage_dataset::prelude::ReadableDataSet;
    use vantage_surrealdb::mocks::SurrealMockBuilder;
    use vantage_surrealdb::prelude::*;

    #[tokio::test]
    async fn test_product_field_accessors_with_condition() {
        // Mock the query response
        let db = SurrealMockBuilder::new()
            .with_query_response(
                "SELECT name, calories, price, bakery, is_deleted, inventory FROM ONLY product WHERE price = 100",
                json!([{
                    "name": "Expensive Cake",
                    "calories": 500,
                    "price": 100,
                    "bakery": "bakery:1",
                    "is_deleted": false,
                    "inventory": {"stock": 5}
                }]),
            )
            .build();

        let products = Product::table(db);

        // Use field accessor in condition
        let price_condition = products.price().eq(100);
        let expensive_products = products.with_condition(price_condition);

        // Get results from mocked query
        let result = expensive_products.get_some().await.unwrap();
        let results = result.unwrap();

        assert_eq!(results.name, "Expensive Cake");
        assert_eq!(results.price, 100);
    }

    #[test]
    fn test_product_field_accessor_operations() {
        // Test that field accessors work with operations
        let db = SurrealMockBuilder::new().build();

        let products = Product::table(db);

        let price_col = products.price();
        let name_col = products.name();

        // Can use them directly in conditions (RefOperation trait)
        let expensive = price_col.eq(100);
        assert_eq!(expensive.preview(), "price = 100");

        let named = name_col.eq("Croissant");
        assert_eq!(named.preview(), "name = \"Croissant\"");
    }
}
