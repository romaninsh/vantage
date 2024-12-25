use std::sync::Arc;

use crate::{expr, expr_arc, prelude::Expression, prelude::ExpressionArc, sql::chunk::Chunk};

use super::Query;

#[derive(Debug, Clone)]
pub enum QueryType {
    Select,
    Insert,
    Update,
    Replace,
    Delete,
    Expression(Expression),
}

#[derive(Debug, Clone)]
pub enum QuerySource {
    None,
    Table(String, Option<String>),
    Query(Arc<Box<Query>>, Option<String>),
    Expression(Expression, Option<String>),
}
impl QuerySource {
    pub fn render_prefix(&self, prefix: &str) -> Expression {
        match self {
            QuerySource::None => Expression::empty(),
            QuerySource::Query(query, None) => {
                expr_arc!(format!("{}({{}})", prefix), query.render_chunk()).render_chunk()
            }
            QuerySource::Query(query, Some(alias)) => expr_arc!(
                format!("{}({{}}) AS {}", prefix, alias),
                query.render_chunk()
            )
            .render_chunk(),
            QuerySource::Table(table, None) => expr!(format!("{}{}", prefix, table)),
            QuerySource::Table(table, Some(alias)) => {
                expr!(format!("{}{} AS {}", prefix, table, alias))
            }
            QuerySource::Expression(expression, None) => {
                expr_arc!(format!("{}{{}}", prefix), expression.render_chunk()).render_chunk()
            }
            QuerySource::Expression(expression, Some(alias)) => expr_arc!(
                format!("{}{{}} AS {}", prefix, alias),
                expression.render_chunk()
            )
            .render_chunk(),
        }
    }
}
impl Chunk for QuerySource {
    fn render_chunk(&self) -> Expression {
        self.render_prefix("FROM ")
    }
}

#[derive(Debug, Clone)]
pub enum ConditionType {
    Where,
    Having,
    On,
}

#[derive(Debug, Clone)]
pub struct QueryConditions {
    condition_type: ConditionType,
    conditions: Vec<Expression>,
}
impl QueryConditions {
    pub fn where_() -> QueryConditions {
        QueryConditions {
            condition_type: ConditionType::Where,
            conditions: Vec::new(),
        }
    }
    pub fn having() -> QueryConditions {
        QueryConditions {
            condition_type: ConditionType::Having,
            conditions: Vec::new(),
        }
    }
    pub fn on() -> QueryConditions {
        QueryConditions {
            condition_type: ConditionType::On,
            conditions: Vec::new(),
        }
    }
    pub fn add_condition(&mut self, condition: Expression) {
        self.conditions.push(condition);
    }
    pub fn with_condition(mut self, condition: Expression) -> Self {
        self.add_condition(condition);
        self
    }
}
impl Chunk for QueryConditions {
    fn render_chunk(&self) -> Expression {
        if self.conditions.is_empty() {
            Expression::empty()
        } else {
            let result = Expression::from_vec(self.conditions.clone(), " AND ");
            match self.condition_type {
                ConditionType::Where => expr_arc!(" WHERE {}", result).render_chunk(),
                ConditionType::Having => expr_arc!(" HAVING {}", result).render_chunk(),
                ConditionType::On => expr_arc!("ON {}", result).render_chunk(),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
}

#[derive(Debug, Clone)]
pub struct JoinQuery {
    join_type: JoinType,
    source: QuerySource,
    on_conditions: QueryConditions,
}
impl JoinQuery {
    pub fn new(
        join_type: JoinType,
        source: QuerySource,
        on_conditions: QueryConditions,
    ) -> JoinQuery {
        JoinQuery {
            join_type,
            source,
            on_conditions,
        }
    }
}
impl Chunk for JoinQuery {
    fn render_chunk(&self) -> Expression {
        let join_type = match self.join_type {
            JoinType::Inner => "JOIN ",
            JoinType::Left => "LEFT JOIN ",
            JoinType::Right => "RIGHT JOIN ",
            JoinType::Full => "FULL JOIN ",
        };
        let source = self.source.render_prefix(join_type);
        let on_conditions = self.on_conditions.render_chunk();
        expr_arc!(" {} {}", source, on_conditions).render_chunk()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::{
        prelude::PgValueColumn,
        sql::{Condition, Operations},
    };

    use super::*;

    #[test]
    fn test_query_source_render() {
        let query = QuerySource::Table("user".to_string(), None);
        let result = query.render_chunk().split();

        assert_eq!(result.0, "FROM user");
        assert_eq!(result.1.len(), 0);
    }

    #[test]
    fn test_conditions_render() {
        let conditions = QueryConditions {
            condition_type: ConditionType::Where,
            conditions: vec![expr!("name = {}", "John"), expr!("age > {}", 30)],
        };
        let result = conditions.render_chunk().split();

        assert_eq!(result.0, " WHERE name = {} AND age > {}");
        assert_eq!(result.1.len(), 2);
        assert_eq!(result.1[0], Value::String("John".to_string()));
        assert_eq!(result.1[1], Value::Number(30.into()));
    }

    #[test]
    fn test_conditions_render_having() {
        let conditions = QueryConditions::having()
            .with_condition(expr!("name = {}", "John"))
            .with_condition(expr!("age > {}", 30));
        let result = conditions.render_chunk().split();

        assert_eq!(result.0, " HAVING name = {} AND age > {}");
        assert_eq!(result.1.len(), 2);
        assert_eq!(result.1[0], Value::String("John".to_string()));
        assert_eq!(result.1[1], Value::Number(30.into()));
    }

    #[test]
    fn test_conditions_expressions() {
        let name = Arc::new(PgValueColumn::new("name"));
        let surname = Arc::new(PgValueColumn::new("surname"));

        let conditions = QueryConditions::having().with_condition(
            Condition::or(name.eq(&surname), surname.eq(&Value::Null)).render_chunk(),
        );
        let result = conditions.render_chunk().split();

        assert_eq!(result.0, " HAVING ((name = surname) OR (surname = {}))");
        assert_eq!(result.1.len(), 1);
        assert_eq!(result.1[0], Value::Null);
    }

    #[test]
    fn test_join_query_render() {
        let join_query = JoinQuery {
            join_type: JoinType::Inner,
            source: QuerySource::Table("user".to_string(), None),
            on_conditions: QueryConditions {
                condition_type: ConditionType::On,
                conditions: vec![expr!("user.id = address.user_id")],
            },
        };
        let result = join_query.render_chunk().split();

        assert_eq!(result.0, " JOIN user ON user.id = address.user_id");
        assert_eq!(result.1.len(), 0);
    }

    #[test]
    fn test_join_with_alias_render() {
        let join_query = JoinQuery {
            join_type: JoinType::Inner,
            source: QuerySource::Table("user".to_string(), Some("u".to_string())),
            on_conditions: QueryConditions {
                condition_type: ConditionType::On,
                conditions: vec![expr!("u.id = address.user_id")],
            },
        };
        let result = join_query.render_chunk().split();

        assert_eq!(result.0, " JOIN user AS u ON u.id = address.user_id");
        assert_eq!(result.1.len(), 0);
    }
}
