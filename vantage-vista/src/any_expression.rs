//! Type-erased expression wrapper
//!
//! `AnyExpression` provides a way to store expressions of different types uniformly
//! while preserving the ability to recover the concrete type through downcasting.

use std::any::{Any, TypeId};

/// Trait for expression types that can be type-erased
///
/// This trait is automatically implemented for any type that is Clone + Send + Sync + 'static,
/// which matches the requirements for TableSource::Expr
pub trait ExpressionLike: Send + Sync {
    /// Clone this expression into a Box
    fn clone_box(&self) -> Box<dyn ExpressionLike>;

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Get the TypeId of the concrete type
    fn type_id(&self) -> TypeId;

    /// Get the type name for debugging
    fn type_name(&self) -> &'static str;
}

impl<T> ExpressionLike for T
where
    T: Clone + Send + Sync + 'static,
{
    fn clone_box(&self) -> Box<dyn ExpressionLike> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

/// Type-erased expression that can be downcast to concrete expression types
pub struct AnyExpression {
    inner: Box<dyn ExpressionLike>,
    type_id: TypeId,
    type_name: &'static str,
}

impl AnyExpression {
    /// Create a new AnyExpression from a concrete expression type
    pub fn new<T: Clone + Send + Sync + 'static>(expr: T) -> Self {
        Self {
            inner: Box::new(expr),
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
        }
    }

    /// Attempt to downcast to a concrete expression type
    pub fn downcast<T: Clone + 'static>(self) -> Result<T, Self> {
        if self.type_id != TypeId::of::<T>() {
            return Err(self);
        }

        let any = self.inner.as_any();
        match any.downcast_ref::<T>() {
            Some(expr) => Ok(expr.clone()),
            None => Err(self),
        }
    }

    /// Get a reference to the expression as a specific type
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        if self.type_id != TypeId::of::<T>() {
            return None;
        }
        self.inner.as_any().downcast_ref::<T>()
    }

    /// Check if this expression matches the given type
    pub fn is_type<T: 'static>(&self) -> bool {
        self.type_id == TypeId::of::<T>()
    }

    /// Get the expression type name for debugging
    pub fn type_name(&self) -> &str {
        self.type_name
    }

    /// Get the TypeId
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Convert to a boxed Any for advanced use cases
    pub fn into_any(self) -> Box<dyn Any> {
        // We need to extract the inner value through Any
        // This is a bit tricky since we can't directly convert ExpressionLike to Any
        Box::new(self)
    }
}

impl Clone for AnyExpression {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
            type_id: self.type_id,
            type_name: self.type_name,
        }
    }
}

impl std::fmt::Debug for AnyExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyExpression")
            .field("type_name", &self.type_name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestExpr {
        value: i32,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct OtherExpr {
        text: String,
    }

    #[test]
    fn test_any_expression_creation_and_downcast() {
        let expr = TestExpr { value: 42 };
        let any = AnyExpression::new(expr.clone());

        assert_eq!(any.type_name(), std::any::type_name::<TestExpr>());
        assert!(any.is_type::<TestExpr>());

        // Successful downcast
        let recovered = any.downcast::<TestExpr>().unwrap();
        assert_eq!(recovered, expr);
    }

    #[test]
    fn test_any_expression_downcast_ref() {
        let expr = TestExpr { value: 42 };
        let any = AnyExpression::new(expr.clone());

        // Successful downcast_ref
        let expr_ref = any.downcast_ref::<TestExpr>().unwrap();
        assert_eq!(expr_ref, &expr);

        // Can still use any after downcast_ref
        assert!(any.is_type::<TestExpr>());
    }

    #[test]
    fn test_any_expression_downcast_wrong_type() {
        let expr = TestExpr { value: 42 };
        let any = AnyExpression::new(expr);

        // Try to downcast to wrong type
        let result = any.downcast::<OtherExpr>();
        assert!(result.is_err());
    }

    #[test]
    fn test_any_expression_is_type() {
        let expr = TestExpr { value: 42 };
        let any = AnyExpression::new(expr);

        assert!(any.is_type::<TestExpr>());
        assert!(!any.is_type::<OtherExpr>());
    }

    #[test]
    fn test_any_expression_clone() {
        let expr = TestExpr { value: 42 };
        let any = AnyExpression::new(expr.clone());
        let cloned = any.clone();

        assert_eq!(cloned.type_name(), any.type_name());
        assert_eq!(cloned.type_id(), any.type_id());

        // Both should downcast successfully
        let recovered1 = any.downcast::<TestExpr>().unwrap();
        let recovered2 = cloned.downcast::<TestExpr>().unwrap();
        assert_eq!(recovered1, recovered2);
    }

    #[test]
    fn test_any_expression_debug() {
        let expr = TestExpr { value: 42 };
        let any = AnyExpression::new(expr);

        let debug_str = format!("{:?}", any);
        assert!(debug_str.contains("AnyExpression"));
        assert!(debug_str.contains("type_name"));
    }
}
