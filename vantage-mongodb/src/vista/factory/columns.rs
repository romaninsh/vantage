//! Spec columns as typed table columns, and each column's BSON path.

use bson::oid::ObjectId;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::Entity;

use crate::types::AnyMongoType;
use crate::vista::spec::MongoColumnExtras;

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
    TableColumn::from_spec(name, col_spec, alias, column_for_type)
}

/// YAML type alias → typed `Column` (then erased to `Column<AnyMongoType>`).
/// `object_id`, `datetime` and `decimal` columns parse written text back
/// into their BSON type (see `source::coerce`). `date` and `timestamp` are
/// stored as a BSON DateTime too (millisecond precision, not the BSON
/// Timestamp type) and read back typed; their column types are the chrono
/// types the SQL drivers declare them with. BSON has no time of day, so
/// `time` is stored as carrier text and read back typed; `bytes` is a byte
/// count.
fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyMongoType>> {
    let col: TableColumn<AnyMongoType> = match ty {
        "int" | "integer" | "i64" | "i32" | "bytes" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" => TableColumn::from_column(TableColumn::<f64>::new(name)),
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "time" => TableColumn::from_column(TableColumn::<NaiveTime>::new(name)),
        "object_id" | "objectid" | "oid" => {
            TableColumn::from_column(TableColumn::<ObjectId>::new(name))
        }
        "datetime" => TableColumn::from_column(TableColumn::<bson::DateTime>::new(name)),
        "date" => TableColumn::from_column(TableColumn::<NaiveDate>::new(name)),
        "timestamp" => TableColumn::from_column(TableColumn::<DateTime<Utc>>::new(name)),
        "decimal" => TableColumn::from_column(TableColumn::<bson::Decimal128>::new(name)),
        "json" => TableColumn::from_column(TableColumn::<serde_json::Value>::new(name)),
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
