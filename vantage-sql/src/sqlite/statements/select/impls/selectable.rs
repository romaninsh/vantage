use vantage_expressions::traits::selectable::{Selectable, SourceRef};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::primitives::fx::Fx;
use crate::primitives::identifier::ident;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::types::AnySqliteType;

type Expr = Expression<AnySqliteType>;

impl SqliteSelect {
    pub fn as_aggregate(&self, func: &str, column: impl Expressive<AnySqliteType>) -> Expr {
        let mut s = self.clone();
        s.clear_fields();
        s.add_expression(Fx::new(func, [column.expr()]), None);
        s.clear_order_by();
        s.render()
    }
}

impl Selectable<AnySqliteType> for SqliteSelect {
    fn set_source(&mut self, source: impl Into<SourceRef<AnySqliteType>>, alias: Option<String>) {
        self.from.clear();
        let source_ref = source.into().into_expressive_enum();
        let expr = match (source_ref, alias) {
            (ExpressiveEnum::Scalar(val), None) => {
                let Some(name) = val.try_get::<String>().filter(|s| !s.is_empty()) else {
                    return;
                };
                ident(name).expr()
            }
            (ExpressiveEnum::Scalar(val), Some(alias)) => {
                let Some(name) = val.try_get::<String>().filter(|s| !s.is_empty()) else {
                    return;
                };
                ident(name).with_alias(alias).expr()
            }
            (ExpressiveEnum::Nested(expr), None) => expr,
            (ExpressiveEnum::Nested(expr), Some(alias)) => {
                expr_any!("{} AS {}", (expr), (ident(alias)))
            }
            _ => return,
        };
        self.from.push(expr);
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(ident(field).expr());
    }

    fn add_expression(
        &mut self,
        expression: impl Expressive<AnySqliteType>,
        alias: Option<String>,
    ) {
        let expr = expression.expr();
        let field = match alias {
            Some(a) => expr_any!("{} AS {}", (expr), (ident(a))),
            None => expr,
        };
        self.fields.push(field);
    }

    fn add_where_condition(&mut self, condition: impl Expressive<AnySqliteType>) {
        self.where_conditions.push(condition.expr());
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, expression: impl Expressive<AnySqliteType>, ascending: bool) {
        self.order_by.push((expression.expr(), ascending));
    }

    fn add_group_by(&mut self, expression: impl Expressive<AnySqliteType>) {
        self.group_by.push(expression.expr());
    }

    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>) {
        self.limit = limit;
        self.skip = skip;
    }

    fn clear_fields(&mut self) {
        self.fields.clear();
    }

    fn clear_where_conditions(&mut self) {
        self.where_conditions.clear();
    }

    fn clear_order_by(&mut self) {
        self.order_by.clear();
    }

    fn clear_group_by(&mut self) {
        self.group_by.clear();
    }

    fn has_fields(&self) -> bool {
        !self.fields.is_empty()
    }

    fn has_where_conditions(&self) -> bool {
        !self.where_conditions.is_empty()
    }

    fn has_order_by(&self) -> bool {
        !self.order_by.is_empty()
    }

    fn has_group_by(&self) -> bool {
        !self.group_by.is_empty()
    }

    fn is_distinct(&self) -> bool {
        self.distinct
    }

    fn get_limit(&self) -> Option<i64> {
        self.limit
    }

    fn get_skip(&self) -> Option<i64> {
        self.skip
    }

    fn as_count(&self) -> Expr {
        let mut count_select = self.clone();
        count_select.fields.clear();
        count_select
            .fields
            .push(Expression::new("COUNT(*)", vec![]));
        count_select.order_by.clear();
        count_select.render()
    }

    fn as_sum(&self, column: impl Expressive<AnySqliteType>) -> Expr {
        self.as_aggregate("sum", column)
    }

    fn as_max(&self, column: impl Expressive<AnySqliteType>) -> Expr {
        self.as_aggregate("max", column)
    }

    fn as_min(&self, column: impl Expressive<AnySqliteType>) -> Expr {
        self.as_aggregate("min", column)
    }
}
