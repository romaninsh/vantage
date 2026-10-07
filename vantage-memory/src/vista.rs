//! The Vista layer: a `TableShell` over one store table.

pub mod factory;
mod refs;
mod shell;
mod watch;
mod writes;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_vista::{VistaCapabilities, VistaMetadata};

use crate::eval::MemoryCondition;
use crate::{MemoryTableHandle, Query};

pub use refs::Catalog;

/// A Vista shell over a store table. Holds its own query state; the rows
/// live in the shared table; a clone shares the table but not the query.
#[derive(Clone)]
pub struct MemoryTableShell {
    table: MemoryTableHandle,
    metadata: VistaMetadata,
    catalog: Catalog,
    query: Query,
    /// Literal `column = value` conditions of the query; full-record writes
    /// fill them and patches may not contradict them.
    pub(crate) invariants: IndexMap<String, CborValue>,
    page_size: Option<usize>,
    capabilities: VistaCapabilities,
}

impl MemoryTableShell {
    pub fn new(table: MemoryTableHandle, metadata: VistaMetadata, catalog: Catalog) -> Self {
        Self {
            table,
            metadata,
            catalog,
            query: Query::new(),
            invariants: IndexMap::new(),
            page_size: None,
            capabilities: Self::capabilities_for_memory(),
        }
    }

    pub fn capabilities_for_memory() -> VistaCapabilities {
        VistaCapabilities {
            can_count: true,
            can_insert: true,
            can_update: true,
            can_delete: true,
            can_import: true,
            can_order: true,
            can_search: true,
            can_filter_operators: true,
            can_set_page_size: true,
            can_fetch_page: true,
            can_fetch_window: true,
            can_traverse_to_record: true,
            can_traverse_to_set: true,
            can_subscribe: true,
            can_confine_writes: true,
            ..VistaCapabilities::default()
        }
    }

    pub(crate) fn query(&self) -> &Query {
        &self.query
    }

    /// The store table this shell reads and writes.
    pub fn table(&self) -> &MemoryTableHandle {
        &self.table
    }

    /// The declared id column, else the store table's.
    fn id_column_name(&self) -> &str {
        self.metadata
            .id_column
            .as_deref()
            .unwrap_or_else(|| self.table.id_column())
    }

    /// The shell's query with a one-off window, leaving the shell untouched.
    fn windowed(&self, offset: usize, limit: Option<usize>) -> Query {
        self.query.clone().window(offset, limit)
    }
}

/// Human-readable form of a condition, for `preview_query`.
fn describe(c: &MemoryCondition) -> String {
    let join = |list: &[MemoryCondition], sep: &str| {
        let parts: Vec<String> = list.iter().map(describe).collect();
        format!("({})", parts.join(sep))
    };
    match c {
        MemoryCondition::Cmp { path, op, value } => {
            format!("{path} {} {value:?}", op.key_symbol())
        }
        MemoryCondition::Search(text) => format!("search {text:?}"),
        MemoryCondition::And(all) => join(all, " AND "),
        MemoryCondition::Or(any) => join(any, " OR "),
        MemoryCondition::Not(inner) => format!("NOT {}", describe(inner)),
        MemoryCondition::Column(name) => name.clone(),
        MemoryCondition::Deferred(_) => "<deferred>".to_string(),
    }
}
