use vantage_expressions::OwnedExpression;

#[derive(Debug, Clone)]
pub struct Target {
    target: OwnedExpression,
}

impl Target {
    pub fn new(target: impl Into<OwnedExpression>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl Into<OwnedExpression> for Target {
    fn into(self) -> OwnedExpression {
        self.target
    }
}
