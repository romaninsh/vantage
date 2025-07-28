use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_surrealdb::{
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
    select.add_order_by("name", true);

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
    select.add_order_by("name", true);

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
    select.add_order_by("name", true);

    let result = select.preview();
    assert_eq!(
        result,
        "SELECT * FROM (bakery:hill_valley<-belongs_to<-client) ORDER BY name"
    );

    let mut select = SurrealSelect::new();

    // Second query: SELECT * FROM client WHERE bakery = bakery:hill_valley order by name
    select.set_source("client", None);
    select.add_where_condition(expr!("bakery = {}", Thing::new("bakery", "hill_valley")));
    select.add_order_by("name", true);

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
                .as_list(Field::new("inventory").dot("stock"))
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
        .with_order("product_name", true)
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
                                    .contains(Parent::new().dot("id")),
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
