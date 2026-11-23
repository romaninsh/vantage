//! Roll-up columns: an aggregate over the rows a has_many relation gives for
//! each parent row (an invoice's total over its lines).

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_expressions::{Expression, Expressive, Selectable, SelectableDataSource, expr_any};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::ReferenceKind;

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

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
    /// The table roll-up column `name` aggregates: `relation`'s target,
    /// built now. `column` is the target column it reads. Every backend's
    /// [`TableSource::rollup_expression`] starts here, so the refusals are
    /// the same on each.
    pub fn rollup_target(
        &self,
        name: &str,
        relation: &str,
        column: Option<&str>,
    ) -> Result<Table<T, EmptyEntity>> {
        let target = self.get_ref_target_erased(relation)?;
        // Over its own table the column would read itself, without end.
        if target.table_name() == self.table_name() && column == Some(name) {
            return Err(error!(
                "a roll-up column can't aggregate itself",
                column = name,
                relation = relation,
                table = self.table_name()
            ));
        }
        // A window would aggregate one page of children, or, past the first
        // page, skip the aggregate's only row.
        if target.pagination().is_some() {
            return Err(error!(
                "a roll-up's relation target can't have pagination",
                relation = relation,
                table = self.table_name()
            ));
        }
        Ok(target)
    }
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
    /// The column is an expression column like any other
    /// ([`Self::with_try_expression`]): the backend renders it
    /// ([`TableSource::rollup_expression`], a correlated subquery by default)
    /// each time a query that includes the column is built. The relation's
    /// target is built then, not here, so adding a roll-up builds no table.
    ///
    /// Errors here: an unknown relation, one that isn't a has_many or is
    /// joined by more than one key, an op without the column it needs.
    ///
    /// Errors when a query includes the column ([`Self::try_select`]), and
    /// from [`Self::check_expressions`]: a target that fails to build or has
    /// a window, an unknown or computed child column, a column that would
    /// aggregate itself, a backend that refuses roll-ups.
    pub fn with_rollup(
        mut self,
        name: &str,
        relation: &str,
        op: RollupOp,
        column: Option<&str>,
        condition: Option<T::Condition>,
    ) -> Result<Self> {
        if self.ref_cardinality(relation)? != ReferenceKind::HasMany {
            return Err(error!(
                "a roll-up needs a has_many relation",
                relation = relation,
                table = self.table_name()
            ));
        }
        // The aggregate correlates one key; the others would be dropped.
        if self.ref_keys(relation)?.len() > 1 {
            return Err(error!(
                "a roll-up's relation can't be joined by more than one key",
                relation = relation,
                table = self.table_name()
            ));
        }
        if op != RollupOp::Count && column.is_none() {
            return Err(error!("roll-up needs a column", op = op));
        }
        let rollup = name.to_string();
        let relation = relation.to_string();
        let column = column.map(str::to_string);
        let expr_fn: ExpressionFn<T> = Arc::new(move |parent| {
            parent.data_source().rollup_expression(
                parent,
                &rollup,
                &relation,
                op,
                column.as_deref(),
                condition.clone(),
            )
        });
        self.expressions.insert(name.to_string(), expr_fn);
        Ok(self)
    }

    /// `op` over `column` of this table's rows as a parenthesised subquery,
    /// with the sum wrapped in `COALESCE(…, 0)` so no rows read 0. An
    /// expression column is aggregated by its expression.
    pub fn aggregate_query(
        &self,
        op: RollupOp,
        column: Option<&str>,
    ) -> Result<Expression<T::Value>> {
        let value = || {
            let name = column.ok_or_else(|| error!("roll-up needs a column", op = op))?;
            self.try_get_column_expr(name)?.ok_or_else(|| {
                error!(
                    "roll-up column not found",
                    column = name,
                    table = self.table_name()
                )
            })
        };
        let rows = || {
            let mut select = self.select_empty();
            select.clear_order_by();
            select
        };
        Ok(match op {
            RollupOp::Count => self.get_count_query(),
            RollupOp::Sum => expr_any!("COALESCE(({}), 0)", (rows().as_sum(value()?))),
            RollupOp::Min => expr_any!("({})", (rows().as_min(value()?))),
            RollupOp::Max => expr_any!("({})", (rows().as_max(value()?))),
            RollupOp::Avg => expr_any!("({})", (rows().as_avg(value()?))),
        })
    }
}
