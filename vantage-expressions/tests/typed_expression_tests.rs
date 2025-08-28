//! # Typed Expression Tests
//!
//! This module explores a typed approach to expressions where ExampleExpression<T>
//! carries type information about what the expression will return when executed.
//!
//! ## Design Goals
//!
//! - Type-safe expression building with return type information
//! - Clean syntax for creating typed expressions
//! - DataSource that can execute expressions and return typed results
//! - Intuitive API that leverages Rust's type system

use chrono::{DateTime, Utc};
use std::{marker::PhantomData, pin::Pin};
use vantage_expressions::*;

// Supported types enum similar to IntoExpressive
#[derive(Clone)]
pub enum SupportedType {
    Bool(bool),
    String(String),
    Timestamp(DateTime<Utc>),
    Nested(ExampleExpression),
    Deferred(
        std::sync::Arc<
            dyn Fn() -> Pin<Box<dyn std::future::Future<Output = SupportedType> + Send>>
                + Send
                + Sync,
        >,
    ),
}

impl std::fmt::Debug for SupportedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SupportedType::Bool(b) => write!(f, "Bool({:?})", b),
            SupportedType::String(s) => write!(f, "String({:?})", s),
            SupportedType::Timestamp(dt) => write!(f, "Timestamp({:?})", dt),
            SupportedType::Nested(expr) => write!(f, "Nested({:?})", expr),
            SupportedType::Deferred(_) => write!(f, "Deferred(<closure>)"),
        }
    }
}

// Typed expression that knows its return type
#[derive(Clone)]
pub struct ExampleExpression<T = SupportedType> {
    template: String,
    params: Vec<IntoExpressive<ExampleExpression>>, // Parameters are untyped
    _phantom: PhantomData<T>,
}

impl<T> ExampleExpression<T> {
    fn new(template: &str, params: Vec<IntoExpressive<ExampleExpression>>) -> Self {
        Self {
            template: template.to_string(),
            params,
            _phantom: PhantomData,
        }
    }

    fn preview(&self) -> String {
        let mut preview = self.template.clone();
        for param in &self.params {
            let param_str = match param {
                IntoExpressive::Scalar(v) => format!("{}", v),
                IntoExpressive::Nested(expr) => expr.preview(),
                IntoExpressive::Deferred(_) => "**deferred()".to_string(),
            };
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

impl<T> std::fmt::Debug for ExampleExpression<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}<{}>", self.preview(), std::any::type_name::<T>())
    }
}

// Macro for creating typed expressions
macro_rules! example_expr {
    // Type-annotated version: example_expr::<Bool>("{} == {}", a, b)
    ($ty:ty; $template:expr) => {
        ExampleExpression::<$ty>::new($template, vec![])
    };

    ($ty:ty; $template:expr, $($param:expr),*) => {
        ExampleExpression::<$ty>::new(
            $template,
            vec![
                $(
                    $param.into()
                ),*
            ]
        )
    };
}

// From implementation for typed expressions - convert to untyped first
impl<T> From<ExampleExpression<T>> for IntoExpressive<ExampleExpression> {
    fn from(expr: ExampleExpression<T>) -> Self {
        let untyped = ExampleExpression {
            template: expr.template,
            params: expr.params,
            _phantom: PhantomData::<SupportedType>,
        };
        IntoExpressive::Nested(untyped)
    }
}

// Typed DataSource that can execute expressions with their correct return types
#[derive(Clone)]
struct TypedMockDatabase {
    patterns: Vec<(String, SupportedType)>,
}

impl TypedMockDatabase {
    fn new(patterns: Vec<(&str, SupportedType)>) -> Self {
        Self {
            patterns: patterns
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    // Execute typed expressions and return the correct type
    async fn execute<T>(&self, expr: &ExampleExpression<T>) -> T
    where
        T: FromSupportedType,
    {
        // Simulate database execution
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

        let query = expr.preview();
        for (pattern, supported_type) in &self.patterns {
            if query.contains(pattern) {
                return T::from_supported_type(supported_type.clone());
            }
        }
        T::from_supported_type(SupportedType::String("null".to_string()))
    }
}

impl SupportedType {}

// Trait for converting from SupportedType to specific types
trait FromSupportedType {
    fn from_supported_type(supported: SupportedType) -> Self;
}

impl FromSupportedType for bool {
    fn from_supported_type(supported: SupportedType) -> Self {
        match supported {
            SupportedType::Bool(b) => b,
            _ => panic!("Expected Bool, got {:?}", supported),
        }
    }
}

impl FromSupportedType for String {
    fn from_supported_type(supported: SupportedType) -> Self {
        match supported {
            SupportedType::String(s) => s,
            SupportedType::Bool(b) => b.to_string(),
            SupportedType::Timestamp(dt) => dt.to_rfc3339(),
            _ => panic!("Cannot convert {:?} to String", supported),
        }
    }
}

impl FromSupportedType for DateTime<Utc> {
    fn from_supported_type(supported: SupportedType) -> Self {
        match supported {
            SupportedType::Timestamp(dt) => dt,
            _ => panic!("Expected Timestamp, got {:?}", supported),
        }
    }
}

impl FromSupportedType for SupportedType {
    fn from_supported_type(supported: SupportedType) -> Self {
        supported
    }
}

// Helper functions for creating typed expressions
fn eq<T>(left: ExampleExpression<T>, right: ExampleExpression<T>) -> ExampleExpression<bool> {
    example_expr!(bool; "{} = {}", left, right)
}

fn select_name(table: &str, id: i64) -> ExampleExpression<String> {
    example_expr!(String; "(SELECT name FROM {} WHERE id = {})", table, id)
}

fn select_created_at(table: &str, id: i64) -> ExampleExpression<DateTime<Utc>> {
    example_expr!(DateTime<Utc>; "(SELECT created_at FROM {} WHERE id = {})", table, id)
}

#[tokio::test]
async fn test_typed_expressions() {
    let db = TypedMockDatabase::new(vec![
        (
            "SELECT name FROM \"users\" WHERE id = 1",
            SupportedType::String("Alice".to_string()),
        ),
        (
            "SELECT created_at FROM \"users\" WHERE id = 1",
            SupportedType::Timestamp(DateTime::from_timestamp(1640995200, 0).unwrap()),
        ),
        ("\"value1\" = \"value2\"", SupportedType::Bool(true)),
    ]);

    // Type-safe expression creation and execution
    let name_query = select_name("users", 1);
    let name_result: String = db.execute(&name_query).await;
    assert_eq!(name_result, "Alice".to_string());

    let timestamp_query = select_created_at("users", 1);
    let timestamp_result: DateTime<Utc> = db.execute(&timestamp_query).await;
    assert!(timestamp_result > DateTime::from_timestamp(0, 0).unwrap());

    let val1 = example_expr!(String; "\"value1\"");
    let val2 = example_expr!(String; "\"value2\"");
    let bool_query = eq(val1, val2);
    let bool_result: bool = db.execute(&bool_query).await;
    assert_eq!(bool_result, true);
}

#[test]
fn test_typed_expression_syntax() {
    // Clean syntax for creating typed expressions
    let a = "value1";
    let b = "value2";

    let expr_a = example_expr!(String; "{}", a);
    let expr_b = example_expr!(String; "{}", b);
    let comparison = eq(expr_a, expr_b);
    assert_eq!(comparison.preview(), "\"value1\" = \"value2\"");

    let name_expr = select_name("customers", 123);
    assert_eq!(
        name_expr.preview(),
        "(SELECT name FROM \"customers\" WHERE id = 123)"
    );

    let timestamp_expr = select_created_at("users", 1);
    assert_eq!(
        timestamp_expr.preview(),
        "(SELECT created_at FROM \"users\" WHERE id = 1)"
    );
}

#[test]
fn test_expression_composition() {
    // Expressions can be composed while maintaining type safety
    let user_name = select_name("users", 1);
    let admin_name = select_name("admins", 1);

    // This would be a Bool expression comparing two String expressions
    let same_name = eq(user_name, admin_name);
    assert_eq!(
        same_name.preview(),
        "(SELECT name FROM \"users\" WHERE id = 1) = (SELECT name FROM \"admins\" WHERE id = 1)"
    );
}
