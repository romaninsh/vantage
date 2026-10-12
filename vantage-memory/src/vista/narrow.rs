//! Narrowing a `MemoryTableShell`: conditions checked against the declared
//! columns, and their operands conformed to the column's stored form.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_vista::{Condition, FilterOp, SortDirection};

use super::MemoryTableShell;
use crate::MemoryCondition;
use crate::store::ids::{id_forms, supplied_id};

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
            table = self.table.name(),
            column = column
        ))
    }

    /// `condition` as the store evaluates it. Operands are conformed to the
    /// column's [`Affinity`](super::affinity::Affinity), items of a set one by one; a `Like` pattern is
    /// text and stays as given.
    ///
    /// A column with no declared type gives no stored form to conform to,
    /// so (in)equality with an id-like value (an integer, or its canonical
    /// text) matches both forms, the way a foreign key written by hand
    /// meets a text id.
    fn store_condition(&self, condition: &Condition) -> MemoryCondition {
        let column = condition.column.as_str();
        let value = &condition.value;
        let affinity = self.affinity(column);
        let undeclared = self.is_untyped(column);
        match condition.op {
            FilterOp::Like => MemoryCondition::cmp(column, condition.op, value.clone()),
            FilterOp::InSet | FilterOp::NotInSet => {
                let value = match value {
                    CborValue::Array(items) => {
                        CborValue::Array(items.iter().map(|v| affinity.conform(v)).collect())
                    }
                    single => affinity.conform(single),
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
            op => MemoryCondition::cmp(column, op, affinity.conform(value)),
        }
    }

    pub(super) fn narrow_by(&mut self, condition: &Condition) -> Result<()> {
        let column = condition.column.as_str();
        self.check_column(column)?;
        // A dotted path into a map has no flat cell to fill, so writes are
        // left to the set check; a column named with a dot is a flat cell.
        let flat = !column.contains('.') || self.metadata.columns.contains_key(column);
        if condition.op == FilterOp::Eq && flat {
            let value = self.affinity(column).conform(&condition.value);
            self.invariants.insert(column.to_string(), value);
        }
        let stored = self.store_condition(condition);
        self.query.conditions.push(stored);
        Ok(())
    }

    pub(super) fn replace_search(&mut self, text: Option<&str>) {
        self.query.search = text.map(str::to_string);
    }

    /// The store sorts on any field, so any declared column is an order key.
    pub(super) fn replace_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        match order {
            Some((column, dir)) => {
                self.check_column(column)?;
                self.query.order = vec![(column.to_string(), dir)];
            }
            None => self.query.order.clear(),
        }
        Ok(())
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
