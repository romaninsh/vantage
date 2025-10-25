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
    pub price: Option<i64>,
    pub bakery: String, // Record ID for bakery
    pub is_deleted: bool,
    pub inventory: Inventory,
}

impl Product {
    pub fn table(db: SurrealDB) -> Table<SurrealDB, Product> {
        use vantage_surrealdb::prelude::*;
        Table::new("product", db)
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<String>("bakery")
            .with_column_of::<bool>("is_deleted")
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

    #[test]
    fn test_typed_columns_with_get_type() {
        use vantage_surrealdb::prelude::*;

        let db = SurrealMockBuilder::new().build();
        let mut products = Table::new("product", db).into_entity::<Product>();

        // Add typed columns using with_column_of
        products.add_column_of::<i64>("price");
        products.add_column_of::<String>("name");
        products.add_column_of::<bool>("is_deleted");

        // Verify get_type returns correct type names
        let price_col = products.get_column("price").unwrap();
        assert_eq!(price_col.get_type(), "int");

        let name_col = products.get_column("name").unwrap();
        assert_eq!(name_col.get_type(), "string");

        let is_deleted_col = products.get_column("is_deleted").unwrap();
        assert_eq!(is_deleted_col.get_type(), "bool");
    }

    #[test]
    fn test_with_column_of_builder_pattern() {
        use vantage_surrealdb::prelude::*;

        let db = SurrealMockBuilder::new().build();
        let products = Table::new("product", db)
            .with_column_of::<i64>("price")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .into_entity::<Product>();

        // Verify all columns have correct types
        assert_eq!(products.get_column("price").unwrap().get_type(), "int");
        assert_eq!(products.get_column("name").unwrap().get_type(), "string");
        assert_eq!(products.get_column("calories").unwrap().get_type(), "int");
    }

    #[test]
    fn test_type_info_access() {
        use surreal_client::types::SurrealTypeEnum;
        use vantage_surrealdb::prelude::*;

        let db = SurrealMockBuilder::new().build();
        let products = Table::new("product", db)
            .with_column_of::<i64>("price")
            .with_column_of::<String>("name")
            .with_column_of::<bool>("is_deleted")
            .into_entity::<Product>();

        // Get column and downcast to access type_info
        let price_col = products.get_column("price").unwrap();
        let surreal_col = price_col
            .as_any()
            .downcast_ref::<vantage_surrealdb::SurrealColumn>()
            .unwrap();

        // Access TypeInfo to get more than just the name
        let type_info = surreal_col.get_type_info();
        assert_eq!(type_info.type_name(), "int");
        assert_eq!(type_info.type_enum(), SurrealTypeEnum::Int);

        // Same for string column
        let name_col = products.get_column("name").unwrap();
        let surreal_col = name_col
            .as_any()
            .downcast_ref::<vantage_surrealdb::SurrealColumn>()
            .unwrap();
        let type_info = surreal_col.get_type_info();
        assert_eq!(type_info.type_name(), "string");
        assert_eq!(type_info.type_enum(), SurrealTypeEnum::String);

        // And bool column
        let deleted_col = products.get_column("is_deleted").unwrap();
        let surreal_col = deleted_col
            .as_any()
            .downcast_ref::<vantage_surrealdb::SurrealColumn>()
            .unwrap();
        let type_info = surreal_col.get_type_info();
        assert_eq!(type_info.type_name(), "bool");
        assert_eq!(type_info.type_enum(), SurrealTypeEnum::Bool);
    }
}
