use indexmap::IndexMap;

use crate::{column::Column, reference::Reference};

/// Schema a `Vista` is built around.
///
/// Driver factories produce one of these by inspecting a typed
/// `Table<DriverSource, E>`; the universal YAML loader produces one by
/// parsing config. End-user code typically doesn't construct this directly.
///
/// Title columns are derived from column flags at runtime — see
/// `Vista::get_title_columns`.
#[derive(Debug, Clone, Default)]
pub struct VistaMetadata {
    pub columns: IndexMap<String, Column>,
    pub references: IndexMap<String, Reference>,
    pub id_column: Option<String>,
}

impl VistaMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_column(mut self, column: Column) -> Self {
        self.columns.insert(column.name.clone(), column);
        self
    }

    pub fn with_reference(mut self, reference: Reference) -> Self {
        self.references.insert(reference.name.clone(), reference);
        self
    }

    pub fn with_id_column(mut self, name: impl Into<String>) -> Self {
        self.id_column = Some(name.into());
        self
    }
}
