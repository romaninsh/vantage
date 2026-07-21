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
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, ReferenceKind, Vista, VistaCapabilities,
    VistaFactory, VistaMetadata, flags as vista_flags,
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
        // A configured `total_key` lets the shell serve absolute-offset
        // windows (and an exact count) — advertise it before erasing the table.
        let can_fetch_window = table.data_source().total_key().is_some();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = RestApiTableShell::new(
            any_table,
            VistaCapabilities {
                can_count: true,
                can_traverse_to_record: true,
                can_fetch_window,
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
                ref_spec
                    .foreign_key
                    .clone()
                    .unwrap_or_else(|| rel_name.clone()),
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
                    foreign_key: ref_spec
                        .foreign_key
                        .clone()
                        .unwrap_or_else(|| rel_name.clone()),
                    keys: ref_spec.keys.clone(),
                },
            );
        }

        let can_fetch_window = table.data_source().total_key().is_some();
        let source = RestApiTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_traverse_to_record: true,
                can_fetch_window,
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

        let id_column = resolve_id_column(spec);

        let mut table = Table::<RestApi, EmptyEntity>::new(endpoint, self.api.clone());
        for (name, col_spec) in &spec.columns {
            table.add_column(build_column(name, col_spec)?);
            if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
                table.add_title_field(name);
            }
            if let Some(code) = &col_spec.lazy {
                add_lazy_column(&mut table, name, code)?;
            }
        }

        if !table.columns().contains_key(&id_column) {
            return Err(error!(
                "id column not present in spec.columns",
                id = id_column
            ));
        }
        table.set_id_field(&id_column);

        Ok(table)
    }
}

/// Lower a column's `lazy:` script onto the table — a Rhai closure run in
/// Rust on each returned record (`row` in scope), never sent to the API.
/// The REST carrier is already CBOR, so no per-value conversion is needed.
#[cfg(feature = "rhai")]
fn add_lazy_column(table: &mut Table<RestApi, EmptyEntity>, name: &str, code: &str) -> Result<()> {
    let script = vantage_vista::lazy_value_closure(code.to_string());
    table.add_lazy_expression(
        name,
        Arc::new(move |record| {
            let row = record.clone();
            let script = script.clone();
            Box::pin(async move { script(&row) })
        }),
    );
    Ok(())
}

#[cfg(not(feature = "rhai"))]
fn add_lazy_column(
    _table: &mut Table<RestApi, EmptyEntity>,
    name: &str,
    _code: &str,
) -> Result<()> {
    Err(error!(
        "column declares a `lazy:` script but vantage-api-client was built without the `rhai` feature",
        column = name
    ))
}

/// Pick the id column from an `id_column:` field or the first column
/// flagged `id`; falls back to `"id"`.
fn resolve_id_column(spec: &RestApiVistaSpec) -> String {
    if let Some(id) = &spec.id_column {
        return id.clone();
    }
    for (name, col_spec) in &spec.columns {
        if col_spec.flags.iter().any(|f| f == vista_flags::ID) {
            return name.clone();
        }
    }
    "id".to_string()
}

fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<ApiColumnExtras>,
) -> Result<TableColumn<CborValue>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let hidden = col_spec.flags.iter().any(|f| f == vista_flags::HIDDEN);

    let mut col = column_for_type(name, ty)?;
    if hidden {
        col = col.with_flag(ColumnFlag::Hidden);
    }
    Ok(col)
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
    let mut metadata = VistaMetadata::new();
    for (name, col) in table.columns() {
        let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string());
        if col.flags().contains(&ColumnFlag::Hidden) {
            vc = vc.with_flag(vista_flags::HIDDEN);
        }
        metadata = metadata.with_column(vc);
    }
    if let Some(id_field) = table.id_field() {
        let id = id_field.name().to_string();
        metadata = metadata.with_id_column(id.clone());
        if let Some(col) = metadata.columns.get_mut(&id) {
            col.flags.push(vista_flags::ID.to_string());
        }
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
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
