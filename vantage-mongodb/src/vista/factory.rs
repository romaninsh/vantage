//! `MongoVistaFactory` — YAML/typed-table entry points for building a
//! `Vista` against MongoDB.
//!
//! - this file: the factory, its typed-table entry point and capabilities;
//! - `build`: spec → table lowering;
//! - `columns`: spec columns and their BSON paths;
//! - `assemble`: the table wrapped as a `Vista`, and `impl VistaFactory`;
//! - `ref_keys`: the check that reference keys are stored under their own name.

use std::sync::Arc;

use indexmap::IndexMap;
use vantage_core::Result;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{Capability, SpecResolver, Vista, VistaCapabilities};

use crate::mongodb::MongoDB;
use crate::mongodb::impls::joins::table_joins;
use crate::types::AnyMongoType;
use crate::vista::spec::MongoVistaSpec;

mod assemble;
mod build;
mod columns;
mod ref_keys;

pub(crate) use build::build_mongo_table;
pub(crate) use columns::{build_column, paths_from_table_columns};

#[derive(Clone)]
pub struct MongoVistaFactory {
    pub(crate) mongo: MongoDB,
    /// Resolves a YAML spec by table name, so references can rebuild their
    /// target tables from the live spec.
    pub(crate) resolver: Option<Arc<dyn SpecResolver<MongoVistaSpec>>>,
    /// Explicit live-update choice; `None` follows the detected topology.
    pub(crate) change_streams: Option<bool>,
}

impl MongoVistaFactory {
    pub fn new(mongo: MongoDB) -> Self {
        Self {
            mongo,
            resolver: None,
            change_streams: None,
        }
    }

    /// Force live updates on or off, instead of following whether the
    /// deployment supports change streams. Forcing them on is meant for a
    /// replica set whose topology probe failed (`Topology::Unknown`); a
    /// standalone server still has no change streams, so `watch` then
    /// fails on the server.
    pub fn with_change_streams(mut self, enabled: bool) -> Self {
        self.change_streams = Some(enabled);
        self
    }

    /// Whether vistas built here offer live updates.
    pub(crate) fn live_enabled(&self) -> bool {
        self.change_streams
            .unwrap_or_else(|| self.mongo.supports_change_streams())
    }

    /// Attach a spec resolver, so YAML-declared references rebuild their
    /// target tables from the live spec. A target the resolver does not know
    /// (or every target, without a resolver) is a column-less table over the
    /// collection named by the reference, keyed by `_id`: it reads documents
    /// as stored, without spec columns or nested paths.
    pub fn with_resolver(mut self, resolver: Arc<dyn SpecResolver<MongoVistaSpec>>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// The user resolver first, then a spec with no columns named after the
    /// target.
    fn resolver_with_fallback(&self) -> Arc<dyn SpecResolver<MongoVistaSpec>> {
        let user = self.resolver.clone();
        Arc::new(move |name: &str| {
            user.as_ref()
                .and_then(|user| user.resolve(name))
                .or_else(|| Some(MongoVistaSpec::new(name)))
        })
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through the table's reading path. Column aliases (set
    /// via `Column::with_alias`) seed the path map so renames flow through.
    pub fn from_table<E>(&self, table: Table<MongoDB, E>) -> Result<Vista>
    where
        E: Entity<AnyMongoType> + 'static,
    {
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();
        table_joins(&any_table)?;
        ref_keys::check_table(&any_table)?;
        let column_paths = column_paths(&any_table);
        let metadata = assemble::mongo_metadata(&any_table);
        Ok(self.wrap_with_paths(any_table, column_paths, name, metadata))
    }

    /// Build a `Table<MongoDB, EmptyEntity>` from a spec. Refuses a reference
    /// whose key is stored under another field name (see `ref_keys`).
    ///
    /// The reference-key check runs here rather than in `build_mongo_table`,
    /// which also rebuilds traversal targets: vantage-table swallows a target
    /// build error into a column-less fallback, so the check would turn a bad
    /// target into silently empty reads. Traversal targets are checked from
    /// their columns when wrapped (`from_table`), which errors loudly.
    pub fn table_from_spec(&self, spec: &MongoVistaSpec) -> Result<Table<MongoDB, EmptyEntity>> {
        let resolver = self.resolver_with_fallback();
        ref_keys::check_spec(spec, Some(&resolver))?;
        build_mongo_table(spec, self.mongo.clone(), Some(resolver))
    }
}

/// What a MongoDB vista offers. Every vista reads, narrows, orders, searches,
/// pages by window and by keyset cursor, and traverses its references.
///
/// - `writable`: a stored collection. A pipeline view or a declared server
///   view only reads, so it takes no writes.
/// - `live`: a writable collection on a deployment with change streams
///   (or forced on). Import needs a transaction, which those same
///   deployments serve.
pub(crate) fn mongo_capabilities(writable: bool, live: bool) -> VistaCapabilities {
    let reads = [
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Cursor,
        Capability::Traverse,
    ];
    let writes = [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
    ];
    let streams = [Capability::Subscribe, Capability::Import];
    reads
        .into_iter()
        .chain(writes.into_iter().filter(|_| writable))
        .chain(streams.into_iter().filter(|_| live))
        .collect()
}

/// Each column's BSON path. A table that declares nothing beyond its `_id`
/// key gets no paths, so the vista reads and writes documents as stored
/// instead of projecting them down to `_id`.
fn column_paths(table: &Table<MongoDB, EmptyEntity>) -> IndexMap<String, Vec<String>> {
    let bare = table.columns().len() == 1 && table.columns().contains_key("_id");
    if bare {
        IndexMap::new()
    } else {
        paths_from_table_columns(table)
    }
}
