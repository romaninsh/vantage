//! `MongoVistaFactory` — YAML/typed-table entry points for building a
//! `Vista` against MongoDB. The struct, spec-lowering helpers, and the
//! `VistaFactory` trait impl all live in this file.

use std::sync::Arc;

use bson::Document;
use bson::oid::ObjectId;
use indexmap::IndexMap;
use vantage_core::{Context, Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{Capability, SpecResolver, Vista, VistaCapabilities, VistaFactory};

use crate::mongodb::impls::joins::table_joins;
use crate::mongodb::{MongoDB, MongoView};
use crate::types::AnyMongoType;
use crate::vista::source::MongoTableShell;
use crate::vista::spec::{
    MongoBlock, MongoColumnExtras, MongoReferenceExtras, MongoTableExtras, MongoVistaSpec,
};

mod ref_keys;

/// Resolves a YAML spec by table name, so references can rebuild their
/// target tables from the live spec.
pub type MongoSpecResolver = SpecResolver<MongoVistaSpec>;

#[derive(Clone)]
pub struct MongoVistaFactory {
    pub(crate) mongo: MongoDB,
    resolver: Option<MongoSpecResolver>,
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
    pub fn with_resolver(mut self, resolver: MongoSpecResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// The user resolver first, then a spec with no columns named after the
    /// target.
    fn resolver_with_fallback(&self) -> MongoSpecResolver {
        let user = self.resolver.clone();
        Arc::new(move |name: &str| {
            user.as_ref()
                .and_then(|resolve| resolve(name))
                .or_else(|| Some(bare_spec(name)))
        })
    }

    /// The spec a script's `table(name)` builds. With a resolver attached a
    /// name it does not know is an error, so a typo fails loudly; without
    /// one, every name is a bare collection.
    #[cfg(feature = "rhai")]
    pub(crate) fn script_spec(&self, name: &str) -> Result<MongoVistaSpec> {
        match &self.resolver {
            Some(resolve) => {
                resolve(name).ok_or_else(|| error!("script table(): unknown table", table = name))
            }
            None => Ok(bare_spec(name)),
        }
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
        let column_paths = if is_bare(&any_table) {
            IndexMap::new()
        } else {
            paths_from_table_columns(&any_table)
        };
        Ok(self.wrap_with_paths(any_table, column_paths, name, true, IndexMap::new()))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. The two entry points only differ in *where the
    /// path map comes from* (table aliases vs. spec `nested_path`); keeping
    /// the capability set and `Vista::new` call here means a future capability
    /// flip is a one-line edit. `ref_scripts` maps a reference name to its
    /// Rhai build-script.
    fn wrap_with_paths(
        &self,
        table: Table<MongoDB, EmptyEntity>,
        column_paths: IndexMap<String, Vec<String>>,
        name: String,
        search_unflagged_columns: bool,
        ref_scripts: IndexMap<String, String>,
    ) -> Vista {
        // MongoDB sorts on any field.
        let mut metadata = table.vista_metadata(VistaMetadataOptions {
            orderable: Orderable::All,
            references: true,
            contained: true,
            column_flags: true,
            ..VistaMetadataOptions::default()
        });
        for (relation, script) in ref_scripts {
            if let Some(reference) = metadata.references.get_mut(&relation) {
                reference.build_script = Some(script);
            }
        }
        let writable = table.data_source().is_stored();
        let capabilities = mongo_capabilities(writable, writable && self.live_enabled());
        let shell = MongoTableShell::new(
            table,
            capabilities,
            metadata,
            column_paths,
            self.resolver.clone(),
            search_unflagged_columns,
            self.change_streams,
        );
        Vista::new(name, shell)
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

/// A spec with no columns, over the collection of the same name.
fn bare_spec(name: &str) -> MongoVistaSpec {
    MongoVistaSpec {
        name: name.to_string(),
        datasource: None,
        id_column: None,
        columns: IndexMap::new(),
        references: IndexMap::new(),
        contained: IndexMap::new(),
        driver: MongoTableExtras::default(),
    }
}

/// Whether the table declares nothing beyond its `_id` key, which the vista
/// then reads and writes as stored documents.
fn is_bare<E: Entity<AnyMongoType>>(table: &Table<MongoDB, E>) -> bool {
    table.columns().len() == 1 && table.columns().contains_key("_id")
}

/// Build a `Table<MongoDB, EmptyEntity>` from a spec. A spec without columns
/// still gets an `_id` column, so the table knows its key. Each `references:`
/// entry rebuilds its target through `resolver` at traversal time (see
/// `Table::with_spec_references`).
pub(crate) fn build_mongo_table(
    spec: &MongoVistaSpec,
    db: MongoDB,
    resolver: Option<MongoSpecResolver>,
) -> Result<Table<MongoDB, EmptyEntity>> {
    let block = spec.driver.mongo.as_ref();
    let collection = block
        .and_then(|m| m.collection.clone())
        .unwrap_or_else(|| spec.name.clone());
    // The view comes from this spec alone: a reference target built over a
    // view-carrying source reads its own collection.
    let stages = block
        .map(view_stages)
        .transpose()
        .with_context(|| error!("pipeline view can't be built", table = spec.name.as_str()))?
        .flatten();
    let db = match stages {
        Some(stages) => db.with_view(MongoView::new(collection.clone(), stages)),
        None => db.without_view(),
    }
    .with_server_view(block.is_some_and(|m| m.view));

    let joined: Vec<&str> = spec
        .columns
        .keys()
        .filter(|name| name.contains('.'))
        .map(String::as_str)
        .collect();
    check_joined_columns(spec, &joined, db.view().is_some())?;

    let mut table = Table::<MongoDB, EmptyEntity>::new(collection, db);
    if spec.columns.is_empty() {
        table.add_column(TableColumn::<AnyMongoType>::new("_id"));
        table.set_spec_id_field("_id")?;
    } else {
        table.add_spec_columns(&spec.columns, build_column)?;
        table.set_spec_id_field(&spec.resolve_id_column_or("_id"))?;
    }
    let mut table = table.with_spec_references(&spec.references, resolver, build_mongo_table)?;
    // A dotted column name is a join through `has_one` references, which
    // must be declared first; resolving the chains now turns a broken one
    // into a build-time error.
    for name in joined {
        table.add_imported_column(name)?;
    }
    table_joins(&table)?;
    table.with_contained_specs(&spec.contained, build_column)
}

/// The pipeline a `mongo:` block declares, from `pipeline:` or a `rhai:`
/// script; `Ok(None)` without either.
fn view_stages(block: &MongoBlock) -> Result<Option<Vec<Document>>> {
    match &block.rhai {
        Some(_) if block.pipeline.is_some() => Err(error!(
            "a mongo block takes either `pipeline:` or `rhai:`, not both"
        )),
        Some(code) => pipeline_from_rhai(code, &block.args).map(Some),
        None => block.pipeline_stages(),
    }
}

#[cfg(feature = "rhai")]
fn pipeline_from_rhai(code: &str, args: &[(String, String)]) -> Result<Vec<Document>> {
    crate::vista::scripting::eval_pipeline(code, args)
}

#[cfg(not(feature = "rhai"))]
fn pipeline_from_rhai(_code: &str, _args: &[(String, String)]) -> Result<Vec<Document>> {
    Err(missing_rhai("rhai"))
}

/// A spec declares a script (`mongo.<slot>:`) the build cannot run.
#[cfg_attr(feature = "rhai", allow(dead_code))]
pub(crate) fn missing_rhai(slot: &str) -> vantage_core::VantageError {
    error!(
        "vista declares a Rhai script but vantage-mongodb was built without the `rhai` feature",
        slot = slot
    )
}

/// A dotted column names a join, so it cannot carry a BSON path of its own,
/// and a pipeline view reads computed rows that cannot be joined.
fn check_joined_columns(spec: &MongoVistaSpec, joined: &[&str], is_view: bool) -> Result<()> {
    let Some(first) = joined.first() else {
        return Ok(());
    };
    if is_view {
        return Err(error!(
            "a pipeline view cannot declare a joined (dotted) column",
            table = spec.name.as_str(),
            column = *first
        ));
    }
    for name in joined {
        if spec.columns[*name].driver.mongo.is_some() {
            return Err(error!(
                "a joined (dotted) column cannot set a mongo path",
                table = spec.name.as_str(),
                column = *name
            ));
        }
    }
    Ok(())
}

/// The column's BSON path rides on its alias, dot-joined, so a table built
/// from a spec carries it (see `paths_from_table_columns`). MongoDB itself
/// never reads column aliases: the vista source layer does the path walking.
pub(crate) fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<MongoColumnExtras>,
) -> Result<TableColumn<AnyMongoType>> {
    let alias = match col_spec.driver.mongo.as_ref() {
        Some(block) => block.resolved_path(name)?.map(|path| path.join(".")),
        None => None,
    };
    let column = TableColumn::from_spec(name, col_spec, alias, column_for_type)?;
    Ok(
        if col_spec.has_flag(vantage_vista::ColumnFlag::Searchable) {
            column.with_flag(ColumnFlag::Searchable)
        } else {
            column
        },
    )
}

/// YAML type alias → typed `Column` (then erased to `Column<AnyMongoType>`).
/// `object_id`, `datetime` and `decimal` columns parse written text back
/// into their BSON type (see `source::coerce`). `date` and `timestamp` are
/// aliases of `datetime`: a BSON DateTime with millisecond precision, not
/// the BSON Timestamp type.
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyMongoType>> {
    let col: TableColumn<AnyMongoType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" => TableColumn::from_column(TableColumn::<f64>::new(name)),
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "object_id" | "objectid" | "oid" => {
            TableColumn::from_column(TableColumn::<ObjectId>::new(name))
        }
        "datetime" | "date" | "timestamp" => {
            TableColumn::from_column(TableColumn::<bson::DateTime>::new(name))
        }
        "decimal" => TableColumn::from_column(TableColumn::<bson::Decimal128>::new(name)),
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

/// Build the spec column → BSON path map from a typed table's columns. Each
/// column with a `with_alias` uses the alias, split on `.`, as its path;
/// otherwise the spec name is its own path.
pub(crate) fn paths_from_table_columns<T, E>(table: &Table<T, E>) -> IndexMap<String, Vec<String>>
where
    T: vantage_table::traits::table_source::TableSource,
    E: Entity<T::Value>,
    T::Column<T::AnyType>: ColumnLike<T::AnyType>,
{
    let mut paths = IndexMap::new();
    for (name, col) in table.columns() {
        let path = match col.alias() {
            Some(a) => a.split('.').map(str::to_string).collect(),
            None => vec![name.clone()],
        };
        paths.insert(name.clone(), path);
    }
    paths
}

impl VistaFactory for MongoVistaFactory {
    type TableExtras = MongoTableExtras;
    type ColumnExtras = MongoColumnExtras;
    type ReferenceExtras = MongoReferenceExtras;

    fn build_from_spec(&self, spec: MongoVistaSpec) -> Result<Vista> {
        let modify = spec.driver.mongo.as_ref().and_then(|b| b.modify.clone());
        #[cfg(not(feature = "rhai"))]
        if modify.is_some() {
            return Err(missing_rhai("modify"));
        }
        let ref_scripts: IndexMap<String, String> = spec
            .references
            .iter()
            .filter_map(|(name, r)| {
                let script = r.driver.mongo.as_ref()?.rhai.clone()?;
                Some((name.clone(), script))
            })
            .collect();
        #[cfg(not(feature = "rhai"))]
        if let Some(relation) = ref_scripts.keys().next() {
            return Err(error!(
                "reference declares a `rhai:` build script but vantage-mongodb was built without the `rhai` feature",
                vista = spec.name.as_str(),
                relation = relation.as_str()
            ));
        }
        let table = self.table_from_spec(&spec)?;
        // A spec that declares nothing beyond `_id` reads documents as stored,
        // like a bare `from_table`, instead of projecting them down to `_id`.
        let column_paths = if is_bare(&table) {
            IndexMap::new()
        } else {
            paths_from_table_columns(&table)
        };
        let vista = self.wrap_with_paths(table, column_paths, spec.name, false, ref_scripts);

        // Last step: a `modify:` script narrows or tweaks the built vista.
        #[cfg(feature = "rhai")]
        if let Some(code) = modify {
            return crate::vista::scripting::apply_modify(self, vista, &code);
        }
        Ok(vista)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_rhai_names_the_feature_and_slot() {
        let msg = missing_rhai("modify").to_string();
        assert!(msg.contains("`rhai` feature"), "{msg}");
        assert!(msg.contains("modify"), "{msg}");
    }

    #[cfg(not(feature = "rhai"))]
    #[tokio::test]
    async fn reference_scripts_without_rhai_fail_the_build() {
        let client = mongodb::Client::with_uri_str("mongodb://localhost:27017")
            .await
            .unwrap();
        let spec: MongoVistaSpec = serde_yaml_ng::from_str(
            r#"
name: client
columns:
  _id: { type: string, flags: [id] }
references:
  orders:
    table: order
    kind: has_many
    foreign_key: client_id
    mongo: { rhai: 'table("order")' }
"#,
        )
        .unwrap();
        let err = MongoDB::new(client, "unit")
            .vista_factory()
            .build_from_spec(spec)
            .expect_err("a reference script needs the rhai feature");
        assert!(err.to_string().contains("`rhai` feature"), "{err}");
        assert_eq!(
            err.context.get("vista").map(String::as_str),
            Some("\"client\"")
        );
        assert_eq!(
            err.context.get("relation").map(String::as_str),
            Some("\"orders\"")
        );
    }
}
