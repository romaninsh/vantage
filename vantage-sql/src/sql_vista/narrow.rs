//! Narrowing the wrapped table: conditions, quicksearch and order. Each
//! clause is checked before it is pushed, so a refused one leaves the shell's
//! query as it was.

use std::any::type_name;

use chrono::NaiveDate;
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::expr_any;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{Entity, Record, temporal};
use vantage_vista::{
    Capability, Condition, FilterOp, SortDirection, condition_column_error, unsupported,
};

use super::dialect::{Shell, SqlShellDialect};
use super::shell::cbor_record;
use crate::declared::{is_absent, is_temporal, without_option};
use crate::where_subject;

/// The binary comparison template of `op`, `None` for the other operators.
fn comparison(op: FilterOp) -> Option<&'static str> {
    Some(match op {
        FilterOp::Eq => "{} = {}",
        FilterOp::Ne => "{} != {}",
        FilterOp::Gt => "{} > {}",
        FilterOp::Gte => "{} >= {}",
        FilterOp::Lt => "{} < {}",
        FilterOp::Lte => "{} <= {}",
        _ => return None,
    })
}

fn is_op(op: FilterOp) -> bool {
    matches!(op, FilterOp::Is | FilterOp::IsNot)
}

/// `value` as a carrier date when it is a tagged instant: the instant's UTC
/// date. Text is returned as it is, for the column's coercion to judge, so
/// datetime text is refused rather than truncated.
fn instant_as_date(value: &CborValue) -> CborValue {
    match (value, temporal::as_instant(value)) {
        (CborValue::Tag(..), Some(at)) => temporal::date(&at.date_naive()),
        _ => value.clone(),
    }
}

impl<T, E> Shell<T, E>
where
    T: SqlShellDialect,
    E: Entity<T::Value> + 'static,
{
    /// `value` as `field` compares it. On a date, time, datetime or timestamp
    /// column that is the value a write would store
    /// (`TableSource::coerce_column_value`), so text in any form a write
    /// takes compares as the stored form does, and a value that doesn't fit
    /// is the write's error. No value (null, `""`) and every other column
    /// compare the value as given.
    ///
    /// An instant against a date column compares as its UTC date, which a
    /// write would refuse: a filter such as "due within 7 days of now" has no
    /// date of its own to give.
    fn operand(&self, field: &str, value: &CborValue) -> Result<T::Value> {
        let Some(declared) = self
            .table
            .columns()
            .get(field)
            .map(|column| column.get_type())
            .filter(|declared| is_temporal(declared))
        else {
            return Ok(T::cell(value.clone()));
        };
        if is_absent(value) {
            return Ok(T::cell(value.clone()));
        }
        let value = if without_option(declared) == type_name::<NaiveDate>() {
            instant_as_date(value)
        } else {
            value.clone()
        };
        self.table
            .data_source()
            .coerce_column_value(&self.table, field, T::cell(value))
    }

    /// `condition` with the operand [`apply_condition`](Self::apply_condition)
    /// sends, so rows held in memory are judged as the database judged them.
    /// `Like` matches the value's own text, and a condition `apply_condition`
    /// refuses stays as given.
    pub(super) fn typed_condition(&self, condition: &Condition) -> Condition {
        let field = condition.column.as_str();
        let typed = match (&condition.op, &condition.value) {
            (FilterOp::InSet | FilterOp::NotInSet, CborValue::Array(items)) => items
                .iter()
                .map(|v| self.operand(field, v).map(T::cbor))
                .collect::<Result<_>>()
                .map(CborValue::Array),
            (op, value) if comparison(*op).is_some() || is_op(*op) => {
                self.operand(field, value).map(T::cbor)
            }
            _ => return condition.clone(),
        };
        let mut condition = condition.clone();
        if let Ok(value) = typed {
            condition.value = value;
        }
        condition
    }

    /// A dotted column is refused with [`condition_column_error`] rather than
    /// compared as a column name, so a caller can evaluate it over the rows
    /// it reads: SQL evaluates neither a nested path into a column's value
    /// (`address.city`) nor a condition on an imported column (`author.name`),
    /// which exists only as a projection alias. A path under an unknown
    /// column is a plain error.
    pub(super) fn apply_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if field.contains('.') {
            return Err(condition_column_error(&self.metadata, field));
        }
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for condition", field = field))?
            .clone();
        let value = &condition.value;
        let subject = where_subject(&self.table, field, T::column_expr(&column))?;
        if let Some(template) = comparison(condition.op) {
            let cell = self.operand(field, value)?;
            self.table
                .add_condition(T::compare(subject, template, cell));
            return Ok(());
        }
        match condition.op {
            op @ (FilterOp::Is | FilterOp::IsNot) => {
                let cell = match value {
                    CborValue::Null => None,
                    value => Some(self.operand(field, value)?),
                };
                self.table
                    .add_condition(T::is(subject, cell, op == FilterOp::IsNot));
            }
            op @ (FilterOp::InSet | FilterOp::NotInSet) => {
                let CborValue::Array(items) = value else {
                    return Err(error!(
                        "in_set/not_in_set requires an array value",
                        field = field
                    ));
                };
                let values = items
                    .iter()
                    .map(|v| self.operand(field, v))
                    .collect::<Result<Vec<T::Value>>>()?;
                let negate = op == FilterOp::NotInSet;
                self.table
                    .add_condition(T::in_list(subject, &values, negate));
            }
            // The same `LIKE '%…%' ESCAPE '$'` pattern quicksearch uses, on
            // one column.
            FilterOp::Like => {
                let pattern = crate::like_pattern(value);
                let subject = where_subject(&self.table, field, T::quoted(field))?;
                self.table.add_condition(T::like(subject, pattern));
            }
            _ => return unsupported(Capability::FilterOps),
        }
        Ok(())
    }

    /// Replace the quicksearch condition; `None` drops it.
    pub(super) fn apply_search(&mut self, text: Option<&str>) -> Result<()> {
        let condition = text.map(|text| {
            self.table
                .data_source()
                .search_table_condition(&self.table, text)
        });
        self.current_search_handle = self
            .table
            .replace_temp_condition(self.current_search_handle, condition)?;
        Ok(())
    }

    /// The row just inserted under `id`, read with the quicksearch lifted: a
    /// new row conforms to the set's conditions, but needn't match the
    /// search text. A row that still can't be read (one outside a native
    /// condition, say) is an error.
    pub(super) async fn read_back(&self, id: &str) -> Result<Record<CborValue>> {
        let mut table = self.table.clone();
        table.replace_temp_condition(self.current_search_handle, None)?;
        table
            .get_value(id)
            .await?
            .map(cbor_record::<T>)
            .ok_or_else(|| error!("inserted row can't be read back through the set", id = id))
    }

    /// Replace the order; `None` drops it.
    pub(super) fn apply_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        let Some((field, dir)) = order else {
            self.table.clear_orders();
            return Ok(());
        };
        self.check_order_column(field)?;
        self.table.clear_orders();
        self.push_order(field, dir)
    }

    /// A tie-breaker after the orders already set: `ORDER BY a, b`. An
    /// expression column orders by its expression, as a condition compares
    /// it: the SELECT alias isn't there when the projection leaves the
    /// column out.
    pub(super) fn push_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        self.check_order_column(field)?;
        let direction = match dir {
            SortDirection::Ascending => TableSortDirection::Ascending,
            SortDirection::Descending => TableSortDirection::Descending,
        };
        let expression = match self.table.try_expression_column(field)? {
            Some(expr) => self
                .table
                .data_source()
                .expression_condition(expr_any!("({})", (expr)))?,
            None => T::order_term(field),
        };
        self.table.add_order(OrderBy {
            expression,
            direction,
        });
        Ok(())
    }

    fn check_order_column(&self, field: &str) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for order", field = field));
        }
        Ok(())
    }
}
