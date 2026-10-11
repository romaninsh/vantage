//! Narrowing the wrapped table: conditions, quicksearch and order. Each
//! clause is checked before it is pushed, so a refused one leaves the shell's
//! query as it was.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_types::{Entity, Record};
use vantage_vista::{Capability, Condition, FilterOp, SortDirection, unsupported};

use super::dialect::{Shell, SqlShellDialect};
use super::shell::cbor_record;

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

impl<T, E> Shell<T, E>
where
    T: SqlShellDialect,
    E: Entity<T::Value> + 'static,
{
    pub(super) fn apply_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for condition", field = field))?
            .clone();
        let value = &condition.value;
        if let Some(template) = comparison(condition.op) {
            let cell = T::cell(value.clone());
            self.table
                .add_condition(T::compare(&column, template, cell));
            return Ok(());
        }
        match condition.op {
            op @ (FilterOp::InSet | FilterOp::NotInSet) => {
                let CborValue::Array(items) = value else {
                    return Err(error!(
                        "in_set/not_in_set requires an array value",
                        field = field
                    ));
                };
                let values: Vec<T::Value> = items.iter().map(|v| T::cell(v.clone())).collect();
                let negate = op == FilterOp::NotInSet;
                self.table
                    .add_condition(T::in_list(&column, &values, negate));
            }
            // The same `LIKE '%…%' ESCAPE '$'` pattern quicksearch uses, on
            // one column.
            FilterOp::Like => {
                let pattern = crate::like_pattern(value);
                self.table.add_condition(T::like(field, pattern));
            }
            _ => return unsupported(Capability::FilterOps),
        }
        Ok(())
    }

    /// Replace the quicksearch condition; `None` drops it.
    pub(super) fn apply_search(&mut self, text: Option<&str>) {
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        if let Some(text) = text {
            let condition = self
                .table
                .data_source()
                .search_table_condition(&self.table, text);
            self.current_search_handle = Some(self.table.temp_add_condition(condition));
        }
    }

    /// The row just inserted under `id`, read with the quicksearch lifted: a
    /// new row conforms to the set's conditions, but needn't match the
    /// search text. A row that still can't be read (one outside a native
    /// condition, say) is an error.
    pub(super) async fn read_back(&self, id: &str) -> Result<Record<CborValue>> {
        let mut table = self.table.clone();
        if let Some(handle) = self.current_search_handle {
            let _ = table.temp_remove_condition(handle);
        }
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
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for order", field = field));
        }
        self.table.clear_orders();
        let direction = match dir {
            SortDirection::Ascending => TableSortDirection::Ascending,
            SortDirection::Descending => TableSortDirection::Descending,
        };
        self.table.add_order(OrderBy {
            expression: T::order_term(field),
            direction,
        });
        Ok(())
    }
}
