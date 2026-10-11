//! The Vista layer: a `TableShell` over one store table.

mod affinity;
pub mod factory;
mod narrow;
mod refs;
mod shell;
mod watch;
mod writes;

use std::sync::Arc;

use affinity::{Affinity, Untyped};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_vista::{Capability, SortDirection, VistaCapabilities, VistaMetadata};

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
    /// Declared columns whose type is only a display default.
    untyped: Arc<Untyped>,
    capabilities: VistaCapabilities,
}

impl MemoryTableShell {
    /// A shell over `table`. Cells of a column `metadata` declares with an
    /// integer or text type are stored and compared in that form; see
    /// [`with_untyped_columns`](Self::with_untyped_columns).
    pub fn new(table: MemoryTableHandle, metadata: VistaMetadata, catalog: Catalog) -> Self {
        Self {
            table,
            metadata,
            catalog,
            query: Query::new(),
            invariants: IndexMap::new(),
            untyped: Arc::default(),
            capabilities: Self::capabilities_for_memory(),
        }
    }

    /// This shell with `columns` treated as declared without a type, so
    /// their cells are stored as given whatever type the metadata names.
    pub fn with_untyped_columns(
        mut self,
        columns: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.untyped = Arc::new(columns.into_iter().map(Into::into).collect());
        self
    }

    fn affinity(&self, column: &str) -> Affinity {
        Affinity::of(&self.metadata, &self.untyped, self.id_column_name(), column)
    }

    /// Whether `column` has no declared type: not declared, or declared
    /// without a type.
    fn is_untyped(&self, column: &str) -> bool {
        column != self.id_column_name()
            && (self.untyped.contains(column) || !self.metadata.columns.contains_key(column))
    }

    /// Everything but cursors and native conditions: the store evaluates
    /// every operator, search, order and window itself, and every write is
    /// checked against the shell's query, so writes stay inside the set.
    pub fn capabilities_for_memory() -> VistaCapabilities {
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

    /// The query rendered for `preview`.
    fn render_query(&self) -> String {
        let mut out = format!("scan {}", self.table.name());
        let mut clauses: Vec<String> = self.query.conditions.iter().map(describe).collect();
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

/// Human-readable form of a condition, for `preview`.
fn describe(c: &MemoryCondition) -> String {
    let join = |list: &[MemoryCondition], sep: &str| {
        let parts: Vec<String> = list.iter().map(describe).collect();
        format!("({})", parts.join(sep))
    };
    match c {
        MemoryCondition::Cmp { path, op, value } => {
            format!("{path} {} {}", op.key_symbol(), render_value(value))
        }
        MemoryCondition::Search(text) => format!("search {text:?}"),
        MemoryCondition::And(all) => join(all, " AND "),
        MemoryCondition::Or(any) => join(any, " OR "),
        MemoryCondition::Not(inner) => format!("NOT {}", describe(inner)),
        MemoryCondition::Column(name) => name.clone(),
        MemoryCondition::Deferred(_) => "<deferred>".to_string(),
    }
}

/// A condition operand as a literal: text quoted, numbers and booleans bare,
/// arrays bracketed.
fn render_value(value: &CborValue) -> String {
    match value {
        CborValue::Text(s) => format!("{s:?}"),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        CborValue::Array(items) => {
            let items: Vec<String> = items.iter().map(render_value).collect();
            format!("[{}]", items.join(", "))
        }
        other => format!("{other:?}"),
    }
}
