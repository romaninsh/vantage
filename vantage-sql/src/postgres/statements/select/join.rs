use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::postgres::types::AnyPostgresType;

type Expr = Expression<AnyPostgresType>;

#[derive(Debug, Clone)]
pub enum PostgresJoinType {
    Inner,
    Left,
    Right,
}

impl PostgresJoinType {
    fn as_str(&self) -> &'static str {
        match self {
            PostgresJoinType::Inner => "INNER JOIN",
            PostgresJoinType::Left => "LEFT JOIN",
            PostgresJoinType::Right => "RIGHT JOIN",
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
        Expression::new(
            format!("\"{}\" AS \"{}\"", table.into(), alias.into()),
            vec![],
        )
    }

    fn subquery_expr(subquery: impl Expressive<AnyPostgresType>, alias: impl Into<String>) -> Expr {
        Expression::new(
            format!("({{}}) AS \"{}\"", alias.into()),
            vec![ExpressiveEnum::Nested(subquery.expr())],
        )
    }

    pub fn inner(table: impl Into<String>, alias: impl Into<String>, on_condition: Expr) -> Self {
        Self::new(
            PostgresJoinType::Inner,
            Self::table_expr(table, alias),
            on_condition,
        )
    }

    pub fn left(table: impl Into<String>, alias: impl Into<String>, on_condition: Expr) -> Self {
        Self::new(
            PostgresJoinType::Left,
            Self::table_expr(table, alias),
            on_condition,
        )
    }

    pub fn inner_expr(
        subquery: impl Expressive<AnyPostgresType>,
        alias: impl Into<String>,
        on_condition: Expr,
    ) -> Self {
        Self::new(
            PostgresJoinType::Inner,
            Self::subquery_expr(subquery, alias),
            on_condition,
        )
    }

    pub fn left_expr(
        subquery: impl Expressive<AnyPostgresType>,
        alias: impl Into<String>,
        on_condition: Expr,
    ) -> Self {
        Self::new(
            PostgresJoinType::Left,
            Self::subquery_expr(subquery, alias),
            on_condition,
        )
    }

    pub fn render(&self) -> Expr {
        Expression::new(
            format!(" {} {{}} ON {{}}", self.join_type.as_str()),
            vec![
                ExpressiveEnum::Nested(self.table.clone()),
                ExpressiveEnum::Nested(self.on_condition.clone()),
            ],
        )
    }
}
