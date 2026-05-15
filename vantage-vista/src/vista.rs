use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;

use crate::{
    capabilities::VistaCapabilities, column::Column, metadata::VistaMetadata, reference::Reference,
    source::TableShell,
};

/// Universal, schema-bearing data handle.
///
/// A `Vista` is produced by a driver factory from a typed `Table<T, E>` or
/// from a YAML schema. Once built, its metadata is read-only — callers
/// observe via accessors and may narrow the data set with `add_condition_eq`,
/// which delegates to the underlying driver's native condition system. Vista
/// itself stores no condition state.
/// CRUD goes through the `ValueSet` trait family `Table<T, E>` also implements.
pub struct Vista {
    pub(crate) name: String,
    pub(crate) columns: IndexMap<String, Column>,
    pub(crate) references: IndexMap<String, Reference>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) id_column: Option<String>,
    pub(crate) source: Box<dyn TableShell>,
}

impl Vista {
    pub fn new(
        name: impl Into<String>,
        source: Box<dyn TableShell>,
        metadata: VistaMetadata,
    ) -> Self {
        let capabilities = source.capabilities().clone();
        Self {
            name: name.into(),
            columns: metadata.columns,
            references: metadata.references,
            capabilities,
            id_column: metadata.id_column,
            source,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Override the vista's display name. Used by spec-driven construction
    /// to expose `spec.name` rather than the underlying file/table name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    /// Short human label for the underlying driver (e.g. `"csv"`, `"sqlite"`,
    /// `"postgres"`, `"mongodb"`).
    pub fn driver(&self) -> &'static str {
        self.source.driver_name()
    }

    pub(crate) fn source(&self) -> &dyn TableShell {
        self.source.as_ref()
    }

    // ---- metadata accessors -----------------------------------------------

    pub fn get_id_column(&self) -> Option<&str> {
        self.id_column.as_deref()
    }

    /// Columns flagged `title` (in declaration order).
    pub fn get_title_columns(&self) -> Vec<&str> {
        self.columns
            .values()
            .filter(|c| c.is_title())
            .map(|c| c.name.as_str())
            .collect()
    }

    pub fn get_column_names(&self) -> Vec<&str> {
        self.columns.keys().map(String::as_str).collect()
    }

    pub fn get_column(&self, name: &str) -> Option<&Column> {
        self.columns.get(name)
    }

    pub fn get_references(&self) -> Vec<&str> {
        self.references.keys().map(String::as_str).collect()
    }

    pub fn get_reference(&self, name: &str) -> Option<&Reference> {
        self.references.get(name)
    }

    // ---- conditions --------------------------------------------------------

    /// Narrow the vista to records matching `field == value`. Delegates to the
    /// underlying driver, which translates the value into its native condition
    /// type (BSON document for Mongo, `Expression` for CSV/SQL, …) and applies
    /// it to the wrapped table.
    ///
    /// Returns `Err` if the field is unknown to the driver or the value cannot
    /// be translated into the driver's condition vocabulary.
    pub fn add_condition_eq(&mut self, field: impl Into<String>, value: CborValue) -> Result<()> {
        self.source.add_eq_condition(&field.into(), &value)
    }

    // ---- aggregates (not part of ValueSet) ---------------------------------

    pub async fn get_count(&self) -> Result<i64> {
        self.source.get_vista_count(self).await
    }

    /// Push a driver-native condition into the wrapped table. The
    /// boxed value must match the driver's `T::Condition`. Used by
    /// YAML-driven relation factories that need to inject deferred
    /// FK eq-conditions (i.e. conditions whose value is resolved at
    /// fetch time by reading a parent record).
    pub fn add_raw_condition<C: Send + Sync + 'static>(&mut self, condition: C) -> Result<()> {
        self.source.add_raw_condition(Box::new(condition))
    }

    // ---- references --------------------------------------------------------

    /// Traverse a named reference and return the related `Vista`.
    ///
    /// The driver does the work: it consults its wrapped typed table's
    /// reference machinery (set up via `with_one` / `with_many`),
    /// applies the join condition, and wraps the resulting table in a
    /// new `Vista` so callers stay on the universal surface. Returns
    /// `Err` for drivers that don't implement references and for
    /// unknown relation names.
    pub fn get_ref(&self, relation: &str) -> Result<Vista> {
        self.source.get_ref(relation)
    }
}
