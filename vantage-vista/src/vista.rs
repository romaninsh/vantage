use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;

use crate::{
    capabilities::VistaCapabilities, column::Column, metadata::VistaMetadata, reference::Reference,
    source::VistaSource,
};

/// Universal, schema-bearing data handle.
///
/// A `Vista` is produced by a driver factory from a typed `Table<T, E>` or
/// from a YAML schema. Once built, its metadata is read-only — callers
/// observe via accessors and may narrow the data set with `add_condition_eq`.
/// CRUD goes through the `ValueSet` trait family `Table<T, E>` also implements.
pub struct Vista {
    pub(crate) name: String,
    pub(crate) columns: IndexMap<String, Column>,
    pub(crate) references: IndexMap<String, Reference>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) id_column: Option<String>,
    pub(crate) eq_conditions: Vec<(String, CborValue)>,
    pub(crate) source: Box<dyn VistaSource>,
}

impl Vista {
    pub fn new(
        name: impl Into<String>,
        source: Box<dyn VistaSource>,
        metadata: VistaMetadata,
    ) -> Self {
        let capabilities = source.capabilities().clone();
        Self {
            name: name.into(),
            columns: metadata.columns,
            references: metadata.references,
            capabilities,
            id_column: metadata.id_column,
            eq_conditions: Vec::new(),
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

    pub(crate) fn source(&self) -> &dyn VistaSource {
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

    pub fn add_condition_eq(&mut self, field: impl Into<String>, value: CborValue) {
        self.eq_conditions.push((field.into(), value));
    }

    pub fn eq_conditions(&self) -> &[(String, CborValue)] {
        &self.eq_conditions
    }

    // ---- aggregates (not part of ValueSet) ---------------------------------

    pub async fn get_count(&self) -> Result<i64> {
        self.source.get_vista_count(self).await
    }
}
