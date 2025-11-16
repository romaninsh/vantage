use std::marker::PhantomData;
use surreal_client::types::{Any, SurrealType};
use vantage_expressions::{Expr, Expression, expr};

/// Wrapper for Expression that carries type information
///
/// TypedExpression combines an expression with SurrealDB type metadata,
/// allowing expressions to maintain type safety while remaining compatible
/// with the generic expression system.
#[derive(Clone)]
pub struct TypedExpression<T: SurrealType> {
    pub expr: Expr,
    _type: PhantomData<T>,
}

impl<T: SurrealType> TypedExpression<T> {
    /// Create a new typed expression
    pub fn new(expr: impl Into<Expr>) -> Self {
        Self {
            expr: expr.into(),
            _type: PhantomData,
        }
    }

    /// Get the type name
    pub fn type_name(&self) -> &'static str {
        T::type_name_static()
    }

    /// Get the underlying expression
    pub fn inner(&self) -> &Expr {
        &self.expr
    }

    /// Consume self and return the inner expression
    pub fn into_inner(self) -> Expr {
        self.expr
    }

    /// Preview the expression for debugging
    pub fn preview(&self) -> String {
        match &self.expr {
            vantage_expressions::protocol::expressive::IntoExpressive::Nested(e) => e.preview(),
            vantage_expressions::protocol::expressive::IntoExpressive::Scalar(v) => {
                format!("{}", v)
            }
            vantage_expressions::protocol::expressive::IntoExpressive::Deferred(_) => {
                "**deferred()".to_string()
            }
        }
    }

    /// Equality comparison - can compare with same type
    pub fn eq(&self, other: TypedExpression<T>) -> Expression {
        expr!("{} = {}", self.expr.clone(), other.expr)
    }

    /// Subtraction - can subtract same type
    pub fn sub(&self, other: TypedExpression<T>) -> Expression {
        expr!("{} - {}", self.expr.clone(), other.expr)
    }

    /// Contains operation - can check with same type
    pub fn contains(&self, other: TypedExpression<T>) -> Expression {
        expr!("{} CONTAINS {}", self.expr.clone(), other.expr)
    }

    /// IN operation - can check with same type
    pub fn in_(&self, other: TypedExpression<T>) -> Expression {
        expr!("{} IN {}", self.expr.clone(), other.expr)
    }
}

/// Special implementations for Any type - can compare with any value
impl TypedExpression<Any> {
    /// Equality comparison with any Expr-compatible value
    pub fn eq_value(&self, other: impl Into<Expr>) -> Expression {
        expr!("{} = {}", self.expr.clone(), other.into())
    }

    /// Subtraction with any Expr-compatible value
    pub fn sub_value(&self, other: impl Into<Expr>) -> Expression {
        expr!("{} - {}", self.expr.clone(), other.into())
    }

    /// Contains operation with any Expr-compatible value
    pub fn contains_value(&self, other: impl Into<Expr>) -> Expression {
        expr!("{} CONTAINS {}", self.expr.clone(), other.into())
    }

    /// IN operation with any Expr-compatible value
    pub fn in_value(&self, other: impl Into<Expr>) -> Expression {
        expr!("{} IN {}", self.expr.clone(), other.into())
    }
    /// Cast Any to a specific type
    ///
    /// This allows you to use Any expressions with typed expressions:
    /// ```ignore
    /// let any_expr: TypedExpression<Any> = ...;
    /// let age: TypedExpression<i64> = ...;
    /// age.eq(any_expr.cast_to())
    /// ```
    pub fn cast_to<T: SurrealType>(self) -> TypedExpression<T> {
        TypedExpression {
            expr: self.expr,
            _type: PhantomData,
        }
    }
}

impl<T: SurrealType> From<TypedExpression<T>> for Expr {
    fn from(typed: TypedExpression<T>) -> Self {
        typed.expr
    }
}

impl<T: SurrealType> From<TypedExpression<T>> for Expression {
    fn from(typed: TypedExpression<T>) -> Self {
        match typed.expr {
            vantage_expressions::protocol::expressive::IntoExpressive::Nested(e) => e,
            vantage_expressions::protocol::expressive::IntoExpressive::Scalar(v) => {
                Expression::new("{}", vec![v.into()])
            }
            vantage_expressions::protocol::expressive::IntoExpressive::Deferred(_) => {
                panic!("Cannot convert deferred expression to Expression")
            }
        }
    }
}

// Convenience conversions for String type
impl From<&str> for TypedExpression<String> {
    fn from(s: &str) -> Self {
        TypedExpression::new(s)
    }
}

impl From<String> for TypedExpression<String> {
    fn from(s: String) -> Self {
        TypedExpression::new(s)
    }
}

impl<T: SurrealType> crate::operation::Expressive for TypedExpression<T> {
    fn expr(&self) -> Expression {
        match &self.expr {
            vantage_expressions::protocol::expressive::IntoExpressive::Nested(e) => e.clone(),
            vantage_expressions::protocol::expressive::IntoExpressive::Scalar(v) => {
                Expression::new("{}", vec![v.clone().into()])
            }
            vantage_expressions::protocol::expressive::IntoExpressive::Deferred(_) => {
                panic!("Cannot convert deferred expression to Expression")
            }
        }
    }
}

impl<T: SurrealType> std::fmt::Debug for TypedExpression<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypedExpression")
            .field("expression", &self.expr)
            .field("type", &T::type_name_static())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typed_expression_creation() {
        let expr: TypedExpression<String> =
            TypedExpression::new(vantage_expressions::expr!("name"));
        assert_eq!(expr.type_name(), "string");
    }

    #[test]
    fn test_typed_expression_conversion() {
        let typed: TypedExpression<i64> = TypedExpression::new(vantage_expressions::expr!("age"));
        let _expr: Expr = typed.into();
    }

    #[test]
    fn test_typed_eq_same_type() {
        let age1: TypedExpression<i64> = TypedExpression::new(expr!("age"));
        let age2: TypedExpression<i64> = TypedExpression::new(expr!("18"));
        let result = age1.eq(age2);
        assert_eq!(result.preview(), "age = 18");
    }

    #[test]
    fn test_typed_eq_with_any() {
        let age: TypedExpression<i64> = TypedExpression::new(expr!("age"));
        let any_val: TypedExpression<Any> = TypedExpression::new(expr!("some_value"));
        let result = age.eq(any_val.cast_to());
        assert_eq!(result.preview(), "age = some_value");
    }

    #[test]
    fn test_any_eq_value() {
        let any_field: TypedExpression<Any> = TypedExpression::new(expr!("field"));
        let result = any_field.eq_value(4);
        assert_eq!(result.preview(), "field = 4");

        let result2 = any_field.eq_value("foo");
        assert_eq!(result2.preview(), "field = \"foo\"");
    }

    #[test]
    fn test_typed_operations() {
        let price1: TypedExpression<i64> = TypedExpression::new(expr!("price"));
        let price2: TypedExpression<i64> = TypedExpression::new(expr!("discount"));

        let sub_result = price1.sub(price2);
        assert_eq!(sub_result.preview(), "price - discount");
    }

    #[test]
    fn test_typed_contains() {
        let tags: TypedExpression<String> = TypedExpression::new(expr!("tags"));
        let tag: TypedExpression<String> = TypedExpression::new(expr!("'baked'"));

        let result = tags.contains(tag);
        assert_eq!(result.preview(), "tags CONTAINS 'baked'");
    }

    #[test]
    fn test_type_safety_comprehensive() {
        // typed_expr_any.eq(4) - OK
        let any_field: TypedExpression<Any> = TypedExpression::new(expr!("field"));
        let result1 = any_field.eq_value(4);
        assert_eq!(result1.preview(), "field = 4");

        // typed_expr_any.eq("foo") - OK
        let result2 = any_field.eq_value("foo");
        assert_eq!(result2.preview(), "field = \"foo\"");

        // typed_expr_int.eq(typed_expr_int) - OK
        let age1: TypedExpression<i64> = TypedExpression::new(expr!("age"));
        let age2: TypedExpression<i64> = TypedExpression::new(expr!("18"));
        let result3 = age1.eq(age2);
        assert_eq!(result3.preview(), "age = 18");

        // typed_expr_X.eq(typed_expr_any.cast_to()) - OK
        let price: TypedExpression<i64> = TypedExpression::new(expr!("price"));
        let any_value: TypedExpression<Any> = TypedExpression::new(expr!("value"));
        let result4 = price.eq(any_value.cast_to());
        assert_eq!(result4.preview(), "price = value");
    }

    // These should NOT compile (commented out for documentation):

    // typed_expr_int.eq(4) - Type error: eq expects TypedExpression<i64>, not i64
    // #[test]
    // fn test_typed_eq_raw_value() {
    //     let age: TypedExpression<i64> = TypedExpression::new(expr!("age"));
    //     let result = age.eq(4); // ERROR: expected TypedExpression<i64>, found i64
    // }

    // typed_expr_int.eq("foo") - Type error: eq expects TypedExpression<i64>, not &str
    // #[test]
    // fn test_typed_eq_wrong_value_type() {
    //     let age: TypedExpression<i64> = TypedExpression::new(expr!("age"));
    //     let result = age.eq("foo"); // ERROR: expected TypedExpression<i64>, found &str
    // }

    // typed_expr_X.eq(typed_expr_Y) - Type error: mismatched types
    // #[test]
    // fn test_typed_eq_different_types() {
    //     let age: TypedExpression<i64> = TypedExpression::new(expr!("age"));
    //     let name: TypedExpression<String> = TypedExpression::new(expr!("name"));
    //     let result = age.eq(name); // ERROR: expected TypedExpression<i64>, found TypedExpression<String>
    // }

    #[test]
    fn test_from_str_conversion() {
        let name_expr: TypedExpression<String> = "John".into();
        let id_expr: TypedExpression<String> = "user:123".into();

        let result = name_expr.eq(id_expr);
        assert_eq!(result.preview(), "\"John\" = \"user:123\"");
    }

    #[test]
    fn test_from_string_conversion() {
        let value: TypedExpression<String> = String::from("test").into();
        assert_eq!(value.type_name(), "string");
    }
}
