//! `MongoVistaFactory` — YAML/typed-table entry points for building a
//! `Vista` against MongoDB. The struct, spec-lowering helpers, and the
//! `VistaFactory` trait impl all live in this file.

use bson::oid::ObjectId;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Column as VistaColumn, NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
    flags as vista_flags,
};

use crate::mongodb::MongoDB;
use crate::types::AnyMongoType;
use crate::vista::source::MongoVistaSource;
use crate::vista::spec::{MongoColumnExtras, MongoTableExtras, MongoVistaSpec};

pub struct MongoVistaFactory {
    pub(crate) mongo: MongoDB,
}

impl MongoVistaFactory {
    pub fn new(mongo: MongoDB) -> Self {
        Self { mongo }
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through the table's reading path. Column aliases (set
    /// via `Column::with_alias`) seed the path map so renames flow through.
    pub fn from_table<E>(&self, table: Table<MongoDB, E>) -> Result<Vista>
    where
        E: Entity<AnyMongoType> + 'static,
    {
        let metadata = metadata_from_table(&table);
        let column_paths = paths_from_table_columns(&table);
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = MongoVistaSource::new(
            any_table,
            VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                ..VistaCapabilities::default()
            },
            column_paths,
        );
        Ok(Vista::new(name, Box::new(source), metadata))
    }

    /// Compute the spec column → BSON path map for a `MongoVistaSpec`.
    /// Pulled out so `build_from_spec` can hand both the table and the paths
    /// to the source in one shot.
    pub(crate) fn paths_from_spec(&self, spec: &MongoVistaSpec) -> IndexMap<String, Vec<String>> {
        let mut paths = IndexMap::new();
        for (name, col_spec) in &spec.columns {
            let path = col_spec
                .driver
                .mongo
                .as_ref()
                .and_then(|b| b.resolved_path())
                .unwrap_or_else(|| vec![name.clone()]);
            paths.insert(name.clone(), path);
        }
        paths
    }

    /// Build a `Table<MongoDB, EmptyEntity>` from a spec.
    pub fn table_from_spec(&self, spec: &MongoVistaSpec) -> Result<Table<MongoDB, EmptyEntity>> {
        let collection = spec
            .driver
            .mongo
            .as_ref()
            .and_then(|m| m.collection.clone())
            .unwrap_or_else(|| spec.name.clone());

        let mut table = Table::<MongoDB, EmptyEntity>::new(collection, self.mongo.clone());

        for (name, col_spec) in &spec.columns {
            let column = build_column(name, col_spec)?;
            table.add_column(column);
        }

        let id_column = resolve_id_column(spec);
        if !table.columns().contains_key(&id_column) {
            return Err(error!(
                "id column not present in spec.columns",
                id = id_column
            ));
        }
        table.set_id_field(&id_column);
        for (name, col_spec) in &spec.columns {
            if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
                table.add_title_field(name);
            }
        }

        Ok(table)
    }
}

pub(crate) fn resolve_id_column(spec: &MongoVistaSpec) -> String {
    if let Some(id) = &spec.id_column {
        return id.clone();
    }
    for (name, col_spec) in &spec.columns {
        if col_spec.flags.iter().any(|f| f == vista_flags::ID) {
            return name.clone();
        }
    }
    "_id".to_string()
}

pub(crate) fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<MongoColumnExtras>,
) -> Result<TableColumn<AnyMongoType>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let hidden = col_spec.flags.iter().any(|f| f == vista_flags::HIDDEN);

    // The vista source layer handles read/write/filter via `column_paths` —
    // we deliberately don't push BSON renames down via `with_alias`, since
    // Mongo's `doc_to_record` doesn't honour aliases anyway.
    let mut col = column_for_type(name, ty)?;
    if hidden {
        col = col.with_flag(ColumnFlag::Hidden);
    }
    Ok(col)
}

/// YAML type alias → typed `Column` (then erased to `Column<AnyMongoType>`).
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
/// column with a `with_alias` uses the alias as a single-segment path;
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
            Some(a) => vec![a.to_string()],
            None => vec![name.clone()],
        };
        paths.insert(name.clone(), path);
    }
    paths
}

pub(crate) fn metadata_from_table<T, E>(table: &Table<T, E>) -> VistaMetadata
where
    T: vantage_table::traits::table_source::TableSource,
    E: Entity<T::Value>,
    T::Column<T::AnyType>: ColumnLike<T::AnyType>,
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
        metadata = metadata.with_id_column(id_field.name().to_string());
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
    metadata
}

impl VistaFactory for MongoVistaFactory {
    type TableExtras = MongoTableExtras;
    type ColumnExtras = MongoColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: MongoVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let column_paths = self.paths_from_spec(&spec);
        let table = self.table_from_spec(&spec)?;

        // Mirror `from_table` — we can't call it here because we need to
        // override the column_paths with the spec-derived map (which knows
        // about nested_path) rather than the alias-derived one.
        let metadata = metadata_from_table(&table);
        let source = MongoVistaSource::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                ..VistaCapabilities::default()
            },
            column_paths,
        );
        let mut vista = Vista::new(spec.name.clone(), Box::new(source), metadata);
        vista.set_name(vista_name);
        Ok(vista)
    }
}
