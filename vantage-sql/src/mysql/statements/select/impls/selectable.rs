use vantage_expressions::traits::selectable::{Selectable, SourceRef};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::condition::MysqlCondition;
use crate::mysql::statements::MysqlSelect;
use crate::mysql::types::AnyMysqlType;
use crate::primitives::fx::Fx;
use crate::primitives::identifier::ident;

type Expr = Expression<AnyMysqlType>;

impl MysqlSelect {
    pub fn as_aggregate(&self, func: &str, column: impl Expressive<AnyMysqlType>) -> Expr {
        let mut s = self.clone();
        s.clear_fields();
        let agg = Fx::new(func, [column.expr()]);

        let needs_cast = matches!(func, "sum" | "avg");
        if needs_cast {
            let cast = expr_any!("CAST({} AS SIGNED)", (agg));
            s.add_expression(cast);
        } else {
            s.add_expression(agg);
        }

        s.clear_order_by();
        s.render()
    }
}

impl Selectable<AnyMysqlType, MysqlCondition> for MysqlSelect {
    fn add_source(&mut self, source: impl Into<SourceRef<AnyMysqlType>>, alias: Option<String>) {
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

    fn add_expression(&mut self, expression: impl Expressive<AnyMysqlType>) {
        self.fields.push(expression.expr());
    }

    fn add_where_condition(&mut self, condition: impl Into<MysqlCondition>) {
        self.where_conditions.push(condition.into().into_expr());
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(
        &mut self,
        order: impl Into<MysqlCondition>,
        direction: vantage_expressions::Order,
    ) {
        self.order_by.push((order.into().into_expr(), direction));
    }

    fn add_group_by(&mut self, expression: impl Expressive<AnyMysqlType>) {
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

    fn as_field(&self, field: impl Into<String>) -> Expr {
        let mut s = self.clone();
        s.fields.clear();
        s.fields.push(ident(field).expr());
        s.order_by.clear();
        s.render()
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

    fn as_sum(&self, column: impl Expressive<AnyMysqlType>) -> Expr {
        self.as_aggregate("sum", column)
    }

    fn as_max(&self, column: impl Expressive<AnyMysqlType>) -> Expr {
        self.as_aggregate("max", column)
    }

    fn as_min(&self, column: impl Expressive<AnyMysqlType>) -> Expr {
        self.as_aggregate("min", column)
    }
}
