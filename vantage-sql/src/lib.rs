pub mod protocol;
pub mod query;

use vantage_expressions::{OwnedExpression, expr};

#[derive(Debug, Clone)]
pub struct Identifier {
    identifier: String,
}

impl Identifier {
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
        }
    }
}

impl Into<OwnedExpression> for Identifier {
    fn into(self) -> OwnedExpression {
        expr!(format!("`{}`", self.identifier))
    }
}
