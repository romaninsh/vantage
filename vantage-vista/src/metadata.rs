use indexmap::IndexMap;

use crate::{
    column::{Column, ColumnFlag},
    id_generator::IdGenerator,
    reference::Reference,
};

/// Schema a `Vista` is built around.
///
/// Driver factories produce one of these by inspecting a typed
/// `Table<DriverSource, E>`; the universal YAML loader produces one by
/// parsing config. Contained relations are references of a contained kind;
/// the schema of their records belongs to the shell that serves them.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct VistaMetadata {
    pub columns: IndexMap<String, Column>,
    pub references: IndexMap<String, Reference>,
    pub id_column: Option<String>,
    /// How writers mint the id of a new record; `None` mints a UUIDv7.
    pub id_generator: Option<IdGenerator>,
}

impl VistaMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_column(mut self, column: Column) -> Self {
        self.columns.insert(column.name.clone(), column);
        self
    }

    /// [`with_column`](Self::with_column) at position `index` (clamped to the
    /// end), shifting later columns along.
    pub(crate) fn with_column_at(mut self, index: usize, column: Column) -> Self {
        let index = index.min(self.columns.len());
        self.columns
            .shift_insert(index, column.name.clone(), column);
        self
    }

    /// Insert each `(position, column)` in order, a column at `position`
    /// (clamped to the end) shifting later columns along. This is the shape
    /// of `Table::computed_columns`.
    pub fn with_columns_at<'a>(
        self,
        columns: impl IntoIterator<Item = (usize, &'a Column)>,
    ) -> Self {
        columns.into_iter().fold(self, |metadata, (at, column)| {
            metadata.with_column_at(at, column.clone())
        })
    }

    pub fn with_reference(mut self, reference: Reference) -> Self {
        self.references.insert(reference.name.clone(), reference);
        self
    }

    pub fn with_id_column(mut self, name: impl Into<String>) -> Self {
        self.id_column = Some(name.into());
        self
    }

    pub fn with_id_generator(mut self, generator: IdGenerator) -> Self {
        self.id_generator = Some(generator);
        self
    }

    /// The columns a quicksearch reads: those flagged `Searchable`, or, when
    /// none is, every column. Dotted paths and imported columns are the
    /// driver's to exclude.
    pub fn search_columns(&self) -> Vec<&str> {
        let flagged: Vec<&str> = self
            .columns
            .values()
            .filter(|column| column.has_flag(ColumnFlag::Searchable))
            .map(|column| column.name.as_str())
            .collect();
        if flagged.is_empty() {
            self.columns.keys().map(String::as_str).collect()
        } else {
            flagged
        }
    }
}
