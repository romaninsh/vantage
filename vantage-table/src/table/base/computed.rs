//! Columns a table computes rather than stores: server-side expressions,
//! lazy (post-fetch) expressions, imported implicit-reference columns and
//! the columns a `Vista` computes after each read.

use std::sync::Arc;

use vantage_core::Result;
use vantage_expressions::Expression;
use vantage_types::{Entity, Record};
use vantage_vista::forbidden_column;

use super::Table;
use crate::traits::table_source::TableSource;

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Apply lazy expressions to one returned record, in declaration order.
    /// Each callback borrows the record as built so far; the value it
    /// returns is inserted under the expression's name before the next
    /// callback runs. See [`Self::with_lazy_expression`].
    /// Public so driver shells that bypass the `list_values` read path
    /// (e.g. a REST shell's windowed fetch) can still apply lazy columns.
    pub async fn apply_lazy_expressions(
        &self,
        record: &mut vantage_types::Record<T::Value>,
    ) -> vantage_core::Result<()> {
        for (name, f) in &self.lazy_expressions {
            let value = f(record).await?;
            record.insert(name.clone(), value);
        }
        Ok(())
    }

    /// Drop imported implicit-reference columns (`"country.name"`) from a write
    /// payload. They are read-only, expression-backed projections that no
    /// backend can honestly store; a round-trip (read → modify → save) would
    /// otherwise carry them back, and a SCHEMALESS store would create a literal
    /// `country.name` field. Called before invariants on the full-record write
    /// paths (insert, insert-returning-id, replace); `patch_value` instead
    /// rejects imported keys outright, since a partial payload is explicit
    /// intent per key.
    pub(in crate::table) fn strip_imported_columns(
        &self,
        record: &mut vantage_types::Record<T::Value>,
    ) {
        for name in &self.imported_columns {
            record.shift_remove(name);
        }
    }

    /// Drop expression and lazy columns from a full-record write payload, as
    /// [`Self::strip_imported_columns`] does for imported ones: a row read
    /// and saved back carries values the table computes, not stores.
    pub(in crate::table) fn strip_expression_columns(&self, record: &mut Record<T::Value>) {
        record.retain(|name, _| !self.is_expression_column(name));
    }

    /// A patch is explicit intent per key, so a column it can't write is
    /// refused rather than dropped: one a rule made read-only, or an
    /// expression or lazy column.
    pub(in crate::table) fn refuse_read_only_columns(
        &self,
        partial: &Record<T::Value>,
    ) -> Result<()> {
        for name in partial.keys() {
            if let Some(rule) = self.denials.column_rule(name) {
                return forbidden_column(name, rule, self.table_name());
            }
            if self.is_expression_column(name) {
                return forbidden_column(name, "expression column", self.table_name());
            }
        }
        Ok(())
    }

    /// Whether `name` is a server-side expression or lazy column.
    pub(in crate::table) fn is_expression_column(&self, name: &str) -> bool {
        self.expressions.contains_key(name) || self.lazy_expressions.contains_key(name)
    }

    /// Whether `name` is an imported implicit-reference column
    /// (`"country.name"`) — an expression-backed traversal projection that is
    /// read-only and must never be persisted, ordered, or searched as if it
    /// were a physical field.
    pub fn is_imported_column(&self, name: &str) -> bool {
        self.imported_columns.contains(name)
    }

    /// Whether `name` is computed rather than stored: an imported
    /// implicit-reference column, a server-side expression column, or a lazy
    /// (post-fetch) computed column. Driver factories flag such columns
    /// `calculated` in vista metadata so consumers render them read-only.
    pub fn is_calculated_column(&self, name: &str) -> bool {
        self.imported_columns.contains(name) || self.is_expression_column(name)
    }

    /// Register a column the `Vista` computes after each read, positioned
    /// after every column added so far. Driver factories fold these into
    /// `VistaMetadata`. A stored column of the same name (one inherited by a
    /// derived table) is replaced, so the column is never both read and
    /// computed.
    pub fn add_computed_column(&mut self, column: vantage_vista::Column) {
        if let Some((stored_index, _, _)) = self.columns.shift_remove_full(&column.name) {
            let removed = self.position_of_stored(stored_index);
            for (at, _) in self.computed_columns.values_mut() {
                if *at > removed {
                    *at -= 1;
                }
            }
        }
        let at = self.columns.len() + self.computed_columns.len();
        self.computed_columns
            .insert(column.name.clone(), (at, column));
    }

    /// Register `spec`'s column as computed when it declares a `lazy:` script
    /// (see [`vantage_vista::ColumnSpec::lazy_column`]). Returns `false` for a
    /// stored column, leaving the caller to add it.
    pub fn add_lazy_spec_column<C>(
        &mut self,
        spec: &vantage_vista::ColumnSpec<C>,
        name: &str,
    ) -> vantage_core::Result<bool> {
        let Some(column) = spec.lazy_column(name)? else {
            return Ok(false);
        };
        self.add_computed_column(column);
        Ok(true)
    }

    /// Position among all columns of the stored column at `stored_index`:
    /// computed columns occupy their recorded positions, stored columns fill
    /// the remaining slots in order.
    pub(in crate::table) fn position_of_stored(&self, stored_index: usize) -> usize {
        let mut computed: Vec<usize> = self.computed_columns.values().map(|(at, _)| *at).collect();
        computed.sort_unstable();
        let mut at = stored_index;
        for position in computed {
            if position <= at {
                at += 1;
            }
        }
        at
    }

    /// Columns registered via [`Self::add_computed_column`], in order, each
    /// with its position among all columns.
    pub fn computed_columns(&self) -> impl Iterator<Item = (usize, &vantage_vista::Column)> {
        self.computed_columns.values().map(|(at, c)| (*at, c))
    }
}

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
    /// Add a computed expression field using builder pattern.
    ///
    /// The closure receives `&Table<T, E>` and returns an `Expression<T::Value>`.
    /// It is evaluated lazily when `select()` builds the query.
    pub fn with_expression(
        mut self,
        name: &str,
        expr_fn: impl Fn(&Table<T, E>) -> Expression<T::Value> + Send + Sync + 'static,
    ) -> Self
    where
        T: 'static,
        E: 'static,
    {
        // Adapt the caller's `Fn(&Table<T, E>)` into the entity-erased
        // `ExpressionFn<T>` we store, so it survives `into_entity`. The closure
        // never touches entity-typed fields, only name-keyed table state.
        let wrapped: crate::table::base::ExpressionFn<T> =
            Arc::new(move |erased: &Table<T, vantage_types::EmptyEntity>| {
                // SAFETY: layout-identical (E is PhantomData only); shared borrow.
                let typed: &Table<T, E> = unsafe {
                    &*(erased as *const Table<T, vantage_types::EmptyEntity> as *const Table<T, E>)
                };
                expr_fn(typed)
            });
        self.expressions.insert(name.to_string(), wrapped);
        self
    }

    /// Add a lazy computed column using builder pattern.
    ///
    /// Unlike [`Self::with_expression`] (which lowers into the data source's
    /// query), a lazy expression runs in Rust on the record the data source
    /// *returned*. Callbacks apply in declaration order: each borrows the
    /// record as built so far, and the value it returns is inserted under
    /// `name` before the next callback runs — so one expensive fetch (a
    /// file's contents) can feed several cheap derived columns declared
    /// after it.
    ///
    /// The name is also registered as a column, so the computed field shows
    /// up in the table's schema (and in Vista metadata derived from it).
    /// Because the callback's future must not borrow the record, clone what
    /// you need from `record` before going async.
    ///
    /// Lazy expressions run on the `list`/`get` read paths (both raw-record
    /// and entity forms). They never participate in queries — no conditions,
    /// ordering, or pushdown — they exist only on returned data.
    pub fn with_lazy_expression<F, Fut>(mut self, name: &str, f: F) -> Self
    where
        F: Fn(&vantage_types::Record<T::Value>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<T::Value>> + Send + 'static,
    {
        if !self.columns.contains_key(name) {
            let column = self.data_source.create_column::<T::AnyType>(name);
            self.add_column(column);
        }
        self.add_lazy_expression(name, Arc::new(move |record| Box::pin(f(record))));
        self
    }

    /// Register a pre-boxed lazy expression callback under `name`, without
    /// touching the column set. Factories lowering a spec (which declares
    /// its columns separately) use this; application code should prefer
    /// [`Self::with_lazy_expression`].
    pub fn add_lazy_expression(&mut self, name: &str, f: crate::table::base::LazyExpressionFn<T>) {
        self.lazy_expressions.insert(name.to_string(), f);
    }
}
