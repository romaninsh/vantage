use serde_json::json;
use std::sync::{Arc, Mutex};
use vantage_expressions::mocks::FlatteningPatternDataSource;
use vantage_expressions::{DataSource, Expression, IntoExpressive, expr};

#[test]
fn test_arc_mutex_with_database_execution() {
    tokio_test::block_on(async {
        // Create database mock with patterns for different values
        let db = FlatteningPatternDataSource::new()
            .with_pattern("hello 10", json!("greeting_10"))
            .with_pattern("select spelling from numbers where num=10", json!("ten"))
            .with_pattern(
                "select spelling from numbers where num=15",
                json!("fifteen"),
            );

        // Create shared mutable variable
        let shared_var = Arc::new(Mutex::new(10i32));

        // Create expressions using OwnedExpression
        let expr1 = expr!("hello {}", &shared_var);
        let expr2 = expr!("select spelling from numbers where num={}", &shared_var);

        // Execute first query
        let result1 = db.execute(&expr1).await;
        assert_eq!(result1, json!("greeting_10"));

        let result2_before = db.execute(&expr2).await;
        assert_eq!(result2_before, json!("ten"));

        // Modify the shared value
        {
            let mut guard = shared_var.lock().unwrap();
            *guard = 15;
        }

        // Execute same expression again - deferred evaluation will see new value
        let result2_after = db.execute(&expr2).await;
        assert_eq!(result2_after, json!("fifteen"));
    });
}

#[test]
fn test_arc_mutex_with_nested_expression() {
    #[derive(Debug, Clone)]
    struct GreetingQuery {
        name: String,
    }

    impl From<&GreetingQuery> for Expression {
        fn from(greeting: &GreetingQuery) -> Expression {
            expr!("Hello {}", greeting.name.clone())
        }
    }

    impl From<GreetingQuery> for IntoExpressive<Expression> {
        fn from(greeting: GreetingQuery) -> Self {
            IntoExpressive::nested(Expression::from(&greeting))
        }
    }

    tokio_test::block_on(async {
        // Create database mock
        let db = FlatteningPatternDataSource::new()
            .with_pattern("select Hello \"world\"", json!("greeting_world"))
            .with_pattern("select Hello \"vantage\"", json!("greeting_vantage"));

        // Create mutable greeting struct
        let greeting = Arc::new(Mutex::new(GreetingQuery {
            name: "world".to_string(),
        }));

        let expr = expr!("select {}", &greeting);

        // Execute first query
        let result1 = db.execute(&expr).await;
        assert_eq!(result1, json!("greeting_world"));

        // Modify the greeting name
        {
            let mut guard = greeting.lock().unwrap();
            guard.name = "vantage".to_string();
        }

        // Execute same expression again - should see new nested expression result
        let result2 = db.execute(&expr).await;
        assert_eq!(result2, json!("greeting_vantage"));
    });
}

#[test]
fn test_triple_nested_expression() {
    #[derive(Debug, Clone)]
    struct Department {
        name: String,
    }

    #[derive(Debug, Clone)]
    struct FilterQuery {
        department: Arc<Mutex<Department>>,
    }

    #[derive(Debug, Clone)]
    struct MainQuery {
        filter: Arc<Mutex<FilterQuery>>,
    }

    impl From<&Department> for Expression {
        fn from(dept: &Department) -> Expression {
            expr!("UPPER({})", dept.name.clone())
        }
    }

    impl From<&FilterQuery> for Expression {
        fn from(filter: &FilterQuery) -> Expression {
            expr!("COUNT(*) WHERE department = {}", &filter.department)
        }
    }

    impl From<&MainQuery> for Expression {
        fn from(main: &MainQuery) -> Expression {
            expr!("SELECT {} FROM users", &main.filter)
        }
    }

    impl From<Department> for IntoExpressive<Expression> {
        fn from(dept: Department) -> Self {
            IntoExpressive::nested(Expression::from(&dept))
        }
    }

    impl From<FilterQuery> for IntoExpressive<Expression> {
        fn from(filter: FilterQuery) -> Self {
            IntoExpressive::nested(Expression::from(&filter))
        }
    }

    impl From<MainQuery> for IntoExpressive<Expression> {
        fn from(main: MainQuery) -> Self {
            IntoExpressive::nested(Expression::from(&main))
        }
    }

    tokio_test::block_on(async {
        // Create database mock with triple-nested query patterns
        let db = FlatteningPatternDataSource::new()
            .with_pattern(
                "query result: SELECT COUNT(*) WHERE department = UPPER(\"engineering\") FROM users",
                json!("result_engineering"),
            )
            .with_pattern(
                "query result: SELECT COUNT(*) WHERE department = UPPER(\"marketing\") FROM users",
                json!("result_marketing"),
            )
            .with_pattern(
                "query result: SELECT COUNT(*) WHERE department = UPPER(\"sales\") FROM users",
                json!("result_sales"),
            );

        // Create triple-nested mutable structure
        let department = Arc::new(Mutex::new(Department {
            name: "engineering".to_string(),
        }));

        let filter = Arc::new(Mutex::new(FilterQuery {
            department: department.clone(),
        }));

        let main_query = Arc::new(Mutex::new(MainQuery {
            filter: filter.clone(),
        }));

        // Level 1: Main expression containing nested expressions
        // Level 2: Filter expression containing department expression
        // Level 3: Department expression with UPPER function
        let expr = expr!("query result: {}", &main_query);

        // Execute first query - engineering
        let result1 = db.execute(&expr).await;
        assert_eq!(result1, json!("result_engineering"));

        // Modify the deepest nested value (Level 3)
        {
            let mut dept_guard = department.lock().unwrap();
            dept_guard.name = "marketing".to_string();
        }

        // Execute same expression again - should see new deeply nested result
        let result2 = db.execute(&expr).await;
        assert_eq!(result2, json!("result_marketing"));

        // Change again to test triple nesting fully
        {
            let mut dept_guard = department.lock().unwrap();
            dept_guard.name = "sales".to_string();
        }

        let result3 = db.execute(&expr).await;
        assert_eq!(result3, json!("result_sales"));
    });
}
