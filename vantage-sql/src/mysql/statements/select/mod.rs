mod impls;
pub mod join;
mod render;

use crate::mysql::types::AnyMysqlType;
use crate::primitives::select::window::Window;
use join::MysqlSelectJoin;
use vantage_expressions::Expression;

type Expr = Expression<AnyMysqlType>;

/// MySQL SELECT statement builder.
#[derive(Debug, Clone)]
pub struct MysqlSelect {
    pub fields: Vec<Expr>,
    pub from: Vec<Expr>,
    pub joins: Vec<MysqlSelectJoin>,
    pub where_conditions: Vec<Expr>,
    pub order_by: Vec<(Expr, vantage_expressions::Order)>,
    pub group_by: Vec<Expr>,
    pub having: Vec<Expr>,
    pub windows: Vec<(String, Window<AnyMysqlType>)>,
    pub ctes: Vec<(String, Expr, bool)>,
    pub distinct: bool,
    pub with_rollup: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

impl MysqlSelect {
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
            with_rollup: false,
            limit: None,
            skip: None,
        }
    }

    pub fn with_rollup(mut self) -> Self {
        self.with_rollup = true;
        self
    }

    pub fn with_join(mut self, join: MysqlSelectJoin) -> Self {
        self.joins.push(join);
        self
    }

    pub fn add_having(&mut self, condition: impl vantage_expressions::Expressive<AnyMysqlType>) {
        self.having.push(condition.expr());
    }

    pub fn with_having(
        mut self,
        condition: impl vantage_expressions::Expressive<AnyMysqlType>,
    ) -> Self {
        self.having.push(condition.expr());
        self
    }

    pub fn with_window(mut self, name: impl Into<String>, window: Window<AnyMysqlType>) -> Self {
        self.windows.push((name.into(), window));
        self
    }

    pub fn with_cte(
        mut self,
        name: impl Into<String>,
        query: impl vantage_expressions::Expressive<AnyMysqlType>,
        recursive: bool,
    ) -> Self {
        self.ctes.push((name.into(), query.expr(), recursive));
        self
    }
}

impl Default for MysqlSelect {
    fn default() -> Self {
        Self::new()
    }
}
