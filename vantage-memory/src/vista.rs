//! The Vista layer: a `TableShell` over one store table.

mod affinity;
mod catalog;
mod factory;
mod narrow;
mod refs;
mod shell;
mod watch;
mod writes;

use affinity::Affinity;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_vista::{Capability, SortDirection, VistaCapabilities, VistaMetadata};

use crate::eval::MemoryCondition;
use crate::{MemoryTableHandle, Query};

pub use catalog::MemoryCatalog;
pub use factory::{MemoryBlock, MemoryTableExtras, MemoryVistaFactory, MemoryVistaSpec};

/// A Vista shell over a store table. Holds its own query state; the rows
/// live in the shared table; a clone shares the table but not the query.
#[derive(Clone)]
pub struct MemoryTableShell {
    table: MemoryTableHandle,
    metadata: VistaMetadata,
    catalog: MemoryCatalog,
    query: Query,
    /// Literal `column = value` conditions of the query; full-record writes
    /// fill them and patches may not contradict them.
    pub(crate) invariants: IndexMap<String, CborValue>,
    capabilities: VistaCapabilities,
}

impl MemoryTableShell {
    /// A shell over `table`. Cells of a column `metadata` declares with an
    /// integer or text type are stored and compared in that form; a column
    /// declared [untyped](vantage_vista::Column::untyped) stores them as
    /// given. Metadata without an id column takes the store table's.
    /// Traversal targets come from `catalog`.
    pub fn new(table: MemoryTableHandle, metadata: VistaMetadata, catalog: MemoryCatalog) -> Self {
        let metadata = match metadata.id_column {
            Some(_) => metadata,
            None => metadata.with_id_column(table.id_column().to_string()),
        };
        Self {
            table,
            metadata,
            catalog,
            query: Query::new(),
            invariants: IndexMap::new(),
            capabilities: Self::capabilities_for_memory(),
        }
    }

    fn affinity(&self, column: &str) -> Affinity {
        Affinity::of(&self.metadata, self.id_column_name(), column)
    }

    /// Whether `column` has no declared type: not declared, or declared
    /// without a type.
    fn is_untyped(&self, column: &str) -> bool {
        column != self.id_column_name()
            && self
                .metadata
                .columns
                .get(column)
                .is_none_or(|c| c.original_type.is_none())
    }

    /// Everything but cursors and native conditions: the store evaluates
    /// every operator, search, order and window itself, and every write is
    /// checked against the shell's query, so writes stay inside the set.
    pub(crate) fn capabilities_for_memory() -> VistaCapabilities {
        [
            Capability::Insert,
            Capability::Update,
            Capability::Delete,
            Capability::Import,
            Capability::Subscribe,
            Capability::Order,
            Capability::Search,
            Capability::FilterOps,
            Capability::Window,
            Capability::Traverse,
            Capability::ConfineWrites,
        ]
        .into_iter()
        .collect()
    }

    pub(crate) fn query(&self) -> &Query {
        &self.query
    }

    /// The store table this shell reads and writes.
    pub fn table(&self) -> &MemoryTableHandle {
        &self.table
    }

    /// The declared id column. `new` fills in the store table's when the
    /// metadata names none, and the metadata never changes afterwards.
    fn id_column_name(&self) -> &str {
        self.metadata
            .id_column
            .as_deref()
            .expect("MemoryTableShell::new sets the id column")
    }

    /// The shell's query with a one-off window, leaving the shell untouched.
    fn windowed(&self, offset: usize, limit: Option<usize>) -> Query {
        self.query.clone().window(offset, limit)
    }

    /// The query rendered for `preview`.
    fn render_query(&self) -> String {
        let mut out = format!("scan {}", self.table.name());
        let mut clauses: Vec<String> = self
            .query
            .conditions
            .iter()
            .map(MemoryCondition::describe)
            .collect();
        if let Some(text) = &self.query.search {
            clauses.push(format!("search {text:?}"));
        }
        if !clauses.is_empty() {
            out.push_str(" WHERE ");
            out.push_str(&clauses.join(" AND "));
        }
        if !self.query.order.is_empty() {
            let keys: Vec<String> = self
                .query
                .order
                .iter()
                .map(|(field, dir)| match dir {
                    SortDirection::Ascending => format!("{field} ASC"),
                    SortDirection::Descending => format!("{field} DESC"),
                })
                .collect();
            out.push_str(" ORDER BY ");
            out.push_str(&keys.join(", "));
        }
        out
    }
}
