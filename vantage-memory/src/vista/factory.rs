//! `MemoryVistaFactory`: builds memory vistas from YAML specs.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};
use vantage_vista::{
    Column, ColumnFlag, JoinKey, NoExtras, Reference, ReferenceKind, ReferenceSugar, Vista,
    VistaFactory, VistaMetadata, VistaSpec,
};

use super::affinity::{Untyped, conform_record};
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
        let untyped = untyped_columns(&spec, &metadata);
        let id_column = spec.resolve_id_column();
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
                conform_record(&metadata, &untyped, &id_column, row)
            })?;
        }
        self.catalog
            .register_untyped(spec.name.clone(), metadata.clone(), untyped.clone());
        let shell = MemoryTableShell::new(table, metadata, self.catalog.clone())
            .with_untyped_columns(untyped);
        Ok(Vista::new(spec.name, shell))
    }
}

/// Stored columns the spec gives no `type:`. Their metadata says `string`
/// for display, but their cells are stored as given.
fn untyped_columns(spec: &MemoryVistaSpec, metadata: &VistaMetadata) -> Untyped {
    spec.columns
        .iter()
        .filter(|(name, col)| {
            let stored = metadata
                .columns
                .get(*name)
                .is_some_and(|c| !c.is_computed());
            col.col_type.is_none() && stored
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Every stored column is orderable: the store sorts on any field. A `lazy:`
/// column is computed by the Vista.
fn metadata_from_spec(spec: &MemoryVistaSpec) -> Result<VistaMetadata> {
    let id = spec.resolve_id_column();
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
                let column = Column::new(name, col.col_type.as_deref().unwrap_or("string"));
                col.flags
                    .iter()
                    .cloned()
                    .chain([ColumnFlag::Orderable])
                    .fold(column, Column::with_flag)
            }
        };
        metadata = metadata.with_column(column);
        match &col.references {
            Some(ReferenceSugar::Sugar(target)) => {
                metadata =
                    metadata.with_reference(join(name, target, ReferenceKind::HasOne, name, &id)?);
            }
            Some(ReferenceSugar::Full(r)) => {
                let fk = r.foreign_key_or(name);
                metadata = metadata.with_reference(join(name, &r.table, r.kind, &fk, &id)?);
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
        let fk = r.foreign_key_or(name);
        metadata = metadata.with_reference(join(name, &r.table, r.kind, &fk, &id)?);
    }
    Ok(metadata)
}

/// A single-key join: a `has_one` reads the parent's `foreign_key` and
/// constrains the target's id column; a `has_many` reads the parent's id
/// and constrains the target's `foreign_key`.
fn join(
    name: &str,
    target: &str,
    kind: ReferenceKind,
    foreign_key: &str,
    source_id: &str,
) -> Result<Reference> {
    let key = match kind {
        ReferenceKind::HasOne => JoinKey::to_target_id(foreign_key),
        ReferenceKind::HasMany => JoinKey::new(source_id, foreign_key),
        _ => {
            return Err(error!(
                "references: entries must be has_one or has_many",
                reference = name
            ));
        }
    };
    Ok(Reference::new(name, target, kind, vec![key]))
}
