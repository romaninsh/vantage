use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use vantage_expressions::{
    Expression, Flatten, expr, expr_any, expression::flatten::ExpressionFlattener,
    protocol::expressive::ExpressiveEnum,
};

#[test]
fn test_basic_composition_example() {
    let where_expr = expr!("age > {} AND status = {}", 21, "active");
    let query_expr = expr!("SELECT * FROM users WHERE {}", (where_expr));

    // Verify the structure
    assert_eq!(query_expr.template, "SELECT * FROM users WHERE {}");
    assert_eq!(query_expr.parameters.len(), 1);
}

#[test]
fn test_surreal_duration_example() {
    // Test that the expr_any macro works with Value type
    let duration_secs = Duration::from_secs(3600).as_secs();
    let surreal_query = expr_any!(
        Value,
        "SELECT * FROM session WHERE created_at > time::now() - {}",
        duration_secs
    );

    assert_eq!(
        surreal_query.template,
        "SELECT * FROM session WHERE created_at > time::now() - {}"
    );
    assert_eq!(surreal_query.parameters.len(), 1);
}

#[test]
fn test_dynamic_query_building() {
    // Simulate user filter conditions
    struct UserFilter {
        min_age: Option<i32>,
        status: Option<String>,
        active_only: bool,
    }

    let user_filter = UserFilter {
        min_age: Some(25),
        status: Some("premium".to_string()),
        active_only: true,
    };

    let mut conditions = Vec::<Expression<Value>>::new();

    // Conditionally add filters
    if let Some(min_age) = user_filter.min_age {
        conditions.push(expr!("age >= {}", min_age));
    }
    if let Some(status) = user_filter.status {
        conditions.push(expr!("status = {}", status));
    }
    if user_filter.active_only {
        conditions.push(expr!("last_login > NOW() - INTERVAL 30 DAY"));
    }

    // Verify we have the expected conditions
    assert_eq!(conditions.len(), 3);

    // Use from_vec to combine conditions
    let where_clause = Expression::from_vec(conditions, " AND ");
    let final_query = expr!("SELECT * FROM users WHERE {}", (where_clause.clone()));

    // Flatten to see the final template and parameters
    let flattener = ExpressionFlattener::new();
    let flattened = flattener.flatten(&final_query);

    // Debug output to see what we actually get
    println!("WHERE clause template: {}", where_clause.template);
    println!("Final query template: {}", final_query.template);
    println!("Flattened template: {}", flattened.template);

    // The from_vec creates multiple placeholders, one for each condition
    assert_eq!(where_clause.template, "{} AND {} AND {}");
    assert_eq!(final_query.template, "SELECT * FROM users WHERE {}");
    // After flattening, the nested structure should be resolved
    assert_eq!(
        flattened.template,
        "SELECT * FROM users WHERE {} AND {} AND {}"
    );
    // The flattened version should have the parameters from nested expressions
    println!("Template: {}", flattened.template);
    println!("Parameters: {:?}", flattened.parameters);
}

#[test]
fn test_simple_dynamic_example() {
    // Simplified version that should work with current implementation
    let mut conditions = Vec::<String>::new();

    let min_age = Some(25);
    let status = Some("premium");
    let active_only = true;

    if min_age.is_some() {
        conditions.push("age >= 25".to_string());
    }
    if status.is_some() {
        conditions.push("status = 'premium'".to_string());
    }
    if active_only {
        conditions.push("last_login > NOW() - INTERVAL 30 DAY".to_string());
    }

    let where_clause = conditions.join(" AND ");
    let final_query = expr!("SELECT * FROM users WHERE {}", where_clause);

    assert_eq!(final_query.template, "SELECT * FROM users WHERE {}");
    assert_eq!(final_query.parameters.len(), 1);
}

#[test]
fn test_flattening_behavior() {
    // Test what flattening actually does
    let inner_expr = expr!("age > {} AND status = {}", 25, "active");
    let outer_expr = expr!("SELECT * FROM users WHERE {}", (inner_expr));

    let flattener = ExpressionFlattener::new();
    let flattened = flattener.flatten(&outer_expr);

    println!("Original template: {}", outer_expr.template);
    println!("Original params count: {}", outer_expr.parameters.len());
    println!("Flattened template: {}", flattened.template);
    println!("Flattened params count: {}", flattened.parameters.len());

    // This will help us understand what flattening produces
    assert_eq!(outer_expr.template, "SELECT * FROM users WHERE {}");
    assert_eq!(outer_expr.parameters.len(), 1);
}

#[tokio::test]
async fn test_querysource_example() {
    use vantage_expressions::mocks::StaticDataSource;
    use vantage_expressions::protocol::datasource::QuerySource;

    // Create a mock database that returns a fixed value
    let db = StaticDataSource::new(serde_json::json!(42));
    let query = expr!("SELECT COUNT(*) FROM users WHERE age > {}", 21);

    // Execute immediately - returns result now
    let count = db.execute(&query).await;
    assert_eq!(count, serde_json::json!(42));

    // Defer execution - returns DeferredFn
    let deferred_query = db.defer(query);
    let count = deferred_query.call().await; // Execute when needed
    match count {
        ExpressiveEnum::Scalar(val) => assert_eq!(val, serde_json::json!(42)),
        _ => panic!("Expected scalar result"),
    }
}

#[tokio::test]
async fn test_deferred_as_parameters() {
    use vantage_expressions::mocks::StaticDataSource;
    use vantage_expressions::protocol::datasource::QuerySource;
    use vantage_expressions::protocol::expressive::ExpressiveEnum;

    // Mock SurrealDB returning user IDs
    let surreal_db = StaticDataSource::new(serde_json::json!([1, 2, 3]));
    let user_ids_query = expr!("SELECT id FROM user WHERE status = {}", "active");

    // Create deferred query - defer() now returns DeferredFn directly
    let deferred_users = surreal_db.defer(user_ids_query);

    // Use deferred query with [deferred] syntax for DeferredFn
    let orders_query = expr!("SELECT * FROM orders WHERE user_id = ANY({})", {
        deferred_users
    });

    // Verify the structure
    assert_eq!(
        orders_query.template,
        "SELECT * FROM orders WHERE user_id = ANY({})"
    );
    assert_eq!(orders_query.parameters.len(), 1);

    // The parameter should be a deferred expression
    match &orders_query.parameters[0] {
        ExpressiveEnum::Deferred(_) => {} // Expected
        _ => panic!("Expected deferred parameter"),
    }
}

#[tokio::test]
async fn test_closure_syntax() {
    use vantage_expressions::protocol::expressive::ExpressiveEnum;

    // Test that {closure} syntax still works with .into()
    let closure =
        move || -> Pin<Box<dyn Future<Output = ExpressiveEnum<serde_json::Value>> + Send>> {
            Box::pin(async move { ExpressiveEnum::Scalar(serde_json::json!(42)) })
        };

    let query = expr!("SELECT * FROM test WHERE value = {}", { closure });

    // Verify the structure
    assert_eq!(query.template, "SELECT * FROM test WHERE value = {}");
    assert_eq!(query.parameters.len(), 1);

    // The parameter should be a deferred expression
    match &query.parameters[0] {
        ExpressiveEnum::Deferred(_) => {} // Expected
        _ => panic!("Expected deferred parameter"),
    }
}

#[tokio::test]
async fn test_mutex_deferred_function() {
    use std::sync::{Arc, Mutex};
    use vantage_expressions::protocol::expressive::{DeferredFn, ExpressiveEnum};

    // 1. Set mutex value
    let counter = Arc::new(Mutex::new(10i32));

    // 2. Create expression with deferred mutex value
    let deferred_count = DeferredFn::from_mutex(counter.clone());
    let query = expr!("SELECT * FROM items LIMIT {}", [deferred_count]);

    // 3. Change value after query construction
    *counter.lock().unwrap() = 25;

    // 4. Execute expression (simulate by calling the deferred function)
    if let ExpressiveEnum::Deferred(deferred_fn) = &query.parameters[0] {
        let result = deferred_fn.call().await;
        match result {
            ExpressiveEnum::Scalar(val) => {
                assert_eq!(val, serde_json::json!(25)); // Should use updated value, not original
            }
            _ => panic!("Expected scalar result"),
        }
    } else {
        panic!("Expected deferred parameter");
    }

    // Verify the query structure
    assert_eq!(query.template, "SELECT * FROM items LIMIT {}");
    assert_eq!(query.parameters.len(), 1);
}

#[tokio::test]
async fn test_union_extensibility() {
    use vantage_expressions::protocol::expressive::{Expressive, ExpressiveEnum};

    /// A UNION SQL construct that combines two SELECT expressions
    #[derive(Clone)]
    pub struct Union<T> {
        left: Expression<T>,
        right: Expression<T>,
    }

    impl<T> Union<T> {
        pub fn new(left: Expression<T>, right: Expression<T>) -> Self {
            Self { left, right }
        }
    }

    impl<T: Clone> Expressive<T> for Union<T> {
        fn expr(&self) -> Expression<T> {
            Expression::new(
                "{} UNION {}",
                vec![
                    ExpressiveEnum::Nested(self.left.clone()),
                    ExpressiveEnum::Nested(self.right.clone()),
                ],
            )
        }
    }

    // Usage example
    let users_query = expr!("SELECT name FROM users WHERE active = {}", true);
    let admins_query = expr!("SELECT name FROM admins WHERE role = {}", "super");

    let union = Union::new(users_query, admins_query);
    let final_query = expr!("SELECT DISTINCT name FROM ({})", (union.expr()));

    // Verify the structure
    assert_eq!(final_query.template, "SELECT DISTINCT name FROM ({})");
    assert_eq!(final_query.parameters.len(), 1);

    // The parameter should be a nested expression (the union)
    match &final_query.parameters[0] {
        ExpressiveEnum::Nested(union_expr) => {
            assert_eq!(union_expr.template, "{} UNION {}");
            assert_eq!(union_expr.parameters.len(), 2);

            // Both parameters should be nested expressions
            match (&union_expr.parameters[0], &union_expr.parameters[1]) {
                (ExpressiveEnum::Nested(left), ExpressiveEnum::Nested(right)) => {
                    assert_eq!(left.template, "SELECT name FROM users WHERE active = {}");
                    assert_eq!(right.template, "SELECT name FROM admins WHERE role = {}");
                }
                _ => panic!("Expected nested expressions for both union parts"),
            }
        }
        _ => panic!("Expected nested expression for union"),
    }

    // Test preview functionality to see the rendered query
    let preview = final_query.preview();
    let expected = "SELECT DISTINCT name FROM (SELECT name FROM users WHERE active = true UNION SELECT name FROM admins WHERE role = \"super\")";
    assert_eq!(preview, expected);
}
