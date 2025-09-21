pub mod protocol;
pub mod select;

use vantage_expressions::{Expression, IntoExpressive, expr};

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

impl Into<Expression> for Identifier {
    fn into(self) -> Expression {
        expr!(format!("`{}`", self.identifier))
    }
}

impl From<Identifier> for IntoExpressive<Expression> {
    fn from(id: Identifier) -> Self {
        IntoExpressive::nested(id.into())
    }
}
