//! Type-erased expression wrapper: store expressions of different types
//! uniformly and recover the concrete type by downcasting.

use std::any::{Any, TypeId};

/// Implemented for every `Clone + Send + Sync + 'static` type.
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

    /// Box this wrapper (not the stored expression) as `Any`.
    pub fn into_any(self) -> Box<dyn Any> {
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
