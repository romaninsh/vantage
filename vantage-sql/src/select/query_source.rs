use crate::Identifier;
use vantage_expressions::{OwnedExpression, expr};

#[derive(Debug, Clone)]
pub struct QuerySource {
    source: OwnedExpression,
    alias: Option<String>,
}

impl QuerySource {
    pub fn new(source: impl Into<OwnedExpression>) -> Self {
        Self {
            source: source.into(),
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: String) -> Self {
        self.alias = Some(alias);
        self
    }
}

impl Into<OwnedExpression> for QuerySource {
    fn into(self) -> OwnedExpression {
        match self.alias {
            Some(alias) => expr!("{} AS {}", self.source, Identifier::new(alias)),
            None => self.source,
        }
    }
}
