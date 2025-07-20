pub mod protocol;
pub mod select;

use vantage_expressions::{IntoExpressive, OwnedExpression, expr};

pub use select::Select;

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

impl From<Identifier> for IntoExpressive<OwnedExpression> {
    fn from(id: Identifier) -> Self {
        IntoExpressive::nested(id.into())
    }
}
