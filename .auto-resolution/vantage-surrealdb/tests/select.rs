use serde_json::Value;
use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_surrealdb::{
    SurrealDB,
    field_projection::FieldProjection,
    identifier::{Identifier, Parent},
    operation::{Expressive, RefOperation},
    select::{SurrealSelect, field::Field},
    sum::{Fx, Sum},
    surreal_return::SurrealReturn,
    thing::Thing,
};

fn snip(str: &str) -> String {
    str.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("( ", "(")
        .replace(" )", ")")
        .replace("{ ", "{")
        .replace(" }", "}")
}

#[test]
fn query01() {
    let mut select = SurrealSelect::new();

    // Set the source table
    select.set_source("product", None);
    select.add_where_condition(expr!("bakery = {}", Thing::new("bakery", "hill_valley")));
    select.add_where_condition(expr!("is_deleted = {}", false));
    select.add_order_by(expr!("name"), true);

    let result = select.preview();

    assert_eq!(
        result,
        "SELECT * FROM product WHERE bakery = bakery:hill_valley AND is_deleted = false ORDER BY name"
    );

    let mut select = SurrealSelect::new();

    select.set_source(
        Thing::new("bakery", "hill_valley").rref("owns", "product"),
        None,
    );
    select.add_where_condition(expr!("is_deleted = {}", false));
    select.add_order_by(expr!("name"), true);

    let result2 = select.preview();

    assert_eq!(
        result2,
        "SELECT * FROM (bakery:hill_valley->owns->product) WHERE is_deleted = false ORDER BY name"
    );
}

#[test]
fn query02() {
    let mut select = SurrealSelect::new();

    // First query: SELECT * FROM bakery:hill_valley<-belongs_to<-client order by name
    select.set_source(
        Thing::new("bakery", "hill_valley").lref("belongs_to", "client"),
        None,
    );
    select.add_order_by(expr!("name"), true);

    let result = select.preview();
    assert_eq!(
        result,
        "SELECT * FROM (bakery:hill_valley<-belongs_to<-client) ORDER BY name"
    );

    let mut select = SurrealSelect::new();

    // Second query: SELECT * FROM client WHERE bakery = bakery:hill_valley order by name
    select.set_source("client", None);
    select.add_where_condition(expr!("bakery = {}", Thing::new("bakery", "hill_valley")));
    select.add_order_by(expr!("name"), true);

    let result = select.preview();
    assert_eq!(
        result,
        "SELECT * FROM client WHERE bakery = bakery:hill_valley ORDER BY name"
    );
}

#[test]
fn query03() {
    let mut select = SurrealSelect::new();

    select.add_field("name");
    select.add_field("price");
    select.add_expression(
        Identifier::new("inventory").dot("stock"),
        Some("stock".to_string()),
    );

    select.set_source("product", None);
    select.add_where_condition(Field::new("is_deleted").eq(false));

    let result = select.preview();
    assert_eq!(
        result,
        "SELECT name, price, inventory.stock AS stock FROM product WHERE is_deleted = false"
    );
}

#[test]
fn query04() {
    let select = SurrealReturn::new(
        Sum::new(
            SurrealSelect::new()
                .with_source("product")
                .with_condition(Field::new("is_deleted").eq(false))
                .only_expression(Field::new("inventory").dot("stock"))
                .into(),
        )
        .expr()
        .sub(
            Fx::new(
                "count",
                vec![SurrealSelect::new().with_source("product").expr()],
            )
            .expr(),
        ),
    );
    assert_eq!(
        select.preview(),
        "RETURN math::sum(SELECT VALUE inventory.stock FROM product WHERE is_deleted = false) - count(SELECT * FROM product)"
    );
}

#[test]
fn sniptest() {
    assert_eq!(
        "RETURN math::sum(SELECT VALUE inventory.stock FROM product WHERE is_deleted = false) - count(SELECT * FROM product)",
        snip(
            "RETURN math::sum(
                SELECT VALUE inventory.stock
                FROM product
                WHERE is_deleted = false
            ) - count(SELECT * FROM product)"
        )
    )
}

#[test]
fn query07() {
    let projection = FieldProjection::new(expr!("lines[*]"))
        .with_expression(expr!("product.name"), "product_name")
        .with_field("quantity")
        .with_field("price")
        .with_expression(expr!("quantity * price"), "subtotal");

    let select = SurrealSelect::new()
        .with_source("order")
        .with_field("id")
        .with_field("created_at")
        .with_expression(projection.into(), Some("items".to_string()));
    assert_eq!(
        select.preview(),
        snip(
            "SELECT id, created_at, lines[*].{
                product_name: product.name,
                quantity: quantity,
                price: price,
                subtotal: quantity * price
            } AS items FROM order"
        )
    )
}

#[test]
fn query11() {
    let select = SurrealSelect::new()
        .with_order(expr!("product_name"), true)
        .with_condition(expr!("total_items_ordered > current_inventory"))
        .with_source(
            SurrealSelect::new()
                .with_expression(
                    Identifier::new("name").into(),
                    Some("product_name".to_string()),
                )
                .with_expression(
                    Identifier::new("inventory").dot("stock"),
                    Some("current_inventory".to_string()),
                )
                .with_expression(
                    Sum::new(
                        SurrealSelect::new()
                            .with_value()
                            .with_expression(
                                Sum::new(expr!("lines[WHERE product = $parent.id].quantity"))
                                    .into(),
                                None,
                            )
                            .with_source("order")
                            .with_condition(
                                Identifier::new("lines")
                                    .dot("product")
                                    .contains(Parent::identifier().dot("id")),
                            )
                            .expr(),
                    )
                    .into(),
                    Some("total_items_ordered".to_string()),
                )
                .with_source("product")
                .with_condition(
                    Thing::new("bakery", "hill_valley").in_(expr!("").lref("owns", "bakery")),
                )
                .with_condition(Identifier::new("is_deleted").eq(false))
                .expr(),
        );

    assert_eq!(
        select.preview(),
        snip(
            "SELECT * FROM (
    SELECT
        name AS product_name,
        inventory.stock AS current_inventory,
        math::sum(
            SELECT VALUE math::sum(
                lines[WHERE product = $parent.id].quantity
            )
            FROM order
            WHERE lines.product CONTAINS $parent.id
        ) AS total_items_ordered
    FROM product
    WHERE bakery:hill_valley IN <-owns<-bakery
        AND is_deleted = false
) WHERE total_items_ordered > current_inventory
ORDER BY product_name"
        )
    )
}

#[test]
fn test_set_source_accepts_string_and_expression() {
    // Test with string literal directly
    let mut select1 = SurrealSelect::new();
    select1.set_source("users", None);
    let result1 = select1.preview();
    assert_eq!(result1, "SELECT * FROM users");

    // Test with String type
    let table_name = String::from("products");
    let mut select2 = SurrealSelect::new();
    select2.set_source(table_name, None);
    let result2 = select2.preview();
    assert_eq!(result2, "SELECT * FROM products");

    // Test with expression
    let table_expr = "product_table";
    let mut select3 = SurrealSelect::new();
    select3.set_source(table_expr, None);
    let result3 = select3.preview();
    assert_eq!(result3, "SELECT * FROM product_table");
}

async fn setup_test_db_with_data(mock_data: Value) -> SurrealDB {
    use surreal_client::{Engine, SurrealClient};

    struct MockEngine {
        data: Value,
    }

    impl MockEngine {
        fn new(data: Value) -> Self {
            Self { data }
        }
    }

    #[async_trait::async_trait]
    impl Engine for MockEngine {
        async fn send_message(
            &mut self,
            _method: &str,
            _params: Value,
        ) -> surreal_client::error::Result<Value> {
            Ok(self.data.clone())
        }
    }

    let client = SurrealClient::new(
        Box::new(MockEngine::new(mock_data)),
        Some("test".to_string()),
        Some("v1".to_string()),
    );

    SurrealDB::new(client)
}

#[tokio::test]
async fn test_get_rows() {
    let mock_data = serde_json::json!([
        {"name": "John Doe", "email": "john@example.com"},
        {"name": "Jane Smith", "email": "jane@example.com"}
    ]);
    let db = setup_test_db_with_data(mock_data).await;

    // Test SurrealSelect<result::Rows> -> Vec<Map<String, Value>>
    let select = SurrealSelect::new()
        .with_source("users")
        .with_field("name")
        .with_field("email");

    // Test the query structure
    assert_eq!(select.preview(), "SELECT name, email FROM users");

    // Test actual execution (with mock data)
    let rows = select.get(&db).await;
    assert_eq!(rows.len(), 2);
    assert!(rows[0].contains_key("name"));
    assert!(rows[0].contains_key("email"));
}

#[tokio::test]
async fn test_get_list() {
    let mock_data = serde_json::json!(["Product A", "Product B"]);
    let db = setup_test_db_with_data(mock_data).await;

    // Test SurrealSelect<result::List> -> Vec<Value>
    let select = SurrealSelect::new()
        .with_source("products")
        .with_condition(Field::new("active").eq(true))
        .only_column("name");

    assert_eq!(
        select.preview(),
        "SELECT VALUE name FROM products WHERE active = true"
    );

    // Test actual execution
    let values = select.get(&db).await;
    assert_eq!(values.len(), 2);
    assert!(values[0].is_string());
    assert!(values[1].is_string());
}

#[tokio::test]
async fn test_single_row() {
    let mock_data = serde_json::json!([
        {"theme": "dark", "language": "en"}
    ]);
    let db = setup_test_db_with_data(mock_data).await;

    // Test SurrealSelect<result::SingleRow>
    let select = SurrealSelect::new()
        .with_source("settings")
        .with_field("theme")
        .with_field("language")
        .only_first_row();

    assert_eq!(
        select.preview(),
        "SELECT theme, language FROM ONLY settings"
    );

    // Test actual execution
    let row = select.get(&db).await;
    assert!(!row.is_empty());
    assert!(row.get("theme").unwrap().is_string());
    assert!(row.get("language").unwrap().is_string());
    assert_eq!(row.get("theme").unwrap().as_str().unwrap(), "dark");
    assert_eq!(row.get("language").unwrap().as_str().unwrap(), "en");
}

#[test]
fn test_type_conversions() {
    // Test Rows -> List conversion
    let rows_query = SurrealSelect::new()
        .with_source("products")
        .with_condition(Field::new("category").eq("electronics"));

    let list_query = rows_query.only_column("price");
    assert_eq!(
        list_query.preview(),
        "SELECT VALUE price FROM products WHERE category = \"electronics\""
    );

    // Test Rows -> SingleRow conversion
    let rows_query2 = SurrealSelect::new()
        .with_source("users")
        .with_condition(Field::new("email").eq("test@example.com"));

    let single_row_query = rows_query2.only_first_row();
    assert_eq!(
        single_row_query.preview(),
        "SELECT * FROM ONLY users WHERE email = \"test@example.com\""
    );

    // Test SingleRow -> Single conversion
    let single_query = single_row_query.only_column("id");
    assert_eq!(
        single_query.preview(),
        "SELECT VALUE id FROM ONLY users WHERE email = \"test@example.com\""
    );
}

#[test]
fn test_aggregation_methods() {
    // Test as_sum
    let sum_query = SurrealSelect::new()
        .with_source("orders")
        .with_condition(Field::new("status").eq("completed"))
        .as_sum("total");

    assert_eq!(
        sum_query.preview(),
        "RETURN math::sum(SELECT VALUE total FROM orders WHERE status = \"completed\")"
    );

    // Test as_count
    let count_query = SurrealSelect::new()
        .with_source("products")
        .with_condition(Field::new("active").eq(true))
        .as_count();

    assert_eq!(
        count_query.preview(),
        "RETURN count(SELECT VALUE id FROM products WHERE active = true)"
    );
}

#[test]
fn test_value_select() {
    // Test SELECT VALUE with expression
    let select = SurrealSelect::new()
        .with_source("inventory")
        .with_condition(Field::new("product_id").eq("prod123"))
        .only_expression(expr!("stock * price"));

    assert_eq!(
        select.preview(),
        "SELECT VALUE stock * price FROM inventory WHERE product_id = \"prod123\""
    );
}

#[tokio::test]
async fn test_single_value() {
    let mock_data = serde_json::json!("John Doe");
    let db = setup_test_db_with_data(mock_data).await;

    // Approach 1: only_first_row() then only_column()
    let name1 = SurrealSelect::new()
        .with_source("users")
        .with_condition(Field::new("id").eq("user123"))
        .only_first_row()
        .only_column("name")
        .get(&db)
        .await;

    // Approach 2: only_column() then only_first_row()
    let name2 = SurrealSelect::new()
        .with_source("users")
        .with_condition(Field::new("id").eq("user123"))
        .only_column("name")
        .only_first_row()
        .get(&db)
        .await;

    // Both should return the same result
    assert!(name1.is_string());
    assert!(name2.is_string());
    assert_eq!(name1.as_str().unwrap(), "John Doe");
    assert_eq!(name2.as_str().unwrap(), "John Doe");
}
