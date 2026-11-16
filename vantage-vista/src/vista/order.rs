//! Ordering a [`Vista`]: one column with `set_order`, more columns breaking
//! its ties with `add_order`.

use vantage_core::{Result, error};

use super::{Vista, narrow::refusal};
use crate::{
    capabilities::{Capability, unsupported},
    column::ColumnFlag,
    query::VistaQuery,
    sort::SortDirection,
};

impl Vista {
    pub fn with_order(mut self, column: &str, dir: SortDirection) -> Result<Self> {
        self.set_order(Some((column, dir)))?;
        Ok(self)
    }

    /// Order by `column`, replacing any earlier order and its tie-breakers,
    /// or clear it with `None`. Clearing when no order is applied does
    /// nothing.
    ///
    /// A computed column, a shell without [`Order`](Capability::Order), and
    /// a column without [`Orderable`](ColumnFlag::Orderable) are refused, in
    /// that order. Every undeclared column would be refused for lacking the
    /// flag, so it is checked first and is a plain error instead.
    pub fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        let Some((column, dir)) = order else {
            if self.applied.order.is_some() {
                self.edit_shell(|shell| shell.set_order(None))?;
                self.applied.order = None;
                self.applied.then_by.clear();
            }
            return Ok(());
        };
        self.require_orderable(column, "set_order")?;
        self.edit_shell(|shell| shell.set_order(Some((column, dir))))?;
        self.applied.order = Some((column.to_string(), dir));
        self.applied.then_by.clear();
        Ok(())
    }

    /// Break the ties of the current order by `column`; with no order
    /// applied, order by it as [`set_order`](Self::set_order) does. Refused
    /// as `set_order` refuses, and as `Unsupported` by a shell that orders
    /// by one column only.
    pub fn add_order(&mut self, column: &str, dir: SortDirection) -> Result<()> {
        if self.applied.order.is_none() {
            return self.set_order(Some((column, dir)));
        }
        self.require_orderable(column, "add_order")?;
        self.edit_shell(|shell| shell.add_order(column, dir))?;
        self.applied.then_by.push((column.to_string(), dir));
        Ok(())
    }

    /// Apply `query`'s whole order. `Ok(true)` when the shell refused part
    /// of it as `Unsupported` or `Unimplemented`: the caller then sorts by
    /// the whole order itself. Every column is checked as declared first, so
    /// a refusal never hands an unknown column back in the residual.
    pub(super) fn apply_query_order(&mut self, query: &VistaQuery) -> Result<bool> {
        for (column, _) in query.orders() {
            self.require_declared(column, "with_query")?;
        }
        let mut orders = query.orders();
        let Some((column, dir)) = orders.next() else {
            return Ok(false);
        };
        if refusal(self.set_order(Some((column, dir))))? {
            return Ok(true);
        }
        for (column, dir) in orders {
            if refusal(self.add_order(column, dir))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn require_orderable(&self, column: &str, verb: &'static str) -> Result<()> {
        self.require_declared(column, verb)?;
        self.refuse_computed(column, "order")?;
        if !self.capabilities().has(Capability::Order) {
            return unsupported(Capability::Order);
        }
        if !self
            .column(column)
            .is_some_and(|c| c.has_flag(ColumnFlag::Orderable))
        {
            return Err(error!("Column is not orderable", column = column).mark_unsupported());
        }
        Ok(())
    }
}
