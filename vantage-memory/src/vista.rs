//! The Vista layer: a `TableShell` over a typed `Table<MemoryDB>`.

mod affinity;
mod factory;
mod narrow;
mod refs;
mod shell;
mod spec_table;
mod watch;

use affinity::Affinity;
use factory::Resolver;
use vantage_table::conditions::ConditionHandle;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{Capability, VistaCapabilities, VistaMetadata};

use crate::{MemoryDB, MemoryTableHandle};

pub use factory::{MemoryBlock, MemoryTableExtras, MemoryVistaFactory, MemoryVistaSpec};

/// A Vista shell over a typed memory table, which holds its query state and
/// its hooks. A clone copies the query; the rows live in the shared store.
/// Built by [`MemoryVistaFactory`].
#[derive(Clone)]
pub struct MemoryTableShell {
    table: Table<MemoryDB, EmptyEntity>,
    /// The store table behind `table`, outside any transaction.
    handle: MemoryTableHandle,
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,
    /// The quicksearch condition, replaced when the search changes.
    search: Option<ConditionHandle>,
    /// The building factory's specs, which describe traversed targets.
    specs: Resolver,
}

impl MemoryTableShell {
    /// `metadata` must name its id column.
    fn new(
        table: Table<MemoryDB, EmptyEntity>,
        handle: MemoryTableHandle,
        metadata: VistaMetadata,
        specs: Resolver,
    ) -> Self {
        Self {
            table,
            handle,
            metadata,
            capabilities: memory_capabilities(),
            search: None,
            specs,
        }
    }

    /// The store table this shell reads and writes.
    pub fn table(&self) -> &MemoryTableHandle {
        &self.handle
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

    fn id_column_name(&self) -> &str {
        self.metadata
            .id_column
            .as_deref()
            .expect("the factory sets the id column")
    }
}

/// Everything but cursors and native conditions: the store evaluates
/// every operator, search, order and window itself, and every write is
/// checked against the table's conditions, so writes stay inside the set.
fn memory_capabilities() -> VistaCapabilities {
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
        Capability::WritesReachHooks,
    ]
    .into_iter()
    .collect()
}
