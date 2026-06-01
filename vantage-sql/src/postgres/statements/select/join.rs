use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::primitives::identifier::ident;
use crate::primitives::select::{JoinBuilder, SelectBuilder};
use crate::postgres::types::AnyPostgresType;

type Expr = Expression<AnyPostgresType>;

#[derive(Debug, Clone)]
pub enum PostgresJoinType {
    Inner,
    Left,
    Right,
    FullOuter,
    // -- PostgreSQL-specific --
    LeftLateral,
    CrossLateral,
}

impl PostgresJoinType {
    fn as_str(&self) -> &'static str {
        match self {
            PostgresJoinType::Inner => "INNER JOIN",
            PostgresJoinType::Left => "LEFT JOIN",
            PostgresJoinType::Right => "RIGHT JOIN",
            PostgresJoinType::FullOuter => "FULL OUTER JOIN",
            PostgresJoinType::LeftLateral => "LEFT JOIN LATERAL",
            PostgresJoinType::CrossLateral => "CROSS JOIN LATERAL",
        }
    }
}

/// A JOIN clause for PostgresSelect.
#[derive(Debug, Clone)]
pub struct PostgresSelectJoin {
    join_type: PostgresJoinType,
    table: Expr,
    on_condition: Expr,
}

impl PostgresSelectJoin {
    fn new(join_type: PostgresJoinType, table: Expr, on_condition: Expr) -> Self {
        Self {
            join_type,
            table,
            on_condition,
        }
    }

    fn table_expr(table: impl Into<String>, alias: impl Into<String>) -> Expr {
        ident(table).with_alias(alias).expr()
    }

    fn subquery_expr(subquery: impl Expressive<AnyPostgresType>, alias: impl Into<String>) -> Expr {
        expr_any!("({}) AS {}", (subquery), (ident(alias)))
    }

    pub fn inner(
        table: impl Into<String>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            PostgresJoinType::Inner,
            Self::table_expr(table, alias),
            on_condition.into(),
        )
    }

    pub fn left(
        table: impl Into<String>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            PostgresJoinType::Left,
            Self::table_expr(table, alias),
            on_condition.into(),
        )
    }

    pub fn full_outer(
        table: impl Into<String>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            PostgresJoinType::FullOuter,
            Self::table_expr(table, alias),
            on_condition.into(),
        )
    }

    pub fn inner_expr(
        subquery: impl Expressive<AnyPostgresType>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            PostgresJoinType::Inner,
            Self::subquery_expr(subquery, alias),
            on_condition.into(),
        )
    }

    pub fn left_expr(
        subquery: impl Expressive<AnyPostgresType>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            PostgresJoinType::Left,
            Self::subquery_expr(subquery, alias),
            on_condition.into(),
        )
    }

    /// PostgreSQL-specific: `LEFT JOIN LATERAL (subquery) AS alias ON TRUE`
    pub fn left_lateral(
        subquery: impl Expressive<AnyPostgresType>,
        alias: impl Into<String>,
    ) -> Self {
        Self::new(
            PostgresJoinType::LeftLateral,
            Self::subquery_expr(subquery, alias),
            Expression::new("TRUE", vec![]),
        )
    }

    /// PostgreSQL-specific: `CROSS JOIN LATERAL (subquery) AS alias`
    pub fn cross_lateral(
        subquery: impl Expressive<AnyPostgresType>,
        alias: impl Into<String>,
    ) -> Self {
        Self::new(
            PostgresJoinType::CrossLateral,
            Self::subquery_expr(subquery, alias),
            Expression::new("", vec![]),
        )
    }

    /// PostgreSQL-specific: `CROSS JOIN LATERAL expr` (no wrapping parens).
    pub fn cross_lateral_raw(table_expr: impl Expressive<AnyPostgresType>) -> Self {
        Self::new(
            PostgresJoinType::CrossLateral,
            table_expr.expr(),
            Expression::new("", vec![]),
        )
    }

    pub fn render(&self) -> Expr {
        match self.join_type {
            PostgresJoinType::CrossLateral => {
                Expression::new(
                    format!(" {} {{}}", self.join_type.as_str()),
                    vec![ExpressiveEnum::Nested(self.table.clone())],
                )
            }
            _ => Expression::new(
                format!(" {} {{}} ON {{}}", self.join_type.as_str()),
                vec![
                    ExpressiveEnum::Nested(self.table.clone()),
                    ExpressiveEnum::Nested(self.on_condition.clone()),
                ],
            ),
        }
    }
}

impl SelectBuilder<AnyPostgresType> for super::PostgresSelect {
    type Join = PostgresSelectJoin;

    fn push_join(&mut self, join: PostgresSelectJoin) {
        self.joins.push(join);
    }

    fn push_having(&mut self, cond: Expr) {
        self.having.push(cond);
    }

    fn push_cte(&mut self, name: String, query: Expr, recursive: bool) {
        self.ctes.push((name, query, recursive));
    }
}

impl JoinBuilder<AnyPostgresType> for PostgresSelectJoin {
    fn make_inner(table: &str, alias: &str, on: Expr) -> Self {
        Self::inner(table, alias, on)
    }

    fn make_left(table: &str, alias: &str, on: Expr) -> Self {
        Self::left(table, alias, on)
    }
}
