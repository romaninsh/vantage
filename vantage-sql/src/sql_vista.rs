//! Vista pieces the SQL dialects share: the generic shell and what it asks
//! of a dialect, YAML spec lowering, the YAML column types, the metadata
//! flags and the capability set.

mod dialect;
// The backends name the shell publicly (`PostgresTableShell`, …) while the
// dialect trait its impls are bounded by stays crate-internal: only this
// crate's dialects implement it.
#[allow(private_bounds, private_interfaces)]
mod narrow;
#[allow(private_bounds, private_interfaces)]
mod refs;
#[allow(private_bounds, private_interfaces)]
mod shell;
mod spec_table;

pub(crate) use dialect::{Shell, SqlShellDialect};
pub use shell::SqlTableShell;
#[cfg(feature = "mysql")]
pub(crate) use shell::native_record;
pub(crate) use spec_table::{SourceBlock, SqlSpecDialect, build_spec_table, lower_spec};

use vantage_core::{Result, error};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::{Orderable, VistaMetadataOptions};
use vantage_vista::{Capability, VistaCapabilities};

/// SQL can ORDER BY any column server-side, computed ones included (they
/// order by their output alias); computed columns are read-only. Every
/// typed column flag (a YAML `searchable`, `mandatory`, `label`, …) reaches
/// the vista column.
pub(crate) fn metadata_options() -> VistaMetadataOptions {
    VistaMetadataOptions {
        orderable: Orderable::All,
        calculated: true,
        column_flags: true,
        references: true,
        contained: true,
        ..VistaMetadataOptions::default()
    }
}

/// What every SQL shell implements. A query-sourced (`rhai:` or `base:`)
/// vista is `read_only`: it takes no writes, so it confines none.
pub(crate) fn sql_capabilities(read_only: bool) -> VistaCapabilities {
    let reads = [
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::Window,
        Capability::Traverse,
    ];
    let writes = [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
    ];
    let caps: VistaCapabilities = reads.into_iter().collect();
    if read_only {
        return caps;
    }
    writes.into_iter().fold(caps, VistaCapabilities::with)
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
