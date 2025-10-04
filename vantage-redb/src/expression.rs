use serde_json::Value;
use vantage_expressions::protocol::expressive::{Expressive, IntoExpressive};

/// RedbExpression represents operations available in ReDB key-value store.
/// Since ReDB is simple, we support basic value operations and equality conditions.
#[derive(Debug, Clone)]
pub enum RedbExpression {
    /// A simple value wrapper
    Value(Value),
    /// Equality condition with column name and value
    Eq { column: String, value: Value },
}

impl RedbExpression {
    pub fn new(value: Value) -> Self {
        Self::Value(value)
    }

    pub fn eq(column: String, value: Value) -> Self {
        Self::Eq { column, value }
    }

    pub fn value(&self) -> Option<&Value> {
        match self {
            Self::Value(v) => Some(v),
            Self::Eq { .. } => None,
        }
    }

    pub fn into_value(self) -> Option<Value> {
        match self {
            Self::Value(v) => Some(v),
            Self::Eq { .. } => None,
        }
    }

    pub fn as_eq(&self) -> Option<(&str, &Value)> {
        match self {
            Self::Eq { column, value } => Some((column, value)),
            Self::Value(_) => None,
        }
    }
}

impl Expressive<RedbExpression> for RedbExpression {
    fn expr(&self, template: &str, args: Vec<IntoExpressive<RedbExpression>>) -> RedbExpression {
        // For ReDB, we ignore templates and just wrap the first scalar value we find
        for arg in args {
            if let Some(scalar) = arg.as_scalar() {
                return RedbExpression::new(scalar.clone());
            }
        }
        // Fallback to template as string value
        RedbExpression::new(Value::String(template.to_string()))
    }
}

impl From<Value> for RedbExpression {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}

impl From<RedbExpression> for IntoExpressive<RedbExpression> {
    fn from(expr: RedbExpression) -> Self {
        IntoExpressive::Nested(expr)
    }
}

impl From<RedbExpression> for Value {
    fn from(expr: RedbExpression) -> Self {
        match expr {
            RedbExpression::Value(v) => v,
            RedbExpression::Eq { .. } => Value::Null, // Can't convert condition to value
        }
    }
}
