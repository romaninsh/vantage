//! Condition builders on `Column<T>` and other `Expressive` values:
//! `column.gt(3)`, `column.in_([..])`, `column.descending()`.

use ciborium::Value as CborValue;
use vantage_expressions::Expressive;
use vantage_table::sorting::OrderBy;
use vantage_vista::FilterOp;

use crate::eval::MemoryCondition;
use crate::types::AnyMemoryType;

fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template
}

fn cmp<T>(
    expr: &(impl Expressive<T> + ?Sized),
    op: FilterOp,
    value: impl Into<AnyMemoryType>,
) -> MemoryCondition {
    MemoryCondition::cmp(field_name(expr), op, value.into().into_value())
}

fn set<I, V>(values: I) -> CborValue
where
    I: IntoIterator<Item = V>,
    V: Into<AnyMemoryType>,
{
    CborValue::Array(values.into_iter().map(|v| v.into().into_value()).collect())
}

pub trait MemoryOperation<T>: Expressive<T> {
    fn eq(&self, value: impl Into<AnyMemoryType>) -> MemoryCondition {
        cmp(self, FilterOp::Eq, value)
    }

    fn ne(&self, value: impl Into<AnyMemoryType>) -> MemoryCondition {
        cmp(self, FilterOp::Ne, value)
    }

    fn gt(&self, value: impl Into<AnyMemoryType>) -> MemoryCondition {
        cmp(self, FilterOp::Gt, value)
    }

    fn gte(&self, value: impl Into<AnyMemoryType>) -> MemoryCondition {
        cmp(self, FilterOp::Gte, value)
    }

    fn lt(&self, value: impl Into<AnyMemoryType>) -> MemoryCondition {
        cmp(self, FilterOp::Lt, value)
    }

    fn lte(&self, value: impl Into<AnyMemoryType>) -> MemoryCondition {
        cmp(self, FilterOp::Lte, value)
    }

    fn in_<I, V>(&self, values: I) -> MemoryCondition
    where
        I: IntoIterator<Item = V>,
        V: Into<AnyMemoryType>,
    {
        MemoryCondition::cmp(field_name(self), FilterOp::InSet, set(values))
    }

    fn not_in<I, V>(&self, values: I) -> MemoryCondition
    where
        I: IntoIterator<Item = V>,
        V: Into<AnyMemoryType>,
    {
        MemoryCondition::cmp(field_name(self), FilterOp::NotInSet, set(values))
    }

    /// Case-insensitive SQL-style pattern (`%` any run, `_` one character).
    fn like(&self, pattern: impl Into<String>) -> MemoryCondition {
        MemoryCondition::cmp(field_name(self), FilterOp::Like, pattern.into())
    }

    fn ascending(&self) -> OrderBy<MemoryCondition> {
        OrderBy::ascending(MemoryCondition::Column(field_name(self)))
    }

    fn descending(&self) -> OrderBy<MemoryCondition> {
        OrderBy::descending(MemoryCondition::Column(field_name(self)))
    }
}

impl<T, S: Expressive<T>> MemoryOperation<T> for S {}
