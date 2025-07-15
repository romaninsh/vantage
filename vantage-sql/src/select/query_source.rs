use crate::Identifier;
use std::sync::Arc;
use vantage_expressions::{OwnedExpression, expr};

#[derive(Debug, Clone)]
pub enum QuerySource {
    None,
    Table(String, Option<String>),
    Query(Arc<Box<crate::Select>>, Option<String>),
    Expression(OwnedExpression, Option<String>),
}

impl QuerySource {
    pub fn new(source: impl Into<OwnedExpression>) -> Self {
        Self::Expression(source.into(), None)
    }

    pub fn table(name: impl Into<String>) -> Self {
        Self::Table(name.into(), None)
    }

    pub fn table_with_alias(name: impl Into<String>, alias: impl Into<String>) -> Self {
        Self::Table(name.into(), Some(alias.into()))
    }

    pub fn query(query: crate::Select) -> Self {
        Self::Query(Arc::new(Box::new(query)), None)
    }

    pub fn query_with_alias(query: crate::Select, alias: impl Into<String>) -> Self {
        Self::Query(Arc::new(Box::new(query)), Some(alias.into()))
    }

    pub fn expression(expr: OwnedExpression) -> Self {
        Self::Expression(expr, None)
    }

    pub fn expression_with_alias(expr: OwnedExpression, alias: impl Into<String>) -> Self {
        Self::Expression(expr, Some(alias.into()))
    }

    pub fn with_alias(mut self, alias: String) -> Self {
        match &mut self {
            QuerySource::None => self,
            QuerySource::Table(_, a) => {
                *a = Some(alias);
                self
            }
            QuerySource::Query(_, a) => {
                *a = Some(alias);
                self
            }
            QuerySource::Expression(_, a) => {
                *a = Some(alias);
                self
            }
        }
    }

    pub fn render_with_prefix(&self, prefix: &str) -> OwnedExpression {
        match self {
            QuerySource::None => expr!(""),
            QuerySource::Table(table, None) => {
                if prefix.is_empty() {
                    Identifier::new(table).into()
                } else {
                    expr!("{}{}", prefix, Identifier::new(table))
                }
            }
            QuerySource::Table(table, Some(alias)) => {
                if prefix.is_empty() {
                    expr!("{} AS {}", Identifier::new(table), Identifier::new(alias))
                } else {
                    expr!(
                        "{}{} AS {}",
                        prefix,
                        Identifier::new(table),
                        Identifier::new(alias)
                    )
                }
            }
            QuerySource::Query(query, None) => {
                let subquery: OwnedExpression = query.as_ref().as_ref().clone().into();
                if prefix.is_empty() {
                    expr!("({})", subquery)
                } else {
                    expr!("{}({})", prefix, subquery)
                }
            }
            QuerySource::Query(query, Some(alias)) => {
                let subquery: OwnedExpression = query.as_ref().as_ref().clone().into();
                if prefix.is_empty() {
                    expr!("({}) AS {}", subquery, Identifier::new(alias))
                } else {
                    expr!("{}({}) AS {}", prefix, subquery, Identifier::new(alias))
                }
            }
            QuerySource::Expression(expression, None) => {
                if prefix.is_empty() {
                    expression.clone()
                } else {
                    expr!("{}{}", prefix, expression.clone())
                }
            }
            QuerySource::Expression(expression, Some(alias)) => {
                if prefix.is_empty() {
                    expr!("{} AS {}", expression.clone(), Identifier::new(alias))
                } else {
                    expr!(
                        "{}{} AS {}",
                        prefix,
                        expression.clone(),
                        Identifier::new(alias)
                    )
                }
            }
        }
    }
}

impl Into<OwnedExpression> for QuerySource {
    fn into(self) -> OwnedExpression {
        self.render_with_prefix("")
    }
}
