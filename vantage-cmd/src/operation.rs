//! `CmdOperation` — ergonomic `.eq()` on columns, building a [`CmdCondition`].

use ciborium::Value as CborValue;
use vantage_table::column::core::{Column, ColumnType};

use crate::condition::CmdCondition;

/// Build conditions from a typed column without going through the
/// expression machinery (command backends filter via CLI flags, not SQL).
pub trait CmdOperation {
    /// `column == value`.
    fn eq(&self, value: impl Into<CborValue>) -> CmdCondition;
}

impl<T: ColumnType> CmdOperation for Column<T> {
    fn eq(&self, value: impl Into<CborValue>) -> CmdCondition {
        CmdCondition::eq(self.name(), value)
    }
}
