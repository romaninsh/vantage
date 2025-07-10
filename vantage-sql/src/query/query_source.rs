use vantage_expressions::OwnedExpression;

#[derive(Debug, Clone)]
pub struct QuerySource {
    source: OwnedExpression,
}

impl QuerySource {
    pub fn new(source: impl Into<OwnedExpression>) -> Self {
        Self {
            source: source.into(),
        }
    }
}

impl Into<OwnedExpression> for QuerySource {
    fn into(self) -> OwnedExpression {
        self.source
    }
}
