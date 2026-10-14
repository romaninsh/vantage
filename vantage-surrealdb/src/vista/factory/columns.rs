//! Spec columns: the stored field each names, and the typed column its YAML
//! type alias maps to.

use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_vista::ColumnSpec;

use crate::thing::Thing;
use crate::types::AnySurrealType;
use crate::vista::spec::SurrealColumnExtras;

/// The `surreal.field` block names the stored field.
pub(super) fn build_column(
    name: &str,
    col_spec: &ColumnSpec<SurrealColumnExtras>,
) -> Result<TableColumn<AnySurrealType>> {
    let field = col_spec
        .driver
        .surreal
        .as_ref()
        .and_then(|b| b.field.clone());
    TableColumn::from_spec(name, col_spec, field, column_for_type)
}

/// YAML type alias → typed `Column` (then erased to `Column<AnySurrealType>`).
fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnySurrealType>> {
    let col: TableColumn<AnySurrealType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "thing" | "record" | "record_id" => {
            TableColumn::from_column(TableColumn::<Thing>::new(name))
        }
        "datetime" => {
            TableColumn::from_column(TableColumn::<chrono::DateTime<chrono::Utc>>::new(name))
        }
        #[cfg(feature = "decimal")]
        "decimal" | "numeric" => {
            TableColumn::from_column(TableColumn::<rust_decimal::Decimal>::new(name))
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
