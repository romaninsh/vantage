//! The table's name, source, key fields, pagination, invariants and denials.

use indexmap::IndexMap;
use vantage_types::Entity;
use vantage_vista::{Capability, Denials, forbidden};

use super::Table;
use crate::{
    pagination::Pagination,
    traits::{table_source::TableSource, table_source_spec::TableSourceSpec},
};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Use a callback with a builder pattern for configuration
    pub fn with<F>(mut self, func: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        func(&mut self);
        self
    }

    /// Get the table name.
    ///
    /// For a query-sourced table this is its FROM alias.
    pub fn table_name(&self) -> &str {
        self.source.name()
    }

    /// Refuse a by-id write through a conditioned table whose source can't
    /// keep it inside the set (see [`TableSource::can_confine_writes`]). An
    /// unconditioned table writes anywhere.
    pub(crate) fn require_confined_writes(&self, verb: &'static str) -> vantage_core::Result<()> {
        if self.conditions.is_empty() || self.data_source.can_confine_writes() {
            return Ok(());
        }
        Err(vantage_core::error!(
            "this data source can't keep a write inside a narrowed table",
            verb = verb,
            table = self.table_name()
        )
        .mark_unsupported())
    }

    /// The table's source (a name, or a query used as a derived source).
    pub fn source(&self) -> &T::Source {
        &self.source
    }

    /// Override the table name. Used by REST API drivers to swap a
    /// canonical resource path for a per-reference URI template at
    /// traversal time.
    ///
    /// This replaces the source with a name-based one, so it must not be
    /// called on a query-sourced (derived) table.
    pub fn set_table_name(&mut self, name: impl Into<String>) {
        self.source = T::Source::from_name(name.into());
    }

    /// Get the underlying data source
    pub fn data_source(&self) -> &T {
        &self.data_source
    }

    /// A copy of this table over `data_source`, keeping its columns,
    /// conditions, references and hooks.
    pub fn with_data_source(&self, data_source: T) -> Self {
        Self {
            data_source,
            ..self.clone()
        }
    }

    /// Read rows locked until the transaction commits. The datasource renders
    /// it (`SELECT … FOR UPDATE`) on the reads that return rows; a datasource
    /// without locking reads ignores it.
    pub fn set_lock(&mut self) {
        self.lock = true;
    }

    pub fn is_locked(&self) -> bool {
        self.lock
    }

    /// Refuse `capability` on this table, naming `rule` in the `Forbidden`
    /// error. Nothing lifts a denial; the first rule given for a capability
    /// is the one reported.
    pub fn deny(&mut self, capability: Capability, rule: impl Into<String>) {
        self.denials.deny(capability, rule);
    }

    /// Make `column` read-only: a patch that sets it is refused `Forbidden`,
    /// a whole-row replace keeps its stored value.
    pub fn deny_column(&mut self, column: impl Into<String>, rule: impl Into<String>) {
        self.denials.deny_column(column, rule);
    }

    pub fn denials(&self) -> &Denials {
        &self.denials
    }

    /// Refuse `capability` when a rule denies it.
    pub(crate) fn require_allowed(&self, capability: Capability) -> vantage_core::Result<()> {
        match self.denials.rule_for(capability) {
            Some(rule) => forbidden(capability, rule, self.table_name()),
            None => Ok(()),
        }
    }

    /// Get the title field column if set
    pub fn title_field(&self) -> Option<&T::Column<T::AnyType>> {
        self.title_field
            .as_ref()
            .and_then(|name| self.columns.get(name))
    }

    /// Names of columns marked as display titles (set via
    /// [`Self::with_title_column_of`]). These show alongside the id in
    /// list views and on the leading lines of single-record displays.
    pub fn title_fields(&self) -> &[String] {
        &self.title_fields
    }

    /// Get the id field column if set
    pub fn id_field(&self) -> Option<&T::Column<T::AnyType>> {
        self.id_field
            .as_ref()
            .and_then(|name| self.columns.get(name))
    }

    /// Name of the [`id_field`](Self::id_field) column, or `"id"` when there
    /// is none.
    pub fn id_field_name(&self) -> String {
        use crate::traits::column_like::ColumnLike;
        self.id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string())
    }

    /// Mark an already-added column as the id field.
    ///
    /// Use this when the id column has been added via [`Self::add_column`]
    /// (so its type and aliases were chosen explicitly) and you only need
    /// to flag it. [`Self::with_id_column`] is the typed shortcut that
    /// creates the column for you.
    pub fn set_id_field(&mut self, name: impl Into<String>) {
        self.id_field = Some(name.into());
    }

    /// Mark an already-added column as a display title.
    ///
    /// Companion to [`Self::set_id_field`] for spec-driven construction.
    pub fn add_title_field(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.title_fields.contains(&name) {
            self.title_fields.push(name.clone());
        }
        if self.title_field.is_none() {
            self.title_field = Some(name);
        }
    }

    /// Get the current pagination configuration, if set
    pub fn pagination(&self) -> Option<&Pagination> {
        self.pagination.as_ref()
    }

    /// Column values every row in this set must hold: insert and replace fill
    /// a null or absent column and reject a conflicting one; patch never fills,
    /// and rejects a present value that differs (or is null). Registered by
    /// every literal `column = value` condition, [`Self::with_id`] and
    /// has-one/has-many traversal; never by an expression condition.
    pub fn invariants(&self) -> &IndexMap<String, T::Value> {
        &self.invariants
    }

    /// Register an invariant value for `column` on this set.
    ///
    /// A later call for the same column overwrites the earlier invariant.
    pub fn add_invariant(&mut self, column: impl Into<String>, value: T::Value) {
        self.invariants.insert(column.into(), value);
    }

    /// Builder form of [`Self::add_invariant`].
    pub fn with_invariant(mut self, column: impl Into<String>, value: T::Value) -> Self {
        self.add_invariant(column, value);
        self
    }
}
