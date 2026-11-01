//! `SurrealVistaFactory`: the typed-table and YAML entry points and the
//! capabilities a SurrealDB vista advertises.
//!
//! - `build`: spec → table lowering;
//! - `columns`: spec columns and their YAML types;
//! - `references`: table metadata, with a spec's `references:` lowered in;
//! - `scripting`: the Rhai host for `modify:` scripts and scripted references.

mod build;
mod columns;
mod references;
#[cfg(feature = "rhai")]
mod scripting;

use std::sync::Arc;

use vantage_core::Result;
#[cfg(not(feature = "rhai"))]
use vantage_core::error;
use vantage_table::source::SelectSource;
use vantage_table::table::{BuildEnv, Table, TableInjector};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Capability, SpecResolver, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
};

use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;
use crate::vista::source::SurrealTableShell;
use crate::vista::spec::{
    SurrealColumnExtras, SurrealReferenceExtras, SurrealTableExtras, SurrealVistaSpec,
};
use build::build_surreal_table;
use references::{metadata_from_spec, metadata_from_table};

#[derive(Clone)]
pub struct SurrealVistaFactory {
    db: SurrealDB,
    /// Resolves a YAML spec by table name, so references and `base:` can
    /// rebuild their tables from the live spec.
    resolver: Option<Arc<dyn SpecResolver<SurrealVistaSpec>>>,
    injector: Option<TableInjector<SurrealDB>>,
}

impl SurrealVistaFactory {
    pub fn new(db: SurrealDB) -> Self {
        Self {
            db,
            resolver: None,
            injector: None,
        }
    }

    /// Attach a build injector, run on every table built from a spec: the
    /// root, each reference target when traversed, each `base:` table, and
    /// the tables a script's `table(name)` builds. A table handed to
    /// [`from_table`](Self::from_table) is the caller's and is wrapped as it
    /// is.
    pub fn with_injector(mut self, injector: TableInjector<SurrealDB>) -> Self {
        self.injector = Some(injector);
        self
    }

    /// Attach a spec resolver. Required for YAML-declared references to
    /// resolve their target tables — without it, traversals yield a
    /// column-less target Table and the next query fails loudly.
    pub fn with_resolver(mut self, resolver: Arc<dyn SpecResolver<SurrealVistaSpec>>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path. The original entity
    /// type is preserved so `with_expression` closures remain typecheckable.
    /// A query-sourced table (a `rhai:` or `base:` reference target, say) is
    /// read-only here too.
    pub fn from_table<E>(&self, table: Table<SurrealDB, E>) -> Result<Vista>
    where
        E: Entity<AnySurrealType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = metadata_from_table(&table);
        let read_only = matches!(table.source(), SelectSource::Query { .. });
        Ok(self.wrap(table, name, metadata, read_only))
    }

    /// The one place a vista is made, so `from_table` and `build_from_spec`
    /// agree on capabilities. `read_only` (a `rhai:` or `base:` source)
    /// clears the write capabilities.
    fn wrap<E>(
        &self,
        table: Table<SurrealDB, E>,
        name: String,
        metadata: VistaMetadata,
        read_only: bool,
    ) -> Vista
    where
        E: Entity<AnySurrealType> + 'static,
    {
        let shell = SurrealTableShell::new(
            table,
            surreal_capabilities(read_only),
            metadata,
            self.clone(),
        );
        Vista::new(name, shell)
    }

    /// Build a `Table<SurrealDB, EmptyEntity>` from a spec.
    pub fn table_from_spec(
        &self,
        spec: &SurrealVistaSpec,
    ) -> Result<Table<SurrealDB, EmptyEntity>> {
        let env = BuildEnv::root(&spec.name, self.resolver.clone(), self.injector.clone());
        build_surreal_table(spec, self.db.clone(), &env)
    }
}

impl VistaFactory for SurrealVistaFactory {
    type TableExtras = SurrealTableExtras;
    type ColumnExtras = SurrealColumnExtras;
    type ReferenceExtras = SurrealReferenceExtras;

    fn build_from_spec(&self, spec: SurrealVistaSpec) -> Result<Vista> {
        let read_only = spec
            .driver
            .surreal
            .as_ref()
            .is_some_and(|m| m.rhai.is_some() || m.base.is_some());
        let table = self.table_from_spec(&spec)?;
        let metadata = metadata_from_spec(&table, &spec)?;
        let modify = spec.driver.surreal.as_ref().and_then(|b| b.modify.clone());
        #[cfg(not(feature = "rhai"))]
        if modify.is_some() {
            return Err(error!(
                "vista declares a `modify:` script but vantage-surrealdb was built without the `rhai` feature",
                vista = spec.name.as_str()
            ));
        }
        #[cfg_attr(not(feature = "rhai"), allow(unused_mut))]
        let mut vista = self.wrap(table, spec.name, metadata, read_only);

        // `modify:` runs last, so it composes with `table`/`rhai`/`base`.
        #[cfg(feature = "rhai")]
        if let Some(code) = modify {
            vista = self.apply_modify(vista, &code)?;
        }

        Ok(vista)
    }
}

/// What the SurrealDB shell implements. A query-sourced (`rhai:` or `base:`)
/// vista takes no writes, so it confines none, and has no table for a LIVE
/// subscription. Native conditions narrow it all the same.
fn surreal_capabilities(read_only: bool) -> VistaCapabilities {
    let reads = [
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Traverse,
    ];
    let table_only = [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::DeleteAll,
        Capability::ConfineWrites,
        Capability::WritesReachHooks,
        Capability::Subscribe,
    ];
    let caps: VistaCapabilities = reads.into_iter().collect();
    if read_only {
        return caps;
    }
    table_only.into_iter().fold(caps, VistaCapabilities::with)
}
