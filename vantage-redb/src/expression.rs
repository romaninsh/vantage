use serde_json::Value;
use vantage_expressions::protocol::expressive::{Expressive, IntoExpressive};

/// RedbExpression is a minimal expression wrapper for ReDB key-value operations.
/// Since ReDB is a simple key-value store, expressions are just Value wrappers.
#[derive(Debug, Clone)]
pub struct RedbExpression {
    value: Value,
}

impl RedbExpression {
    pub fn new(value: Value) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
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
        expr.value
    }
}
