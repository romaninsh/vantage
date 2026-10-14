//! redb-specific operation trait that produces `RedbCondition` from columns.
//!
//! Blanket-implemented for any `Expressive<AnyRedbType>` so `Column<T>` and
//! plain identifiers can write `column.eq(value)` / `column.in_(values)`.
//!
//! redb is a key-value store; only `eq` and `in_` are exposed — there is no
//! `gt`/`lt`/`like` since the index can't answer those without a full scan.

use vantage_expressions::Expressive;

use crate::condition::RedbCondition;
use crate::types::AnyRedbType;

fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template
}

/// redb conditions on `Column<T>` and other `Expressive<AnyRedbType>` values.
pub trait RedbOperation<T>: Expressive<T> {
    /// `column == value`
    fn eq(&self, value: impl Into<AnyRedbType>) -> RedbCondition
    where
        Self: Sized,
    {
        RedbCondition::eq(field_name(self), value)
    }

    /// `column IN (values...)`
    fn in_<I, V>(&self, values: I) -> RedbCondition
    where
        Self: Sized,
        I: IntoIterator<Item = V>,
        V: Into<AnyRedbType>,
    {
        RedbCondition::in_(field_name(self), values)
    }
}

impl<T, S: Expressive<T>> RedbOperation<T> for S {}
