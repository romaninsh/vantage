//! Roll-up columns: an aggregate over the rows a has_many relation gives for
//! each parent row (an invoice's total over its lines).

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_expressions::{Expression, Expressive, SelectableDataSource, expr_any};
use vantage_types::Entity;

use crate::{
    source::SelectSeed,
    table::{Table, base::ExpressionFn},
    traits::table_source::TableSource,
};

/// The aggregate a roll-up column computes. Over no rows, `Count` and `Sum`
/// give 0; `Min`, `Max` and `Avg` give null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RollupOp {
    Count,
    Sum,
    Min,
    Max,
    Avg,
}

impl<T, E> Table<T, E>
where
    T: SelectableDataSource<T::Value, T::Condition> + TableSource,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String>,
    T::Column<T::AnyType>: Expressive<T::Value>,
    E: Entity<T::Value> + 'static,
{
    /// Add a read-only column `name` holding `op` over `column` of the rows
    /// `relation` gives for each row, narrowed by `condition`. `column` is
    /// required for every op but `Count`.
    ///
    /// The backend renders the expression
    /// ([`TableSource::rollup_expression`]): a correlated subquery by default.
    /// An unknown relation or column, or a backend that refuses roll-ups, is
    /// an error here, not when the table is read.
    pub fn with_rollup(
        mut self,
        name: &str,
        relation: &str,
        op: RollupOp,
        column: Option<&str>,
        condition: Option<T::Condition>,
    ) -> Result<Self> {
        let expr = self
            .data_source()
            .rollup_expression(&self, relation, op, column, condition)?;
        let expr_fn: ExpressionFn<T> = Arc::new(move |_| expr.clone());
        self.expressions.insert(name.to_string(), expr_fn);
        Ok(self)
    }

    /// `op` over `column` of this table's rows as a parenthesised subquery:
    /// [`Self::get_count_query`] and its siblings, with the sum wrapped in
    /// `COALESCE(…, 0)` so no rows read 0.
    pub fn aggregate_query(
        &self,
        op: RollupOp,
        column: Option<&str>,
    ) -> Result<Expression<T::Value>> {
        let column = || {
            let name = column.ok_or_else(|| error!("roll-up needs a column", op = op))?;
            self.columns().get(name).ok_or_else(|| {
                error!(
                    "roll-up column not found",
                    column = name,
                    table = self.table_name()
                )
            })
        };
        Ok(match op {
            RollupOp::Count => self.get_count_query(),
            RollupOp::Sum => expr_any!("COALESCE({}, 0)", (self.get_sum_query(column()?))),
            RollupOp::Min => self.get_min_query(column()?),
            RollupOp::Max => self.get_max_query(column()?),
            RollupOp::Avg => self.get_avg_query(column()?),
        })
    }
}
