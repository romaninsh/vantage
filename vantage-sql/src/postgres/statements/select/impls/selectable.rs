use vantage_expressions::traits::selectable::{Selectable, SourceRef};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::postgres::statements::PostgresSelect;
use crate::postgres::types::AnyPostgresType;
use crate::primitives::fx::Fx;

type Expr = Expression<AnyPostgresType>;

impl PostgresSelect {
    pub fn as_aggregate(&self, func: &str, column: impl Expressive<AnyPostgresType>) -> Expr {
        let mut s = self.clone();
        s.clear_fields();
        let agg = Fx::new(func, [column.expr()]);

        // PostgreSQL returns NUMERIC for SUM/AVG on integer types, which sqlx
        // can't decode without bigdecimal. Cast those to BIGINT.
        // MAX/MIN preserve the column's original type — no cast needed.
        let needs_cast = matches!(func, "sum" | "avg");
        if needs_cast {
            let cast = Expression::new(
                "CAST({} AS BIGINT)",
                vec![ExpressiveEnum::Nested(agg.expr())],
            );
            s.add_expression(cast, None);
        } else {
            s.add_expression(agg, None);
        }

        s.clear_order_by();
        s.render()
    }
}

impl Selectable<AnyPostgresType> for PostgresSelect {
    fn set_source(&mut self, source: impl Into<SourceRef<AnyPostgresType>>, alias: Option<String>) {
        self.from.clear();
        let source_ref = source.into().into_expressive_enum();
        let expr = match (source_ref, alias) {
            (ExpressiveEnum::Scalar(val), None) => Expression::new(
                format!("\"{}\"", val.try_get::<String>().unwrap_or_default()),
                vec![],
            ),
            (ExpressiveEnum::Scalar(val), Some(alias)) => Expression::new(
                format!(
                    "\"{}\" AS \"{}\"",
                    val.try_get::<String>().unwrap_or_default(),
                    alias
                ),
                vec![],
            ),
            (ExpressiveEnum::Nested(expr), None) => expr,
            (ExpressiveEnum::Nested(expr), Some(alias)) => Expression::new(
                format!("{{}} AS \"{}\"", alias),
                vec![ExpressiveEnum::Nested(expr)],
            ),
            _ => return,
        };
        self.from.push(expr);
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields
            .push(Expression::new(format!("\"{}\"", field.into()), vec![]));
    }

    fn add_expression(
        &mut self,
        expression: impl Expressive<AnyPostgresType>,
        alias: Option<String>,
    ) {
        let expr = expression.expr();
        let field = match alias {
            Some(a) => Expression::new(
                format!("{{}} AS \"{}\"", a),
                vec![ExpressiveEnum::Nested(expr)],
            ),
            None => expr,
        };
        self.fields.push(field);
    }

    fn add_where_condition(&mut self, condition: impl Expressive<AnyPostgresType>) {
        self.where_conditions.push(condition.expr());
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, expression: impl Expressive<AnyPostgresType>, ascending: bool) {
        self.order_by.push((expression.expr(), ascending));
    }

    fn add_group_by(&mut self, expression: impl Expressive<AnyPostgresType>) {
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

    fn as_sum(&self, column: impl Expressive<AnyPostgresType>) -> Expr {
        self.as_aggregate("sum", column)
    }

    fn as_max(&self, column: impl Expressive<AnyPostgresType>) -> Expr {
        self.as_aggregate("max", column)
    }

    fn as_min(&self, column: impl Expressive<AnyPostgresType>) -> Expr {
        self.as_aggregate("min", column)
    }
}
