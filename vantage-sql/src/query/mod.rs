pub mod expressive;
pub mod join_query;
pub mod query_conditions;
pub mod query_source;
pub mod query_type;

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::Value;
use vantage_expressions::{Expressive, LazyExpression, OwnedExpression, expr};

use crate::Identifier;
use join_query::JoinQuery;
use query_conditions::QueryConditions;
use query_source::QuerySource;
use query_type::QueryType;

#[derive(Debug, Clone)]
pub struct Query {
    from: Vec<QuerySource>,
    with: IndexMap<String, QuerySource>,
    distinct: bool,
    query_type: QueryType,
    fields: IndexMap<Option<String>, Arc<Box<dyn Expressive>>>,
    set_fields: IndexMap<String, Value>,

    where_conditions: QueryConditions,
    having_conditions: QueryConditions,
    joins: Vec<JoinQuery>,

    skip_items: Option<i64>,
    limit_items: Option<i64>,

    group_by: Vec<LazyExpression>,
    order_by: Vec<LazyExpression>,
}

impl Query {
    pub fn new() -> Self {
        Self {
            from: Vec::new(),
            with: IndexMap::new(),
            distinct: false,
            query_type: QueryType,
            fields: IndexMap::new(),
            set_fields: IndexMap::new(),
            where_conditions: QueryConditions,
            having_conditions: QueryConditions,
            joins: Vec::new(),
            skip_items: None,
            limit_items: None,
            group_by: Vec::new(),
            order_by: Vec::new(),
        }
    }

    pub fn fields(mut self, fields: IndexMap<Option<String>, Arc<Box<dyn Expressive>>>) -> Self {
        self.fields = fields;
        self
    }

    pub fn from(mut self, sources: Vec<QuerySource>) -> Self {
        self.from = sources;
        self
    }

    fn render_fields(&self) -> OwnedExpression {
        if self.fields.is_empty() {
            expr!("*")
        } else {
            let field_expressions: Vec<OwnedExpression> = self
                .fields
                .iter()
                .map(|(alias, _field)| {
                    if let Some(alias) = alias {
                        Identifier::new(alias.clone()).into()
                    } else {
                        expr!("field")
                    }
                })
                .collect();
            OwnedExpression::from_vec(field_expressions, ", ")
        }
    }

    fn render_from(&self) -> OwnedExpression {
        if self.from.is_empty() {
            expr!("")
        } else {
            let from_expressions: Vec<OwnedExpression> = self
                .from
                .iter()
                .map(|source| source.clone().into())
                .collect();
            expr!(
                " FROM {}",
                OwnedExpression::from_vec(from_expressions, ", ")
            )
        }
    }
}

impl Into<OwnedExpression> for Query {
    fn into(self) -> OwnedExpression {
        expr!("SELECT {}{}", self.render_fields(), self.render_from())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_query() {
        let query = Query::new();
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT *");
    }

    #[test]
    fn test_query_with_fields() {
        let mut fields = IndexMap::new();
        fields.insert(
            Some("name".to_string()),
            Arc::new(Box::new(expr!("name")) as Box<dyn Expressive>),
        );
        fields.insert(
            Some("age".to_string()),
            Arc::new(Box::new(expr!("age")) as Box<dyn Expressive>),
        );

        let query = Query::new().fields(fields);
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name`, `age`");
    }

    #[test]
    fn test_query_with_from() {
        let query = Query::new().from(vec![QuerySource::new(Identifier::new("users"))]);

        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT * FROM `users`");
    }

    #[test]
    fn test_query_with_fields_and_from() {
        let mut fields = IndexMap::new();
        fields.insert(
            Some("name".to_string()),
            Arc::new(Box::new(expr!("name")) as Box<dyn Expressive>),
        );
        fields.insert(
            Some("age".to_string()),
            Arc::new(Box::new(expr!("age")) as Box<dyn Expressive>),
        );

        let query = Query::new()
            .fields(fields)
            .from(vec![QuerySource::new(Identifier::new("users"))]);

        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name`, `age` FROM `users`");
    }
}
