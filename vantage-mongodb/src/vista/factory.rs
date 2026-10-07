//! `MongoVistaFactory` — YAML/typed-table entry points for building a
//! `Vista` against MongoDB. The struct, spec-lowering helpers, and the
//! `VistaFactory` trait impl all live in this file.

use bson::oid::ObjectId;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{NoExtras, Vista, VistaCapabilities, VistaFactory};

use crate::mongodb::MongoDB;
use crate::types::AnyMongoType;
use crate::vista::source::MongoTableShell;
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
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();
        let column_paths = paths_from_table_columns(&any_table);
        Ok(self.wrap_with_paths(any_table, column_paths, name))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. The two entry points only differ in *where the
    /// path map comes from* (table aliases vs. spec `nested_path`); keeping
    /// the capability set and `Vista::new` call here means a future capability
    /// flip is a one-line edit.
    fn wrap_with_paths(
        &self,
        table: Table<MongoDB, EmptyEntity>,
        column_paths: IndexMap<String, Vec<String>>,
        name: String,
    ) -> Vista {
        // MongoDB sorts on any field.
        let metadata = table.vista_metadata(VistaMetadataOptions {
            orderable: Orderable::All,
            references: true,
            contained: true,
            ..VistaMetadataOptions::default()
        });
        let source = MongoTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                can_order: true,
                can_search: true,
                can_set_page_size: true,
                can_fetch_page: true,
                can_fetch_next: true,
                can_traverse_to_record: true,
                can_confine_writes: true,
                ..VistaCapabilities::default()
            },
            metadata,
            column_paths,
        );
        Vista::new(name, Box::new(source))
    }

    /// Compute the spec column → BSON path map for a `MongoVistaSpec`.
    /// Validates each column's `mongo` block.
    pub(crate) fn paths_from_spec(
        &self,
        spec: &MongoVistaSpec,
    ) -> Result<IndexMap<String, Vec<String>>> {
        let mut paths = IndexMap::new();
        for (name, col_spec) in spec.columns.iter().filter(|(_, c)| c.lazy.is_none()) {
            let path = match col_spec.driver.mongo.as_ref() {
                Some(block) => block
                    .resolved_path(name)?
                    .unwrap_or_else(|| vec![name.clone()]),
                None => vec![name.clone()],
            };
            paths.insert(name.clone(), path);
        }
        Ok(paths)
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
        table.add_spec_columns(&spec.columns, build_column)?;
        table.set_spec_id_field(&spec.resolve_id_column_or("_id"))?;
        table.with_contained_specs(&spec.contained, build_column)
    }
}

/// The vista source layer handles read/write/filter via `column_paths` —
/// we deliberately don't push BSON renames down via `with_alias`, since
/// Mongo's `doc_to_record` doesn't honour aliases anyway.
pub(crate) fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<MongoColumnExtras>,
) -> Result<TableColumn<AnyMongoType>> {
    TableColumn::from_spec(name, col_spec, None, column_for_type)
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

impl VistaFactory for MongoVistaFactory {
    type TableExtras = MongoTableExtras;
    type ColumnExtras = MongoColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: MongoVistaSpec) -> Result<Vista> {
        let column_paths = self.paths_from_spec(&spec)?;
        let table = self.table_from_spec(&spec)?;
        Ok(self.wrap_with_paths(table, column_paths, spec.name))
    }
}
