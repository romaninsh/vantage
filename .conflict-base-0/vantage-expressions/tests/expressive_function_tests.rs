//! # Expressive Function Pattern Tests
//!
//! This module demonstrates how to create functions that work transparently with
//! IntoExpressive values (scalar, nested expressions, and deferred computations).
//!
//! ## Pattern for Creating Expressive Functions
//!
//! To create a function like `first()` that handles all IntoExpressive variants:
//!
//! 1. **Function Signature**: Use `impl Into<IntoExpressive<Expression>>`
//!    ```rust
//!    fn your_function(input: impl Into<IntoExpressive<Expression>>) -> Expression
//!    ```
//!
//! 2. **Handle Each Variant**:
//!    - **Scalar**: Process the value directly and wrap result in expression
//!    - **Nested**: Create a new expression that references the nested one
//!    - **Deferred**: Create a deferred expression that recursively calls your function
//!
//! 3. **Pattern Template**:
//!    ```rust
//!    fn your_function(input: impl Into<IntoExpressive<Expression>>) -> Expression {
//!        let input = input.into();
//!        match input {
//!            IntoExpressive::Scalar(value) => {
//!                // Process scalar value directly
//!                let result = process_scalar(value);
//!                expr!("{}", result)
//!            }
//!            IntoExpressive::Nested(expr) => {
//!                // Reference the nested expression
//!                expr!("YOUR_FUNCTION({})", expr)
//!            }
//!            IntoExpressive::Deferred(inner_fn) => {
//!                // Create deferred that recursively calls your function
//!                expr!(
//!                    "{}",
//!                    IntoExpressive::deferred(move || {
//!                        let future = inner_fn();
//!                        Box::pin(async move {
//!                            let result = future.await;
//!                            IntoExpressive::Nested(your_function(result))
//!                        })
//!                    })
//!                )
//!            }
//!        }
//!    }
//!    ```
//!
//! ## Benefits of This Pattern
//!
//! - **Composability**: Functions can be chained and nested naturally
//! - **Type Safety**: All conversions are handled by the type system
//! - **Lazy Evaluation**: Deferred values are computed only when needed
//! - **Expression Building**: Creates proper SQL/query expressions
//! - **Recursion**: Deferred cases can handle nested deferred values automatically
//!
//! ## Usage Examples
//!
//! ```rust
//! // With scalar values
//! first(json!([1, 2, 3]))  // Returns expr with "1"
//!
//! // With expressions
//! first(example_expr!("SELECT * FROM items"))  // Returns "FIRST(SELECT * FROM items)"
//!
//! // With deferred computations
//! first(IntoExpressive::deferred(|| async { json!([1, 2, 3]) }))
//! ```

use serde_json::{Value, json};
use vantage_expressions::{protocol::expressive::Expressive, *};

use vantage_expressions::mocks::PatternDataSource;

#[derive(Clone)]
struct ExampleExpression {
    template: String,
    params: Vec<IntoExpressive<ExampleExpression>>,
}

impl ExampleExpression {
    fn new(template: &str, params: Vec<IntoExpressive<ExampleExpression>>) -> Self {
        Self {
            template: template.to_string(),
            params,
        }
    }

    fn preview(&self) -> String {
        let mut preview = self.template.clone();
        for param in &self.params {
            let param_str = param.preview();
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

impl std::fmt::Debug for ExampleExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.preview())
    }
}

impl Expressive<ExampleExpression> for ExampleExpression {
    fn expr(
        &self,
        template: &str,
        args: Vec<IntoExpressive<ExampleExpression>>,
    ) -> ExampleExpression {
        // Create a new expression with the template and arguments
        ExampleExpression::new(template, args)
    }
}

impl From<ExampleExpression> for IntoExpressive<ExampleExpression> {
    fn from(expr: ExampleExpression) -> Self {
        IntoExpressive::Nested(expr)
    }
}

macro_rules! example_expr {
    // Simple template without parameters: example_expr!("age")
    ($template:expr) => {
        ExampleExpression::new($template, vec![])
    };

    // Template with parameters: example_expr!("{} > {}", param1, param2)
    ($template:expr, $($param:expr),*) => {
        ExampleExpression::new(
            $template,
            vec![
                $(
                    $param.into()
                ),*
            ]
        )
    };
}

fn mock_where_clause(condition: impl Into<IntoExpressive<ExampleExpression>>) -> ExampleExpression {
    let condition_param = condition.into();
    ExampleExpression::new("WHERE {}", vec![condition_param])
}

fn first(input: impl Into<IntoExpressive<ExampleExpression>>) -> ExampleExpression {
    let input = input.into();
    match input {
        IntoExpressive::Scalar(Value::Array(arr)) => {
            let first_val = arr.into_iter().next().unwrap_or(Value::Null);
            example_expr!("{}", first_val)
        }
        IntoExpressive::Scalar(_) => {
            panic!("first() called with non-array scalar value")
        }
        IntoExpressive::Nested(n) => {
            example_expr!("FIRST({})", n)
        }
        IntoExpressive::Deferred(inner_fn) => {
            example_expr!(
                "{}",
                IntoExpressive::deferred(move || {
                    let future = inner_fn();
                    Box::pin(async move {
                        let result = future.await;
                        IntoExpressive::Nested(first(result))
                    })
                })
            )
        }
    }
}

#[test]
fn test_example_expression() {
    let expr = example_expr!("SELECT * FROM users");
    let expressive = IntoExpressive::nested(expr);
    assert_eq!(
        format!("{:?}", expressive.as_nested().unwrap()),
        "SELECT * FROM users"
    );
}

#[test]
fn test_mock_where_clause() {
    let where_expr = mock_where_clause(42i64);
    assert_eq!(format!("{:?}", where_expr), "WHERE 42");
}

#[test]
fn test_first() {
    // Test Scalar case with array
    let result = first(json!([1, 2, 3]));
    assert_eq!(result.preview(), "1");

    // Test Nested case
    let result = first(example_expr!("SELECT * FROM items"));
    assert_eq!(result.preview(), "FIRST(SELECT * FROM items)");

    // Test Deferred case
    let deferred = IntoExpressive::deferred(|| {
        Box::pin(async { IntoExpressive::Scalar(json!([100, 200, 300])) })
    });
    let result = first(deferred);
    assert_eq!(result.preview(), "**deferred()");
}

#[test]
fn test_from_i64() {
    let expr: IntoExpressive<String> = 42i64.into();
    if let Value::Number(n) = expr.as_scalar().unwrap() {
        assert_eq!(n.as_i64(), Some(42));
    } else {
        panic!("Expected scalar number");
    }
}

#[test]
fn test_example_expression_with_params() {
    let expr = example_expr!(
        "SELECT * FROM users WHERE id = {} AND name = {} AND department IN ({})",
        42i64,
        "hello",
        example_expr!("subquery")
    );

    assert_eq!(
        expr.preview(),
        "SELECT * FROM users WHERE id = 42 AND name = \"hello\" AND department IN (subquery)"
    );
}

#[tokio::test]
async fn test_datasource_basic() {
    let db = PatternDataSource::<ExampleExpression>::new()
        .with_pattern("SELECT * FROM items", json!([100, 200, 300, 400]));
    let expr = example_expr!("SELECT * FROM items");

    let closure = db.defer(expr);
    let result = closure().await;
    assert_eq!(result, json!([100, 200, 300, 400]));
}

#[tokio::test]
async fn test_datasource_with_scalar_mixing() {
    let db = PatternDataSource::<ExampleExpression>::new()
        .with_pattern("SELECT COUNT(*) FROM logs", json!(42));
    let subquery = example_expr!("SELECT COUNT(*) FROM logs");

    // Mix deferred result with scalar values
    let mixed_query = example_expr!(
        "SELECT * FROM events WHERE log_count = {} AND status = {} AND user_id = {}",
        IntoExpressive::deferred(move || {
            let db = db.clone();
            let subquery = subquery.clone();
            Box::pin(async move { IntoExpressive::Scalar(db.execute(&subquery).await) })
        }),
        "active",
        42i64
    );

    assert_eq!(
        mixed_query.preview(),
        "SELECT * FROM events WHERE log_count = **deferred() AND status = \"active\" AND user_id = 42"
    );
}

#[tokio::test]
async fn test_nested_queries() {
    let db = PatternDataSource::<ExampleExpression>::new()
        .with_pattern("SELECT * FROM items", json!([100, 200, 300, 400]));
    let expr = example_expr!("SELECT * FROM items");

    let result = db.execute(&expr).await;
    assert_eq!(result, json!([100, 200, 300, 400]));
}
