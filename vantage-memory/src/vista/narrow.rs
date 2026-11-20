//! Narrowing a `MemoryTableShell`: conditions checked against the declared
//! columns, their operands conformed to the column's stored form, then added
//! to the table.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::sorting::{OrderBy, SortDirection as TableSort};
use vantage_vista::{Condition, FilterOp, SortDirection};

use super::MemoryTableShell;
use crate::store::ids::{id_forms, supplied_id};
use crate::{AnyMemoryType, MemoryCondition};

impl MemoryTableShell {
    /// A condition on `column` is accepted when the shell declares no
    /// columns, or declares `column` (or, for a dotted path into a nested
    /// map, its first segment), or `column` is the id column.
    fn check_column(&self, column: &str) -> Result<()> {
        let columns = &self.metadata.columns;
        let head = column.split('.').next().unwrap_or(column);
        if columns.is_empty()
            || columns.contains_key(column)
            || columns.contains_key(head)
            || column == self.id_column_name()
        {
            return Ok(());
        }
        Err(error!(
            "Unknown column for condition",
            table = self.handle.name(),
            column = column
        ))
    }

    /// `condition` as the store evaluates it. Operands are conformed to the
    /// column's [`Affinity`](super::affinity::Affinity), set items one by one;
    /// a `Like` pattern stays as given. An operand a temporal column can't
    /// read as its kind is refused.
    ///
    /// A column with no declared type has no stored form, so (in)equality
    /// with an id-like value (an integer, or its canonical text) matches both
    /// forms.
    fn store_condition(&self, condition: &Condition) -> Result<MemoryCondition> {
        let column = condition.column.as_str();
        let value = &condition.value;
        let affinity = self.affinity(column);
        let undeclared = self.is_untyped(column);
        Ok(match condition.op {
            FilterOp::Like => MemoryCondition::cmp(column, condition.op, value.clone()),
            FilterOp::InSet | FilterOp::NotInSet => {
                let value = match value {
                    CborValue::Array(items) => CborValue::Array(
                        items
                            .iter()
                            .map(|v| affinity.conform(column, v))
                            .collect::<Result<_>>()?,
                    ),
                    single => affinity.conform(column, single)?,
                };
                MemoryCondition::cmp(column, condition.op, value)
            }
            // No declared type: `= 1` and `= "1"` both match cells holding
            // either form. Only (in)equality widens; ranges and sets compare
            // the operand as given.
            op @ (FilterOp::Eq | FilterOp::Ne) if undeclared => match id_like(value) {
                Some(id) => {
                    let set = if op == FilterOp::Eq {
                        FilterOp::InSet
                    } else {
                        FilterOp::NotInSet
                    };
                    MemoryCondition::cmp(column, set, CborValue::Array(id_forms(&id)))
                }
                None => MemoryCondition::cmp(column, op, value.clone()),
            },
            op => MemoryCondition::cmp(column, op, affinity.conform(column, value)?),
        })
    }

    /// The table fills and checks an equality condition on writes. A dotted
    /// path into a map has no flat cell to fill, so it goes in as a
    /// temporary condition, which the table leaves to the set check; a
    /// column named with a dot is a flat cell.
    pub(super) fn narrow_by(&mut self, condition: &Condition) -> Result<()> {
        let column = condition.column.as_str();
        self.check_column(column)?;
        let stored = self.store_condition(condition)?;
        let flat = !column.contains('.') || self.metadata.columns.contains_key(column);
        if !flat {
            self.table.temp_add_condition(stored);
            return Ok(());
        }
        self.table.add_condition(stored);
        // A widened equality is an `InSet`, which the table doesn't fill; a
        // roll-up is computed, so nothing is written to it.
        let rollup = self
            .table
            .data_source()
            .is_rollup(self.table.table_name(), column);
        if condition.op == FilterOp::Eq && !rollup {
            let value = self.affinity(column).conform(column, &condition.value)?;
            self.table
                .add_invariant(column, AnyMemoryType::untyped(value));
        }
        Ok(())
    }

    /// The search reads the metadata's
    /// [`search_columns`](vantage_vista::VistaMetadata::search_columns);
    /// metadata that declares no columns searches every cell.
    pub(super) fn replace_search(&mut self, text: Option<&str>) -> Result<()> {
        let condition = text.map(|text| MemoryCondition::Search {
            text: text.to_string(),
            columns: self
                .metadata
                .search_columns()
                .into_iter()
                .map(str::to_string)
                .collect(),
        });
        self.search = self.table.replace_temp_condition(self.search, condition)?;
        Ok(())
    }

    /// The store sorts on any field, so any declared column is an order key.
    pub(super) fn replace_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        let order = order
            .map(|(column, dir)| self.order_by(column, dir))
            .transpose()?;
        self.table.clear_orders();
        if let Some(order) = order {
            self.table.add_order(order);
        }
        Ok(())
    }

    /// A tie-breaker after the orders already set: the store sorts by every
    /// key in turn.
    pub(super) fn push_order(&mut self, column: &str, dir: SortDirection) -> Result<()> {
        let order = self.order_by(column, dir)?;
        self.table.add_order(order);
        Ok(())
    }

    fn order_by(&self, column: &str, dir: SortDirection) -> Result<OrderBy<MemoryCondition>> {
        self.check_column(column)?;
        Ok(OrderBy {
            expression: MemoryCondition::Column(column.to_string()),
            direction: match dir {
                SortDirection::Ascending => TableSort::Ascending,
                SortDirection::Descending => TableSort::Descending,
            },
        })
    }
}

/// The id `value` names when it is an integer or the canonical text of one.
fn id_like(value: &CborValue) -> Option<String> {
    match value {
        CborValue::Integer(_) => supplied_id(Some(value)),
        CborValue::Text(text) if text.parse::<i64>().is_ok_and(|n| n.to_string() == *text) => {
            Some(text.clone())
        }
        _ => None,
    }
}
