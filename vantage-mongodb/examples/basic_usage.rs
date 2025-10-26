use serde_json::Value;
use vantage_expressions::protocol::selectable::Selectable;
use vantage_mongodb::{Document, count, delete, insert, select, update};

fn main() {
    // Basic find query
    let query = select("users");
    println!("Basic find:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Find with filter
    let mut query = select("users");
    query.add_where_condition(Document::filter("status", "active").into());
    println!("Find with filter:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Find with multiple conditions
    let mut query = select("users");
    query.add_where_condition(Document::filter("age", 25).into());
    query.add_where_condition(Document::filter("city", "New York").into());
    println!("Find with multiple conditions:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Find with operators
    let mut query = select("products");
    query.add_where_condition(Document::gt("price", 100).into());
    println!("Find with $gt operator:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Find with $or operator
    let mut query = select("users");
    query.add_where_condition(
        Document::or(vec![
            Document::filter("status", "active"),
            Document::filter("priority", "high"),
        ])
        .into(),
    );
    println!("Find with $or operator:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Find with projection
    let mut query = select("users");
    query.add_field("name".to_string());
    query.add_field("email".to_string());
    println!("Find with projection:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Find with sort, skip, and limit
    let mut query = select("users");
    query.add_order_by(vantage_expressions::expr!("created_at"), true);
    query.set_limit(Some(10), Some(20));
    println!("Find with sort, skip, and limit:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Complex find query
    let mut query = select("orders");
    query.add_where_condition(
        Document::new()
            .insert("status", "pending")
            .and("total", Document::new().insert("$gte", 50))
            .and(
                "created_at",
                Document::new()
                    .insert("$gte", "2024-01-01")
                    .insert("$lt", "2024-12-31"),
            )
            .into(),
    );
    query.add_field("order_id".to_string());
    query.add_field("customer".to_string());
    query.add_field("total".to_string());
    query.add_order_by(vantage_expressions::expr!("created_at"), true);
    query.set_limit(Some(100), None);
    println!("Complex find query:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Insert one document
    let query = insert("users").insert_one(
        Document::new()
            .insert("name", "John Doe")
            .insert("email", "john@example.com")
            .insert("age", 30),
    );
    println!("Insert one:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Insert multiple documents
    let query = insert("users").insert_many(vec![
        Document::new()
            .insert("name", "Alice")
            .insert("email", "alice@example.com"),
        Document::new()
            .insert("name", "Bob")
            .insert("email", "bob@example.com"),
    ]);
    println!("Insert many:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Update query
    let query = update("users")
        .filter(Document::filter("status", "pending"))
        .set_update(
            Document::new().insert(
                "$set",
                Document::new()
                    .insert("status", "active")
                    .insert("updated_at", "2024-01-01"),
            ),
        );
    println!("Update query:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Delete query
    let query = delete("users").filter(Document::filter("status", "inactive"));
    println!("Delete query:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Count query
    let query = count("users").filter(Document::gt("age", 18));
    println!("Count query:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Advanced operators
    let mut query = select("products");
    query.add_where_condition(
        Document::new()
            .insert(
                "category",
                Document::new().insert(
                    "$in",
                    vec![
                        Value::String("electronics".to_string()),
                        Value::String("books".to_string()),
                    ],
                ),
            )
            .and("name", Document::new().insert("$regex", "^laptop"))
            .and("description", Document::new().insert("$exists", true))
            .into(),
    );
    println!("Advanced operators ($in, $regex, $exists):");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Text search simulation
    let mut query = select("articles");
    query.add_where_condition(
        Document::new()
            .insert(
                "$text",
                Document::new().insert("$search", "mongodb database"),
            )
            .into(),
    );
    println!("Text search:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());

    // Geospatial query simulation
    let mut query = select("locations");
    query.add_where_condition(
        Document::new()
            .insert(
                "location",
                Document::new().insert(
                    "$near",
                    Document::new()
                        .insert(
                            "$geometry",
                            Document::new()
                                .insert("type", "Point")
                                .insert("coordinates", vec![-73.9857, 40.7484]),
                        )
                        .insert("$maxDistance", 1000),
                ),
            )
            .into(),
    );
    println!("Geospatial query:");
    let expr: vantage_expressions::Expression = query.into();
    println!("{}\n", expr.preview());
}
