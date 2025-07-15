use crate::select::{QueryConditions, QuerySource};
use vantage_expressions::{OwnedExpression, expr};

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn new(join_type: JoinType, source: QuerySource, on_conditions: QueryConditions) -> Self {
        Self {
            join_type,
            source,
            on_conditions,
        }
    }

    pub fn inner(source: QuerySource) -> Self {
        Self::new(JoinType::Inner, source, QueryConditions::new())
    }

    pub fn left(source: QuerySource) -> Self {
        Self::new(JoinType::Left, source, QueryConditions::new())
    }

    pub fn right(source: QuerySource) -> Self {
        Self::new(JoinType::Right, source, QueryConditions::new())
    }

    pub fn full(source: QuerySource) -> Self {
        Self::new(JoinType::Full, source, QueryConditions::new())
    }

    pub fn on(mut self, condition: OwnedExpression) -> Self {
        self.on_conditions.add_condition(condition);
        self
    }

    pub fn add_on_condition(&mut self, condition: OwnedExpression) {
        self.on_conditions.add_condition(condition);
    }

    pub fn render(&self) -> OwnedExpression {
        let source = self.source.render_with_prefix("");
        let on_conditions = self.render_on_conditions();

        let join_clause = match self.join_type {
            JoinType::Inner => {
                if on_conditions.preview().is_empty() {
                    expr!(" JOIN {}", source)
                } else {
                    expr!(" JOIN {} {}", source, on_conditions)
                }
            }
            JoinType::Left => {
                if on_conditions.preview().is_empty() {
                    expr!(" LEFT JOIN {}", source)
                } else {
                    expr!(" LEFT JOIN {} {}", source, on_conditions)
                }
            }
            JoinType::Right => {
                if on_conditions.preview().is_empty() {
                    expr!(" RIGHT JOIN {}", source)
                } else {
                    expr!(" RIGHT JOIN {} {}", source, on_conditions)
                }
            }
            JoinType::Full => {
                if on_conditions.preview().is_empty() {
                    expr!(" FULL JOIN {}", source)
                } else {
                    expr!(" FULL JOIN {} {}", source, on_conditions)
                }
            }
        };

        join_clause
    }

    fn render_on_conditions(&self) -> OwnedExpression {
        if self.on_conditions.has_conditions() {
            let conditions = self.on_conditions.render_conditions();
            expr!("ON {}", conditions)
        } else {
            expr!("")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inner_join() {
        let source = QuerySource::table("users");
        let join = JoinQuery::inner(source).on(expr!("users.id = orders.user_id"));

        let result = join.render();
        assert_eq!(
            result.preview(),
            " JOIN `users` ON users.id = orders.user_id"
        );
    }

    #[test]
    fn test_left_join_with_alias() {
        let source = QuerySource::table_with_alias("users", "u");
        let join = JoinQuery::left(source).on(expr!("u.id = orders.user_id"));

        let result = join.render();
        assert_eq!(
            result.preview(),
            " LEFT JOIN `users` AS `u` ON u.id = orders.user_id"
        );
    }

    #[test]
    fn test_multiple_on_conditions() {
        let source = QuerySource::table("users");
        let join = JoinQuery::inner(source)
            .on(expr!("users.id = orders.user_id"))
            .on(expr!("orders.status = 'active'"));

        let result = join.render();
        assert_eq!(
            result.preview(),
            " JOIN `users` ON (users.id = orders.user_id) AND (orders.status = 'active')"
        );
    }

    #[test]
    fn test_join_types() {
        let source = QuerySource::table("users");

        let inner = JoinQuery::inner(source.clone()).on(expr!("users.id = orders.user_id"));
        assert_eq!(
            inner.render().preview(),
            " JOIN `users` ON users.id = orders.user_id"
        );

        let left = JoinQuery::left(source.clone()).on(expr!("users.id = orders.user_id"));
        assert_eq!(
            left.render().preview(),
            " LEFT JOIN `users` ON users.id = orders.user_id"
        );

        let right = JoinQuery::right(source.clone()).on(expr!("users.id = orders.user_id"));
        assert_eq!(
            right.render().preview(),
            " RIGHT JOIN `users` ON users.id = orders.user_id"
        );

        let full = JoinQuery::full(source).on(expr!("users.id = orders.user_id"));
        assert_eq!(
            full.render().preview(),
            " FULL JOIN `users` ON users.id = orders.user_id"
        );
    }
}
