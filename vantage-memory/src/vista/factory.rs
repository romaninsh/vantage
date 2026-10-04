//! `MemoryVistaFactory`: builds memory vistas from YAML specs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};
use vantage_vista::{
    Column, NoExtras, Reference, ReferenceKind, ReferenceSugar, Vista, VistaFactory, VistaMetadata,
    VistaSpec, flags,
};

use super::{Catalog, MemoryTableShell};
use crate::{MemoryStore, TableDef, seed};

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

pub struct MemoryVistaFactory {
    catalog: Catalog,
}

impl MemoryVistaFactory {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            catalog: Catalog::new(store),
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
        let id_column = id_column(&spec);
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
        // edits made since nor duplicates rows without an id.
        if let Some(path) = &block.seed
            && table.is_empty()
        {
            seed::load_file(&table, path)?;
        }
        self.catalog.register(spec.name.clone(), metadata.clone());
        let shell = MemoryTableShell::new(table, metadata, self.catalog.clone());
        Ok(Vista::new(spec.name, Box::new(shell)))
    }
}

/// The spec's `id_column`, else the column flagged `id`, else `"id"`.
fn id_column(spec: &MemoryVistaSpec) -> String {
    spec.id_column.clone().unwrap_or_else(|| {
        spec.columns
            .iter()
            .find(|(_, c)| c.flags.iter().any(|f| f == flags::ID))
            .map_or_else(|| "id".to_string(), |(name, _)| name.clone())
    })
}

/// Every stored column is orderable: the store sorts on any field. A `lazy:`
/// column is computed by the Vista.
fn metadata_from_spec(spec: &MemoryVistaSpec) -> Result<VistaMetadata> {
    let mut metadata = VistaMetadata::new().with_id_column(id_column(spec));
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
                let mut column = Column::new(name, col.col_type.as_deref().unwrap_or("string"));
                column.flags = col.flags.clone();
                if !column.has_flag(flags::ORDERABLE) {
                    column = column.with_flag(flags::ORDERABLE);
                }
                column
            }
        };
        metadata = metadata.with_column(column);
        match &col.references {
            Some(ReferenceSugar::Sugar(target)) => {
                metadata = metadata.with_reference(Reference::new(
                    name,
                    target,
                    ReferenceKind::HasOne,
                    name,
                ));
            }
            Some(ReferenceSugar::Full(r)) => {
                let fk = r.foreign_key.as_deref().unwrap_or(name);
                metadata = metadata.with_reference(Reference::new(name, &r.table, r.kind, fk));
            }
            None => {}
        }
    }
    for (name, r) in &spec.references {
        if !r.keys.is_empty() {
            return Err(error!(
                "Multi-key references are not supported by vantage-memory",
                reference = name
            ));
        }
        let fk = r.foreign_key.as_deref().unwrap_or(name);
        metadata = metadata.with_reference(Reference::new(name, &r.table, r.kind, fk));
    }
    Ok(metadata)
}
