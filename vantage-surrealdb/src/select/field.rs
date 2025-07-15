use vantage_expressions::OwnedExpression;

use crate::identifier::Identifier;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Field {
    field: String,
}

impl Field {
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
        }
    }
}

impl Into<OwnedExpression> for Field {
    fn into(self) -> OwnedExpression {
        Identifier::new(self.field).into()
    }
}
