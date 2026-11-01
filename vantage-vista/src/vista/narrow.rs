//! Narrowing a [`Vista`]: consuming `with_*` on a value, `add_*`/`set_*` on
//! one you own. Each clause is applied to the shell first and recorded in
//! the query only once the shell accepts it.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};

use super::Vista;
use crate::{
    capabilities::{Capability, unsupported},
    column::ColumnFlag,
    filter::FilterOp,
    page::NativeCondition,
    query::{Condition, VistaQuery},
    sort::SortDirection,
};

impl Vista {
    pub fn with_condition(
        mut self,
        column: &str,
        op: FilterOp,
        value: impl Into<CborValue>,
    ) -> Result<Self> {
        self.add_condition(column, op, value)?;
        Ok(self)
    }

    /// Narrow to the row whose id column equals `id`.
    pub fn with_id(self, id: impl Into<CborValue>) -> Result<Self> {
        let id_column = self
            .id_column()
            .ok_or_else(|| error!("vista has no id column", table = self.name()))?
            .to_string();
        self.with_condition(&id_column, FilterOp::Eq, id)
    }

    pub fn with_search(mut self, text: impl Into<String>) -> Result<Self> {
        self.set_search(Some(&text.into()))?;
        Ok(self)
    }

    pub fn with_order(mut self, column: &str, dir: SortDirection) -> Result<Self> {
        self.set_order(Some((column, dir)))?;
        Ok(self)
    }

    pub fn with_native_condition(mut self, condition: NativeCondition) -> Result<Self> {
        self.add_native_condition(condition)?;
        Ok(self)
    }

    /// Apply what the shell can take of `query` and return the rest: the
    /// clauses the caller must evaluate itself over the rows it reads. A
    /// caller that needs every clause applied checks the residual is empty.
    ///
    /// A clause goes into the residual only when it is refused as
    /// `Unsupported` or `Unimplemented`: the shell lacks the capability, or
    /// the column is computed. Any other error (an unknown column, a value
    /// the backend can't take) is returned, since evaluating the clause
    /// locally would hide the fault.
    ///
    /// A `VistaQuery` records only the keys of native conditions, not the
    /// conditions, so a query holding any can't be re-applied: that is an
    /// `IncorrectUsage` error. Apply native conditions with
    /// [`with_native_condition`](Self::with_native_condition).
    pub fn with_query(mut self, query: &VistaQuery) -> Result<(Self, VistaQuery)> {
        refuse_natives(query)?;
        let mut refused = VistaQuery::default();
        for c in &query.conditions {
            if refusal(self.add_condition(&c.column, c.op, c.value.clone()))? {
                refused.conditions.push(c.clone());
            }
        }
        if let Some(text) = &query.search
            && refusal(self.set_search(Some(text)))?
        {
            refused.search = Some(text.clone());
        }
        if let Some((column, dir)) = &query.order
            && refusal(self.set_order(Some((column, *dir))))?
        {
            refused.order = Some((column.clone(), *dir));
        }
        Ok((self, refused))
    }

    /// Narrow by `column <op> value`.
    ///
    /// A computed column is refused (the backend holds no values for it),
    /// and an operator other than equality is refused unless the shell
    /// advertises [`FilterOps`](Capability::FilterOps). Before that refusal
    /// the column must be declared (or be a dotted path into a declared
    /// column), so an unknown column is a plain error rather than a refusal
    /// a caller would evaluate locally. Otherwise the shell owns its column
    /// vocabulary and decides: a REST shell takes undeclared path
    /// placeholders, for one.
    pub fn add_condition(
        &mut self,
        column: &str,
        op: FilterOp,
        value: impl Into<CborValue>,
    ) -> Result<()> {
        self.refuse_computed(column, "condition")?;
        if op != FilterOp::Eq && !self.capabilities().has(Capability::FilterOps) {
            self.require_declared(column, "add_condition")?;
            return unsupported(Capability::FilterOps);
        }
        let condition = Condition::new(column, op, value);
        self.edit_shell(|shell| shell.add_condition(&condition))?;
        self.applied.conditions.push(condition);
        Ok(())
    }

    /// Set the quicksearch text, replacing any earlier one, or clear it with
    /// `None`. Setting is refused unless the shell advertises
    /// [`Search`](Capability::Search); clearing when no search is applied
    /// does nothing.
    pub fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        let Some(text) = text else {
            if self.applied.search.is_some() {
                self.edit_shell(|shell| shell.set_search(None))?;
                self.applied.search = None;
            }
            return Ok(());
        };
        if !self.capabilities().has(Capability::Search) {
            return unsupported(Capability::Search);
        }
        self.edit_shell(|shell| shell.set_search(Some(text)))?;
        self.applied.search = Some(text.to_string());
        Ok(())
    }

    /// Order by `column`, replacing any earlier order, or clear it with
    /// `None`. Clearing when no order is applied does nothing.
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
            }
            return Ok(());
        };
        self.require_declared(column, "set_order")?;
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
        self.edit_shell(|shell| shell.set_order(Some((column, dir))))?;
        self.applied.order = Some((column.to_string(), dir));
        Ok(())
    }

    /// Narrow by a driver-native condition; the query records its key.
    pub fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        let key = condition.key().to_string();
        self.edit_shell(|shell| shell.add_native_condition(condition))?;
        self.applied.native.push(key);
        Ok(())
    }

    /// A plain error when the shell declares columns and `column` isn't one
    /// of them. A dotted path (`client.name`) into a declared column is
    /// valid: it names a field nested in that column's value. A shell that
    /// declares no columns can't be checked.
    fn require_declared(&self, column: &str, verb: &'static str) -> Result<()> {
        let root = column.split('.').next().unwrap_or(column);
        if self.columns().is_empty() || self.column(column).is_some() || self.column(root).is_some()
        {
            return Ok(());
        }
        Err(error!(
            "Unknown column",
            column = column,
            verb = verb,
            table = self.name()
        ))
    }
}

fn refuse_natives(query: &VistaQuery) -> Result<()> {
    if query.native.is_empty() {
        return Ok(());
    }
    Err(error!(
        "native conditions can't be re-applied from a VistaQuery",
        native = query.native.join(", ")
    )
    .mark_incorrect_usage())
}

/// `Ok(true)` when `result` is a refusal the caller can evaluate locally,
/// `Ok(false)` when the clause was applied, the error itself otherwise.
fn refusal(result: Result<()>) -> Result<bool> {
    match result {
        Ok(()) => Ok(false),
        Err(e) if e.is_unsupported() || e.is_unimplemented() => Ok(true),
        Err(e) => Err(e),
    }
}
