//! Vista factory pieces the SQL dialects share: the YAML column types and
//! the metadata flags.

use vantage_core::{Result, error};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::{Orderable, VistaMetadataOptions};

/// SQL can ORDER BY any column server-side, computed ones included (they
/// order by their output alias); computed columns are read-only.
pub(crate) fn metadata_options() -> VistaMetadataOptions {
    VistaMetadataOptions {
        orderable: Orderable::All,
        calculated: true,
        references: true,
        contained: true,
        ..VistaMetadataOptions::default()
    }
}

/// YAML type alias → typed `Column`, erased to the dialect's value type.
pub(crate) fn column_for_type<A: ColumnType>(name: &str, ty: &str) -> Result<Column<A>> {
    let col = match ty {
        "int" | "integer" | "i64" | "i32" => Column::from_column(Column::<i64>::new(name)),
        "float" | "double" | "f64" | "f32" => Column::from_column(Column::<f64>::new(name)),
        "bool" | "boolean" => Column::from_column(Column::<bool>::new(name)),
        "string" | "text" | "str" => Column::from_column(Column::<String>::new(name)),
        "decimal" | "numeric" => Column::from_column(Column::<rust_decimal::Decimal>::new(name)),
        "date" => Column::from_column(Column::<chrono::NaiveDate>::new(name)),
        "time" => Column::from_column(Column::<chrono::NaiveTime>::new(name)),
        "datetime" => Column::from_column(Column::<chrono::NaiveDateTime>::new(name)),
        "timestamp" => Column::from_column(Column::<chrono::DateTime<chrono::Utc>>::new(name)),
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
