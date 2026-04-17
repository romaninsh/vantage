use crate::identifier::Identifier;
use crate::sum::{Fx, Sum};
use crate::{AnySurrealType, Expr};
use vantage_expressions::result::QueryResult;
use vantage_expressions::traits::expressive::Expressive;
use vantage_expressions::traits::selectable::{Order, Selectable, SourceRef};

use super::super::SurrealSelect;
use super::super::select_field::SelectField;

impl<T: QueryResult> Selectable<AnySurrealType> for SurrealSelect<T> {
    fn add_source(&mut self, source: impl Into<SourceRef<AnySurrealType>>, _alias: Option<String>) {
        self.add_from(source.into());
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(SelectField::new(Identifier::new(field)));
    }

    fn add_expression(&mut self, expression: impl Expressive<AnySurrealType>) {
        self.fields.push(SelectField::new(expression.expr()));
    }

    fn add_where_condition(&mut self, condition: impl Into<Expr>) {
        self.where_conditions.push(condition.into());
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, order: impl Into<Expr>, direction: Order) {
        self.order_by.push((order.into(), direction.ascending));
    }

    fn add_group_by(&mut self, expression: impl Expressive<AnySurrealType>) {
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
        s.fields.push(SelectField::new(Identifier::new(field)));
        s.order_by.clear();
        s.render()
    }

    fn as_count(&self) -> Expr {
        let mut count_select = self.clone();
        count_select.fields.clear();
        count_select
            .fields
            .push(SelectField::new(Identifier::new("id")));
        count_select.order_by.clear();
        let subquery = count_select.render();
        Fx::new("count", vec![subquery]).into()
    }

    fn as_sum(&self, column: impl Expressive<AnySurrealType>) -> Expr {
        Sum::new(column.expr()).into()
    }

    fn as_max(&self, column: impl Expressive<AnySurrealType>) -> Expr {
        Fx::new("math::max", vec![column.expr()]).into()
    }

    fn as_min(&self, column: impl Expressive<AnySurrealType>) -> Expr {
        Fx::new("math::min", vec![column.expr()]).into()
    }
}
