use vantage_expressions::{OwnedExpression, expr};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Variable {
    name: String,
}

impl Variable {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl Into<OwnedExpression> for Variable {
    fn into(self) -> OwnedExpression {
        expr!(format!("${}", self.name))
    }
}
