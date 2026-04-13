use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::primitives::identifier::ident;
use crate::sqlite::types::AnySqliteType;

type Expr = Expression<AnySqliteType>;

#[derive(Debug, Clone)]
pub enum SqliteJoinType {
    Inner,
    Left,
    Right,
}

impl SqliteJoinType {
    fn as_str(&self) -> &'static str {
        match self {
            SqliteJoinType::Inner => "INNER JOIN",
            SqliteJoinType::Left => "LEFT JOIN",
            SqliteJoinType::Right => "RIGHT JOIN",
        }
    }
}

/// A JOIN clause for SqliteSelect.
#[derive(Debug, Clone)]
pub struct SqliteSelectJoin {
    join_type: SqliteJoinType,
    table: Expr,
    on_condition: Expr,
}

impl SqliteSelectJoin {
    fn new(join_type: SqliteJoinType, table: Expr, on_condition: impl Into<Expr>) -> Self {
        Self {
            join_type,
            table,
            on_condition: on_condition.into(),
        }
    }

    fn table_expr(table: impl Into<String>, alias: impl Into<String>) -> Expr {
        ident(table).with_alias(alias).expr()
    }

    fn subquery_expr(subquery: impl Expressive<AnySqliteType>, alias: impl Into<String>) -> Expr {
        expr_any!("({}) AS {}", (subquery), (ident(alias)))
    }

    /// INNER JOIN on a named table.
    pub fn inner(table: impl Into<String>, alias: impl Into<String>, on_condition: impl Into<Expr>) -> Self {
        Self::new(
            SqliteJoinType::Inner,
            Self::table_expr(table, alias),
            on_condition,
        )
    }

    /// LEFT JOIN on a named table.
    pub fn left(table: impl Into<String>, alias: impl Into<String>, on_condition: impl Into<Expr>) -> Self {
        Self::new(
            SqliteJoinType::Left,
            Self::table_expr(table, alias),
            on_condition,
        )
    }

    /// INNER JOIN on a subquery/expression.
    pub fn inner_expr(
        subquery: impl Expressive<AnySqliteType>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            SqliteJoinType::Inner,
            Self::subquery_expr(subquery, alias),
            on_condition,
        )
    }

    /// LEFT JOIN on a subquery/expression.
    pub fn left_expr(
        subquery: impl Expressive<AnySqliteType>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            SqliteJoinType::Left,
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
