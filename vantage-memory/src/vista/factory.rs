//! `MemoryVistaFactory`: builds memory vistas from YAML specs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};
use vantage_vista::{
    Column, ColumnFlag, JoinKey, NoExtras, Reference, ReferenceSugar, Vista, VistaFactory,
    VistaMetadata, VistaSpec,
};

use super::affinity::conform_record;
use super::{MemoryCatalog, MemoryTableShell};
use crate::{MemoryStore, TableDef, seed};

/// The id column a spec gets when it names none.
const DEFAULT_ID_COLUMN: &str = "id";

/// Table-level YAML block: `memory: { indexed: [..], seed: <file> }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryTableExtras {
    #[serde(default)]
    pub memory: MemoryBlock,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MemoryBlock {
    /// Columns given a hash index.
    pub indexed: Vec<String>,
    /// A `.json` / `.yaml` file of rows loaded when the vista is built.
    pub seed: Option<PathBuf>,
}

pub type MemoryVistaSpec = VistaSpec<MemoryTableExtras, NoExtras, NoExtras>;

/// Builds memory vistas from YAML specs, registering each one's metadata in
/// its [`MemoryCatalog`] so traversals between them see declared columns.
pub struct MemoryVistaFactory {
    catalog: MemoryCatalog,
}

impl MemoryVistaFactory {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            catalog: MemoryCatalog::new(store),
        }
    }

    pub fn store(&self) -> &MemoryStore {
        self.catalog.store()
    }
}

impl VistaFactory for MemoryVistaFactory {
    type TableExtras = MemoryTableExtras;
    type ColumnExtras = NoExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: MemoryVistaSpec) -> Result<Vista> {
        if !spec.contained.is_empty() {
            return Err(error!(
                "Contained relations are not supported by vantage-memory",
                vista = spec.name
            ));
        }
        let metadata = metadata_from_spec(&spec)?;
        let id_column = spec.resolve_id_column(DEFAULT_ID_COLUMN);
        let block = spec.driver.memory;
        let table = self.store().define(
            &spec.name,
            TableDef {
                id_column: id_column.clone(),
                indexed: block.indexed.clone(),
                id_prefix: None,
            },
        );
        // The table may predate the spec, keeping its original definition.
        if table.id_column() != id_column {
            return Err(error!(
                "Table already exists with another id column",
                table = spec.name,
                existing = table.id_column(),
                requested = id_column
            ));
        }
        for column in &block.indexed {
            table.add_index(column);
        }
        // Seed only an empty table, so rebuilding the vista neither reverts
        // edits made since nor duplicates rows without an id. Seeded cells
        // take their declared type's form, as written cells do.
        if let Some(path) = &block.seed
            && table.is_empty()
        {
            seed::load_file_with(&table, path, |row| {
                conform_record(&metadata, &id_column, row)
            })?;
        }
        self.catalog.register(spec.name.clone(), metadata.clone());
        let shell = MemoryTableShell::new(table, metadata, self.catalog.clone());
        Ok(Vista::new(spec.name, shell))
    }
}

/// Every stored column is orderable: the store sorts on any field. A column
/// without `type:` is untyped, so its cells are stored as given. A `lazy:`
/// column is computed by the Vista.
fn metadata_from_spec(spec: &MemoryVistaSpec) -> Result<VistaMetadata> {
    let id = spec.resolve_id_column(DEFAULT_ID_COLUMN);
    let mut metadata = VistaMetadata::new().with_id_column(id.clone());
    for (name, col) in &spec.columns {
        if col.expr.is_some() {
            return Err(error!(
                "Server-side `expr:` columns are not supported by vantage-memory",
                column = name
            ));
        }
        let column = match col.lazy_column(name)? {
            Some(computed) => computed,
            None => {
                let column = match &col.col_type {
                    Some(ty) => Column::new(name, ty),
                    None => Column::untyped(name),
                };
                col.flags
                    .iter()
                    .cloned()
                    .chain([ColumnFlag::Orderable])
                    .fold(column, Column::with_flag)
            }
        };
        metadata = metadata.with_column(column);
        // `references: <table>` on a column is a has_one keyed by the column.
        match &col.references {
            Some(ReferenceSugar::Sugar(target)) => {
                metadata = metadata.with_reference(Reference::has_one(
                    name,
                    target,
                    JoinKey::to_target_id(name),
                ));
            }
            Some(ReferenceSugar::Full(r)) => {
                metadata = metadata.with_reference(r.to_reference(name, &id)?);
            }
            None => {}
        }
    }
    for (name, r) in &spec.references {
        metadata = metadata.with_reference(r.to_reference(name, &id)?);
    }
    Ok(metadata)
}
