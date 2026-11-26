//! `MemoryVistaFactory`: typed-table and YAML entry points, plus the
//! `VistaFactory` impl.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use vantage_core::Result;
use vantage_table::table::{BuildEnv, Orderable, Table, TableInjector, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    BuildContext, ColumnFlag, NoExtras, SpecResolver, Vista, VistaFactory, VistaMetadata, VistaSpec,
};

use super::MemoryTableShell;
use super::affinity::declared_types;
use super::spec_table::{build_memory_table, metadata_from_spec};
use crate::{AnyMemoryType, MemoryDB, MemoryStore, MemoryTableHandle};

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

pub(super) type Resolver = Arc<dyn SpecResolver<MemoryVistaSpec>>;

/// Builds memory vistas over typed tables. A reference target is rebuilt
/// from its spec when traversed: the attached resolver's (a miss is an
/// error), or without a resolver the spec of a table this factory built,
/// else, for a table the store already holds, one declaring only its id
/// column.
pub struct MemoryVistaFactory {
    store: MemoryStore,
    resolver: Option<Resolver>,
    injector: Option<TableInjector<MemoryDB>>,
    built: Arc<RwLock<HashMap<String, MemoryVistaSpec>>>,
}

impl MemoryVistaFactory {
    pub fn new(store: MemoryStore) -> Self {
        Self {
            store,
            resolver: None,
            injector: None,
            built: Arc::default(),
        }
    }

    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    /// Attach a build injector, run on every table built from a spec: the
    /// root, and each reference target when traversed. A table handed to
    /// [`from_table`](Self::from_table) is the caller's and is wrapped as it
    /// is.
    pub fn with_injector(mut self, injector: TableInjector<MemoryDB>) -> Self {
        self.injector = Some(injector);
        self
    }

    /// Attach a spec resolver, which finds a spec by table name so a
    /// reference rebuilds its target from the live spec.
    pub fn with_resolver(mut self, resolver: Resolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Wrap a typed table as a Vista, with metadata read off the table. A
    /// table without an id field takes the store table's id column.
    pub fn from_table<E>(&self, table: Table<MemoryDB, E>) -> Result<Vista>
    where
        E: Entity<AnyMemoryType> + 'static,
    {
        table_vista(table.into_entity::<EmptyEntity>(), self.specs())
    }

    /// Build the table `spec` describes, run through the injector. Building
    /// defines the store table, with the spec's indexes, and seeds it when
    /// empty. A reference to `spec.name` is then rebuilt from `spec` when
    /// no resolver is attached.
    pub fn table_from_spec(&self, spec: &MemoryVistaSpec) -> Result<Table<MemoryDB, EmptyEntity>> {
        self.table_from_spec_in(spec, &BuildContext::default())
    }

    fn table_from_spec_in(
        &self,
        spec: &MemoryVistaSpec,
        cx: &BuildContext,
    ) -> Result<Table<MemoryDB, EmptyEntity>> {
        let env = BuildEnv::root(&spec.name, Some(self.specs()), self.injector.clone())
            .with_context(cx.clone());
        let table = build_memory_table(spec, MemoryDB::from_store(self.store.clone()), &env)?;
        self.built.write().insert(spec.name.clone(), spec.clone());
        Ok(table)
    }

    /// An attached resolver's answer is final. Without one, a spec this
    /// factory built, else one declaring only the id column of a table the
    /// store holds.
    fn specs(&self) -> Resolver {
        if let Some(resolver) = &self.resolver {
            return resolver.clone();
        }
        let built = self.built.clone();
        let store = self.store.clone();
        Arc::new(move |name: &str| {
            built.read().get(name).cloned().or_else(|| {
                let table = store.existing_table(name)?;
                let mut spec = MemoryVistaSpec::new(name);
                spec.id_column = Some(table.id_column().to_string());
                Some(spec)
            })
        })
    }
}

/// The Vista over a table built from `spec`: the spec's metadata, so
/// declared types reach the Vista as written, with the denials the injector
/// gave the table and the columns it made computed (roll-ups) flagged
/// `Calculated`.
pub(super) fn spec_vista(
    table: Table<MemoryDB, EmptyEntity>,
    spec: &MemoryVistaSpec,
    specs: Resolver,
) -> Result<Vista> {
    let mut metadata = metadata_from_spec(spec, &table)?.with_denials(table.denials().clone());
    for column in metadata.columns.values_mut() {
        if table.is_calculated_column(&column.name) {
            *column = column.clone().with_flag(ColumnFlag::Calculated);
        }
    }
    let handle = table.data_source().live_table(&table)?;
    Ok(wrap(table, handle, metadata, specs))
}

/// The Vista over `table`, with metadata read off it.
pub(super) fn table_vista(table: Table<MemoryDB, EmptyEntity>, specs: Resolver) -> Result<Vista> {
    let metadata = declared_types(table.vista_metadata(VistaMetadataOptions {
        orderable: Orderable::All,
        column_flags: true,
        id_flag: true,
        references: true,
        contained: true,
        ..VistaMetadataOptions::default()
    }));
    let handle = table.data_source().live_table(&table)?;
    Ok(wrap(table, handle, metadata, specs))
}

/// The shell over `table`. Metadata without an id column takes the store
/// table's.
fn wrap(
    table: Table<MemoryDB, EmptyEntity>,
    handle: MemoryTableHandle,
    metadata: VistaMetadata,
    specs: Resolver,
) -> Vista {
    let metadata = match metadata.id_column {
        Some(_) => metadata,
        None => metadata.with_id_column(handle.id_column().to_string()),
    };
    let name = table.table_name().to_string();
    Vista::new(name, MemoryTableShell::new(table, handle, metadata, specs))
}

impl VistaFactory for MemoryVistaFactory {
    type TableExtras = MemoryTableExtras;
    type ColumnExtras = NoExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: MemoryVistaSpec) -> Result<Vista> {
        self.build_from_spec_in(spec, &BuildContext::default())
    }

    fn build_from_spec_in(&self, spec: MemoryVistaSpec, cx: &BuildContext) -> Result<Vista> {
        let table = self.table_from_spec_in(&spec, cx)?;
        // Only here, on the table the caller asked for: a reference target
        // checking its own would build this one again.
        table.data_source().check_rollups(&table)?;
        spec_vista(table, &spec, self.specs())
    }
}
