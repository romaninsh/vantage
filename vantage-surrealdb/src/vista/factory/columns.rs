//! Spec columns: the stored field each names, and the typed column its YAML
//! type alias maps to (the vocabulary lives in `surrealdb::declared`, shared
//! with the write path).

use vantage_core::Result;
use vantage_table::column::core::Column as TableColumn;
use vantage_vista::ColumnSpec;

use crate::surrealdb::declared::column_for_type;
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
