use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;

type Expr = Expression<AnyMysqlType>;

#[derive(Debug, Clone)]
pub enum MysqlJoinType {
    Inner,
    Left,
    Right,
}

impl MysqlJoinType {
    fn as_str(&self) -> &'static str {
        match self {
            MysqlJoinType::Inner => "INNER JOIN",
            MysqlJoinType::Left => "LEFT JOIN",
            MysqlJoinType::Right => "RIGHT JOIN",
        }
    }
}

/// A JOIN clause for MysqlSelect.
#[derive(Debug, Clone)]
pub struct MysqlSelectJoin {
    join_type: MysqlJoinType,
    table: Expr,
    on_condition: Expr,
}

impl MysqlSelectJoin {
    fn new(join_type: MysqlJoinType, table: Expr, on_condition: impl Into<Expr>) -> Self {
        Self {
            join_type,
            table,
            on_condition: on_condition.into(),
        }
    }

    fn table_expr(table: impl Into<String>, alias: impl Into<String>) -> Expr {
        ident(table).with_alias(alias).expr()
    }

    fn subquery_expr(subquery: impl Expressive<AnyMysqlType>, alias: impl Into<String>) -> Expr {
        expr_any!("({}) AS {}", (subquery), (ident(alias)))
    }

    pub fn inner(table: impl Into<String>, alias: impl Into<String>, on_condition: impl Into<Expr>) -> Self {
        Self::new(
            MysqlJoinType::Inner,
            Self::table_expr(table, alias),
            on_condition,
        )
    }

    pub fn left(table: impl Into<String>, alias: impl Into<String>, on_condition: impl Into<Expr>) -> Self {
        Self::new(
            MysqlJoinType::Left,
            Self::table_expr(table, alias),
            on_condition,
        )
    }

    pub fn inner_expr(
        subquery: impl Expressive<AnyMysqlType>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            MysqlJoinType::Inner,
            Self::subquery_expr(subquery, alias),
            on_condition,
        )
    }

    pub fn left_expr(
        subquery: impl Expressive<AnyMysqlType>,
        alias: impl Into<String>,
        on_condition: impl Into<Expr>,
    ) -> Self {
        Self::new(
            MysqlJoinType::Left,
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
