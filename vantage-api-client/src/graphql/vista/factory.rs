//! `GraphqlApiVistaFactory` — typed-table entry point, `VistaFactory`
//! trait impl, and the YAML pipeline (`build_from_spec`, `register_yaml`,
//! `with_resolver`) for the GraphQL adapter.
//!
//! Mutations differ from one schema to the next, so the factory
//! advertises reads only: traversal always, and ordering, search and
//! paging per what the table's root field accepts. Metadata is harvested
//! from the typed table before erasure so column types, id field, title
//! fields, and relations all reach the Vista layer with full fidelity.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use vantage_core::{Result, error};
use vantage_table::table::{Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Capability, ColumnFlag, Reference as VistaReference, Vista, VistaCapabilities, VistaFactory,
    VistaMetadata,
};

use crate::graphql::api::GraphqlApi;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::lowering::{
    GraphqlSpecResolver, TargetResolver, build_table, target_resolver,
};
use crate::graphql::vista::source::GraphqlApiTableShell;
use crate::graphql::vista::spec::{GraphqlApiVistaSpec, GraphqlColumnExtras, GraphqlTableExtras};
use crate::join_keys::spec_join_keys;

type SpecRegistry = Arc<RwLock<HashMap<String, GraphqlApiVistaSpec>>>;

/// Builds `Vista`s over one [`GraphqlApi`]: from typed tables
/// ([`from_table`](Self::from_table)), from YAML specs
/// ([`build_from_spec`](VistaFactory::build_from_spec)), or from specs
/// registered by name ([`register_yaml`](Self::register_yaml) +
/// [`build`](Self::build)).
pub struct GraphqlApiVistaFactory {
    api: GraphqlApi,
    specs: SpecRegistry,
    resolver: Option<GraphqlSpecResolver>,
}

impl GraphqlApiVistaFactory {
    /// A factory over `api` with an empty spec registry and no resolver.
    pub fn new(api: GraphqlApi) -> Self {
        Self {
            api,
            specs: Arc::new(RwLock::new(HashMap::new())),
            resolver: None,
        }
    }

    /// The data source every Vista from this factory reads through.
    pub fn api(&self) -> &GraphqlApi {
        &self.api
    }

    /// Resolve YAML reference targets through `resolver` instead of the
    /// internal registry, for GraphQL specs kept elsewhere (an app-level
    /// inventory, say). Each spec it returns is lowered onto this factory's
    /// `GraphqlApi`, so every target is queried on the same endpoint; it
    /// cannot point a reference at another driver. A target it does not know
    /// fails the traversal.
    pub fn with_resolver(mut self, resolver: GraphqlSpecResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Add a YAML spec to the factory's internal registry. Without an
    /// explicit resolver, references resolve against this registry, so every
    /// model lives on this factory's `GraphqlApi` and is built on first
    /// traversal.
    pub fn register_yaml(&mut self, yaml: &str) -> Result<()> {
        let spec: GraphqlApiVistaSpec = serde_yaml_ng::from_str(yaml).map_err(|e| {
            error!(
                "Failed to parse GraphqlApiVistaSpec YAML",
                detail = e.to_string()
            )
        })?;
        self.specs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(spec.name.clone(), spec);
        Ok(())
    }

    /// Build the Vista for a model registered with
    /// [`register_yaml`](Self::register_yaml).
    pub fn build(&self, name: &str) -> Result<Vista> {
        let spec = registry_get(&self.specs, name)
            .ok_or_else(|| error!("No registered spec for model name", name = name))?;
        self.build_from_spec(spec)
    }

    /// The installed resolver, else one over the internal registry; either
    /// way paired with this factory's API (see `lowering`).
    fn target_resolver(&self) -> TargetResolver {
        let specs: GraphqlSpecResolver = match &self.resolver {
            Some(resolver) => resolver.clone(),
            None => {
                let registry = self.specs.clone();
                Arc::new(move |name: &str| registry_get(&registry, name))
            }
        };
        target_resolver(self.api.clone(), specs)
    }

    /// Wrap a typed `Table<GraphqlApi, E>` as a `Vista`. Column metadata,
    /// id field, title fields, and references are harvested up front;
    /// the table is erased to `Table<GraphqlApi, EmptyEntity>` so the
    /// shell carries a uniform entity type while still traversing the
    /// table's own `with_one` / `with_many` relations.
    pub fn from_table<E>(&self, table: Table<GraphqlApi, E>) -> Result<Vista>
    where
        E: Entity<AnyGraphqlType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = metadata_from_table(&table, true);
        let table = table.into_entity::<EmptyEntity>();
        Ok(wrap(table, name, metadata, None))
    }

    /// Build a `Table<GraphqlApi, EmptyEntity>` from a YAML spec. Per-table
    /// dialect / filter-arg / root-field overrides land on the table's
    /// `GraphqlApi` clone; `references:` rebuild their targets through the
    /// resolver (or the registry) at traversal time.
    ///
    /// Wrapping this table with [`from_table`](Self::from_table) traverses
    /// its references through the typed table, with unchecked targets;
    /// [`build_from_spec`](VistaFactory::build_from_spec) names each target
    /// after its spec and checks it when traversed.
    ///
    /// The returned table's `GraphqlApi` carries this spec's per-table
    /// overrides, column field aliases included; build other tables from
    /// [`api`](Self::api), not from a clone of this one.
    pub fn table_from_spec(
        &self,
        spec: &GraphqlApiVistaSpec,
    ) -> Result<Table<GraphqlApi, EmptyEntity>> {
        build_table(spec, &self.api, Some(self.target_resolver()))
    }
}

impl VistaFactory for GraphqlApiVistaFactory {
    type TableExtras = GraphqlTableExtras;
    type ColumnExtras = GraphqlColumnExtras;
    type ReferenceExtras = vantage_vista::NoExtras;

    fn build_from_spec(&self, spec: GraphqlApiVistaSpec) -> Result<Vista> {
        let table = self.table_from_spec(&spec)?;
        wrap_spec_table(table, &spec, self.target_resolver())
    }
}

fn registry_get(specs: &SpecRegistry, name: &str) -> Option<GraphqlApiVistaSpec> {
    specs
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(name)
        .cloned()
}

/// Wrap a table lowered from `spec` as a Vista named after the spec. Its
/// metadata carries the spec's references with their real targets and join
/// keys, and its shell traverses them through `resolver`.
pub(crate) fn wrap_spec_table(
    table: Table<GraphqlApi, EmptyEntity>,
    spec: &GraphqlApiVistaSpec,
    resolver: TargetResolver,
) -> Result<Vista> {
    let mut metadata = metadata_from_table(&table, false);
    let source_id = spec.resolve_id_column();
    for (relation, reference) in &spec.references {
        let keys = spec_join_keys(relation, reference, &source_id)?;
        metadata = metadata.with_reference(VistaReference::new(
            relation.clone(),
            reference.table.clone(),
            reference.kind,
            keys,
        ));
    }
    Ok(wrap(table, spec.name.clone(), metadata, Some(resolver)))
}

/// The one place a GraphQL shell is constructed.
fn wrap(
    table: Table<GraphqlApi, EmptyEntity>,
    name: String,
    metadata: VistaMetadata,
    resolver: Option<TargetResolver>,
) -> Vista {
    let capabilities = graphql_capabilities(table.data_source());
    let shell = GraphqlApiTableShell::new(table, capabilities, metadata).with_resolver(resolver);
    Vista::new(name, shell)
}

/// What the GraphQL shell implements, following what the table's root field
/// accepts, so a consumer either pushes the work to the server or does it
/// locally. It only reads: mutations differ from one schema to the next, so
/// no write capability is advertised and the Vista refuses every write.
/// Offset windows need filter push-down too, since local filtering would cut
/// a server-sized page short.
fn graphql_capabilities(api: &GraphqlApi) -> VistaCapabilities {
    [
        (Capability::Traverse, true),
        (Capability::Order, api.can_order()),
        (Capability::Search, api.can_search()),
        (Capability::FilterOps, api.can_filter_operators()),
        (Capability::Window, api.can_page()),
    ]
    .into_iter()
    .filter_map(|(capability, on)| on.then_some(capability))
    .collect()
}

/// Column, id and title metadata, plus the typed table's own `with_one` /
/// `with_many` relations when `references` is set.
fn metadata_from_table<E>(table: &Table<GraphqlApi, E>, references: bool) -> VistaMetadata
where
    E: Entity<AnyGraphqlType> + 'static,
{
    let mut metadata = table.vista_metadata(VistaMetadataOptions {
        id_flag: true,
        column_flags: true,
        references,
        ..VistaMetadataOptions::default()
    });
    // Any column the server returns can be ordered by, except a dotted path:
    // Hasura spells that as a nested `order_by` object, which isn't rendered.
    if table.data_source().can_order() {
        for (name, column) in metadata.columns.iter_mut() {
            if !name.contains('.') && !column.is_computed() {
                *column = column.clone().with_flag(ColumnFlag::Orderable);
            }
        }
    }
    metadata
}

#[cfg(test)]
mod tests;
