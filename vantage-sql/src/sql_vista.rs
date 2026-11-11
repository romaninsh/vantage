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

use crate::declared::{column_for_type_fn, sql_declared_types};
use vantage_table::source::SelectSource;
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

/// The [`SourceBlock`] of a dialect's table block. Each dialect declares its
/// own block type, every one with the same `table`, `rhai`, `base`, `inherit`
/// and `args` fields.
macro_rules! source_block {
    ($block:expr) => {{
        let block = $block;
        let inherit = block.inherit.as_ref();
        $crate::sql_vista::SourceBlock {
            table: block.table.as_deref(),
            rhai: block.rhai.as_deref(),
            base: block.base.as_deref(),
            inherit_columns: inherit.map_or(&[], |i| &i.columns),
            inherit_relations: inherit.map_or(&[], |i| &i.relations),
            args: &block.args,
        }
    }};
}
pub(crate) use source_block;

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
        Capability::WritesReachHooks,
    ];
    let caps: VistaCapabilities = reads.into_iter().collect();
    if read_only {
        return caps;
    }
    writes.into_iter().fold(caps, VistaCapabilities::with)
}

/// Whether a table reads from a sub-select rather than a stored table, so a
/// vista over it must be `read_only`. `from_table` asks this of every table,
/// reference targets included.
pub(crate) fn is_query_sourced<S>(source: &SelectSource<S>) -> bool {
    matches!(source, SelectSource::Query { .. })
}

sql_declared_types!(column_for_type_fn!());
