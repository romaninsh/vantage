mod impls;
pub mod join;
mod render;

use crate::postgres::types::AnyPostgresType;
use crate::primitives::select::window::Window;
use join::PostgresSelectJoin;
use vantage_expressions::Expression;

type Expr = Expression<AnyPostgresType>;

/// PostgreSQL SELECT statement builder.
#[derive(Debug, Clone)]
pub struct PostgresSelect {
    pub fields: Vec<Expr>,
    pub from: Vec<Expr>,
    pub joins: Vec<PostgresSelectJoin>,
    pub where_conditions: Vec<Expr>,
    pub order_by: Vec<(Expr, vantage_expressions::Order)>,
    pub group_by: Vec<Expr>,
    pub having: Vec<Expr>,
    pub windows: Vec<(String, Window<AnyPostgresType>)>,
    pub ctes: Vec<(String, Expr, bool)>,
    pub distinct: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,

    // -- PostgreSQL-specific fields --
    pub distinct_on: Vec<Expr>,
}

impl PostgresSelect {
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            from: Vec::new(),
            joins: Vec::new(),
            where_conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            having: Vec::new(),
            windows: Vec::new(),
            ctes: Vec::new(),
            distinct: false,
            limit: None,
            skip: None,
            distinct_on: Vec::new(),
        }
    }

    pub fn with_join(mut self, join: PostgresSelectJoin) -> Self {
        self.joins.push(join);
        self
    }

    pub fn add_having(&mut self, condition: impl vantage_expressions::Expressive<AnyPostgresType>) {
        self.having.push(condition.expr());
    }

    pub fn with_having(
        mut self,
        condition: impl vantage_expressions::Expressive<AnyPostgresType>,
    ) -> Self {
        self.having.push(condition.expr());
        self
    }

    pub fn with_window(mut self, name: impl Into<String>, window: Window<AnyPostgresType>) -> Self {
        self.windows.push((name.into(), window));
        self
    }

    pub fn with_cte(
        mut self,
        name: impl Into<String>,
        query: impl vantage_expressions::Expressive<AnyPostgresType>,
        recursive: bool,
    ) -> Self {
        self.ctes.push((name.into(), query.expr(), recursive));
        self
    }
}

// -- PostgreSQL-specific methods --

impl PostgresSelect {
    /// `SELECT DISTINCT ON (expr, ...) ...`
    /// ORDER BY must start with the DISTINCT ON expressions.
    pub fn with_distinct_on(
        mut self,
        expr: impl vantage_expressions::Expressive<AnyPostgresType>,
    ) -> Self {
        self.distinct_on.push(expr.expr());
        self
    }
}

impl Default for PostgresSelect {
    fn default() -> Self {
        Self::new()
    }
}
