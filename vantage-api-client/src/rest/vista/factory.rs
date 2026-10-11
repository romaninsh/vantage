//! `RestApiVistaFactory` — typed-table entry point, `VistaFactory`
//! trait impl, plus the YAML factory pipeline (`build_from_spec`,
//! `register_yaml`, `with_model_resolver`).
//!
//! The factory advertises reads only: traversal always, absolute-offset
//! windows when the API pages, and ordering when it has a sort param.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Capability, Reference as VistaReference, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
};

use super::source::RestApiTableShell;
use super::spec::{ApiColumnExtras, ApiReferenceExtras, ApiTableExtras, RestApiVistaSpec};
use crate::RestApi;
use crate::join_keys::spec_join_keys;

/// Callback that maps a model name to its `Vista`. The factory uses
/// it at relation-traversal time so cross-driver lookups (an application
/// catalogue spanning several data sources) and same-driver lookups (the
/// internal `register_yaml` registry) flow through one channel.
pub type ModelResolver = Arc<dyn Fn(&str) -> Result<Vista> + Send + Sync>;

type SpecRegistry = Arc<RwLock<HashMap<String, RestApiVistaSpec>>>;

/// Builds `Vista`s over one [`RestApi`]: from typed tables
/// ([`from_table`](Self::from_table)), from YAML specs
/// ([`build_from_spec`](VistaFactory::build_from_spec)), or from specs
/// registered by name ([`register_yaml`](Self::register_yaml) +
/// [`build`](Self::build)).
pub struct RestApiVistaFactory {
    api: RestApi,
    specs: SpecRegistry,
    resolver: Option<ModelResolver>,
}

/// A registered spec by name. A writer that panicked can't leave a map
/// insert half-done, so a poisoned lock is read through.
fn registry_get(specs: &SpecRegistry, name: &str) -> Option<RestApiVistaSpec> {
    specs
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(name)
        .cloned()
}

impl RestApiVistaFactory {
    /// A factory over `api` with an empty spec registry and no resolver.
    pub fn new(api: RestApi) -> Self {
        Self {
            api,
            specs: Arc::new(RwLock::new(HashMap::new())),
            resolver: None,
        }
    }

    /// The data source every Vista from this factory reads through.
    pub fn api(&self) -> &RestApi {
        &self.api
    }

    /// Install a model-resolver callback. Use this when models live
    /// across multiple drivers and the caller decides which factory
    /// builds each model.
    pub fn with_model_resolver(mut self, resolver: ModelResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Accumulate a YAML spec in the factory's internal registry.
    /// When no explicit resolver is installed, references resolve
    /// against this registry — i.e. all models live in the same
    /// `RestApi` and are built lazily on first traversal.
    pub fn register_yaml(&mut self, yaml: &str) -> Result<()> {
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).map_err(|e| {
            error!(
                "Failed to parse RestApiVistaSpec YAML",
                detail = e.to_string()
            )
        })?;
        self.specs
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(spec.name.clone(), spec);
        Ok(())
    }

    /// Build the Vista for a previously-registered model name. Uses
    /// the internal registry; the installed resolver (if any) takes
    /// over at relation traversal time.
    pub fn build(&self, name: &str) -> Result<Vista> {
        let spec = registry_get(&self.specs, name)
            .ok_or_else(|| error!("No registered spec for model name", name = name.to_string()))?;
        self.build_from_spec(spec)
    }

    /// Wrap a typed `Table<RestApi, E>` as a `Vista`. Column metadata,
    /// id field, title fields, and references are harvested up front;
    /// the table is erased to `Table<RestApi, EmptyEntity>` so the
    /// shell carries a uniform entity type while still traversing the
    /// table's own `with_one` / `with_many` relations.
    pub fn from_table<E>(&self, table: Table<RestApi, E>) -> Result<Vista>
    where
        E: Entity<CborValue> + 'static,
    {
        let metadata = metadata_from_table(&table, true);
        let name = table.table_name().to_string();
        let capabilities = rest_capabilities(table.data_source());
        let shell =
            RestApiTableShell::new(table.into_entity::<EmptyEntity>(), capabilities, metadata);
        Ok(Vista::new(name, shell))
    }

    /// Resolve a model name to a Vista — either via the installed
    /// resolver or through the internal registry.
    fn resolver_for_specs(&self) -> ModelResolver {
        if let Some(r) = &self.resolver {
            return r.clone();
        }
        // Default resolver: recursively build_from_spec against the
        // shared registry. Cloning the Arc keeps the closure cheap.
        let specs = self.specs.clone();
        let api = self.api.clone();
        Arc::new(move |name: &str| -> Result<Vista> {
            let spec = registry_get(&specs, name).ok_or_else(|| {
                error!(
                    "Model resolver: no spec registered for name",
                    name = name.to_string()
                )
            })?;
            // Reuse the same factory pipeline as the top-level build.
            let mut factory = RestApiVistaFactory::new(api.clone());
            factory.specs = specs.clone();
            factory.build_from_spec(spec)
        })
    }
}

impl VistaFactory for RestApiVistaFactory {
    type TableExtras = ApiTableExtras;
    type ColumnExtras = ApiColumnExtras;
    type ReferenceExtras = ApiReferenceExtras;

    fn build_from_spec(&self, spec: RestApiVistaSpec) -> Result<Vista> {
        let table = self.table_from_spec(&spec)?;

        // Column / id / title metadata from the typed table, plus the YAML
        // references, so generic UI layers see them through
        // `vista.references()`. A target's URL form is its own concern (its
        // `api.endpoint`); the parent only declares the join.
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

        let capabilities = rest_capabilities(table.data_source());
        let shell = RestApiTableShell::new(table, capabilities, metadata)
            .with_resolver(self.resolver_for_specs());
        Ok(Vista::new(spec.name, shell))
    }
}

/// What the REST shell implements. It only reads: absolute-offset windows
/// when the API pages (a `total_key` adds the grand total to each page),
/// ordering when it has a sort param, and traversal. With no write
/// capability advertised, the Vista refuses every write.
fn rest_capabilities(api: &RestApi) -> VistaCapabilities {
    let mut caps = VistaCapabilities::new().with(Capability::Traverse);
    if api.serves_windows() {
        caps = caps.with(Capability::Window);
    }
    if api.ordering().is_some() {
        caps = caps.with(Capability::Order);
    }
    caps
}

impl RestApiVistaFactory {
    /// Lower a `RestApiVistaSpec` into a typed `Table<RestApi,
    /// EmptyEntity>`. Endpoint defaults to `spec.name` when the
    /// `api.endpoint` block is absent.
    fn table_from_spec(&self, spec: &RestApiVistaSpec) -> Result<Table<RestApi, EmptyEntity>> {
        let endpoint = spec
            .driver
            .api
            .as_ref()
            .and_then(|b| b.endpoint.clone())
            .unwrap_or_else(|| spec.name.clone());

        let mut table = Table::<RestApi, EmptyEntity>::new(endpoint, self.api.clone());
        table.add_spec_columns(&spec.columns, build_column)?;
        table.set_spec_id_field(&spec.resolve_id_column())?;
        Ok(table)
    }
}

fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<ApiColumnExtras>,
) -> Result<TableColumn<CborValue>> {
    TableColumn::from_spec(name, col_spec, None, column_for_type)
}

/// YAML type alias → typed `Column<T>` → erased to `Column<CborValue>`
/// for storage. The original type label survives via
/// `Column::from_column`, so generic UIs see "i64" / "f64" / "bool"
/// / "string" in `column_types()` regardless of the wire format.
fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<CborValue>> {
    let col: TableColumn<CborValue> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "json" | "any" => TableColumn::from_column(TableColumn::<CborValue>::new(name)),
        other => {
            return Err(error!(
                "Unknown YAML column type",
                column = name,
                ty = other.to_string()
            ));
        }
    };
    Ok(col)
}

/// Column, id and title metadata, plus the typed table's own `with_one` /
/// `with_many` relations when `references` is set.
fn metadata_from_table<E>(table: &Table<RestApi, E>, references: bool) -> VistaMetadata
where
    E: Entity<CborValue> + 'static,
{
    // An API with a sort param is assumed to sort on any column it returns;
    // without one, no column is orderable and consumers sort client-side.
    let orderable = match table.data_source().ordering() {
        Some(_) => Orderable::All,
        None => Orderable::None,
    };
    table.vista_metadata(VistaMetadataOptions {
        orderable,
        id_flag: true,
        references,
        ..VistaMetadataOptions::default()
    })
}
