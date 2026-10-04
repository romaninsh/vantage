//! `RestApiVistaFactory` — typed-table entry point, `VistaFactory`
//! trait impl, plus the YAML factory pipeline (`build_from_spec`,
//! `register_yaml`, `with_model_resolver`).
//!
//! REST API is read-only at this stage, so the factory advertises
//! only `can_count`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Reference as VistaReference, ReferenceKind, Vista, VistaCapabilities, VistaFactory,
    VistaMetadata,
};

use super::source::{RestApiTableShell, YamlReference, YamlReferenceKind};
use super::spec::{ApiColumnExtras, ApiReferenceExtras, ApiTableExtras, RestApiVistaSpec};
use crate::RestApi;

/// Callback that maps a model name to its `Vista`. The factory uses
/// it at relation-traversal time so cross-driver lookups (vantage-ui's
/// inventory layer) and same-driver lookups (the internal
/// `register_yaml` registry) flow through one channel.
pub type ModelResolver = Arc<dyn Fn(&str) -> Result<Vista> + Send + Sync>;

pub struct RestApiVistaFactory {
    api: RestApi,
    specs: Arc<RwLock<HashMap<String, RestApiVistaSpec>>>,
    resolver: Option<ModelResolver>,
}

impl RestApiVistaFactory {
    pub fn new(api: RestApi) -> Self {
        Self {
            api,
            specs: Arc::new(RwLock::new(HashMap::new())),
            resolver: None,
        }
    }

    pub fn api(&self) -> &RestApi {
        &self.api
    }

    /// Install a model-resolver callback. Use this when models live
    /// across multiple drivers (e.g. vantage-ui's inventory crosses
    /// drivers and decides which factory to build each model from).
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
        self.specs.write().unwrap().insert(spec.name.clone(), spec);
        Ok(())
    }

    /// Build the Vista for a previously-registered model name. Uses
    /// the internal registry; the installed resolver (if any) takes
    /// over at relation traversal time.
    pub fn build(&self, name: &str) -> Result<Vista> {
        let spec = self
            .specs
            .read()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| error!("No registered spec for model name", name = name.to_string()))?;
        self.build_from_spec(spec)
    }

    /// Wrap a typed `Table<RestApi, E>` as a `Vista`. Column metadata,
    /// id field, title fields, and references are harvested up front;
    /// the table is erased to `Table<RestApi, EmptyEntity>` so the
    /// shell carries a uniform entity type while still routing
    /// reference traversal through `Reference::resolve_from_row`.
    pub fn from_table<E>(&self, table: Table<RestApi, E>) -> Result<Vista>
    where
        E: Entity<CborValue> + 'static,
    {
        let metadata = metadata_from_table(&table);
        let name = table.table_name().to_string();
        // A paging API serves absolute-offset windows (a `total_key` adds an
        // exact count) — advertise it before erasing the table.
        let can_fetch_window = table.data_source().serves_windows();
        let can_order = table.data_source().ordering().is_some();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = RestApiTableShell::new(
            any_table,
            VistaCapabilities {
                can_count: true,
                can_traverse_to_record: true,
                can_fetch_window,
                can_order,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Ok(Vista::new(name, Box::new(source)))
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
            let spec = specs.read().unwrap().get(name).cloned().ok_or_else(|| {
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
        let vista_name = spec.name.clone();

        // Harvest column / id / title metadata from the typed table.
        let mut metadata = metadata_from_table(&table);

        // Surface YAML-declared references as Vista metadata so
        // generic UI layers see them via `vista.get_references()`.
        for (rel_name, ref_spec) in &spec.references {
            metadata = metadata.with_reference(VistaReference::new(
                rel_name.clone(),
                ref_spec.table.clone(),
                ref_spec.kind,
                ref_spec.foreign_key_or(rel_name),
            ));
        }

        // Build the YAML reference table for the shell. The shell
        // consults this at `get_ref` time and threads a `DeferredFn`
        // through the child. The child's URL form is the child's
        // own concern (its `api.endpoint`), not the parent's.
        let mut yaml_refs = indexmap::IndexMap::new();
        for (rel_name, ref_spec) in &spec.references {
            yaml_refs.insert(
                rel_name.clone(),
                YamlReference {
                    target: ref_spec.table.clone(),
                    kind: match ref_spec.kind {
                        ReferenceKind::HasOne => YamlReferenceKind::HasOne,
                        ReferenceKind::HasMany => YamlReferenceKind::HasMany,
                    },
                    foreign_key: ref_spec.foreign_key_or(rel_name),
                    keys: ref_spec.keys.clone(),
                },
            );
        }

        let can_fetch_window = table.data_source().serves_windows();
        let can_order = table.data_source().ordering().is_some();
        let source = RestApiTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_traverse_to_record: true,
                can_fetch_window,
                can_order,
                ..VistaCapabilities::default()
            },
            metadata,
        )
        .with_yaml_refs(yaml_refs)
        .with_resolver(self.resolver_for_specs());

        let mut vista = Vista::new(vista_name.clone(), Box::new(source));
        vista.set_name(vista_name);
        Ok(vista)
    }
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

fn metadata_from_table<E>(table: &Table<RestApi, E>) -> VistaMetadata
where
    E: Entity<CborValue> + 'static,
{
    // An API with a sort param is assumed to sort on any column it returns;
    // without one, no column is orderable and consumers sort client-side.
    let orderable = match table.data_source().ordering() {
        Some(_) => Orderable::All,
        None => Orderable::None,
    };
    let mut metadata = table.vista_metadata(VistaMetadataOptions {
        orderable,
        id_flag: true,
        ..VistaMetadataOptions::default()
    });
    for relation in table.references() {
        metadata = metadata.with_reference(VistaReference::new(
            relation.clone(),
            "",
            ReferenceKind::HasMany,
            "",
        ));
    }
    metadata
}
