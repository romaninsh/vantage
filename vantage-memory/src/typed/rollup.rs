//! Roll-up columns: each row's aggregate over the child rows whose foreign
//! key holds its id, computed by `MemoryDB` on every read. A database runs
//! the same aggregate as a correlated subquery (`Table::with_rollup`).

mod fill;

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_table::table::{RollupOp, Table};
use vantage_types::Entity;

use super::MemoryDB;
use crate::eval::{MemoryCondition, Query, reads_columns};
use crate::types::AnyMemoryType;

/// Column `column` as `op` over `source` (`None` for `count`) of the rows of
/// `target` whose `foreign_key` holds the row's id and that match
/// `conditions`.
#[derive(Clone)]
#[non_exhaustive]
pub struct MemoryRollup {
    pub column: String,
    pub target: String,
    pub foreign_key: String,
    pub op: RollupOp,
    pub source: Option<String>,
    pub conditions: Vec<MemoryCondition>,
}

impl MemoryRollup {
    pub fn new(
        column: impl Into<String>,
        target: impl Into<String>,
        foreign_key: impl Into<String>,
        op: RollupOp,
    ) -> Self {
        Self {
            column: column.into(),
            target: target.into(),
            foreign_key: foreign_key.into(),
            op,
            source: None,
            conditions: Vec::new(),
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_conditions(
        mut self,
        conditions: impl IntoIterator<Item = MemoryCondition>,
    ) -> Self {
        self.conditions.extend(conditions);
        self
    }
}

pub trait MemoryRollupExt {
    /// Compute `rollup` on every read, from the target's rows as the table's
    /// datasource sees them: an open transaction's included. The column is
    /// computed, so writes drop a value given for it, and reads, conditions
    /// and orders see the computed value, never a stored one.
    ///
    /// The roll-up is kept on the table's `MemoryDB`, which its transaction
    /// and reference copies inherit; swapping in an unrelated `MemoryDB`
    /// drops it.
    fn with_memory_rollup(self, rollup: MemoryRollup) -> Self;
}

impl<E: Entity<AnyMemoryType> + 'static> MemoryRollupExt for Table<MemoryDB, E> {
    fn with_memory_rollup(self, rollup: MemoryRollup) -> Self {
        let column = rollup.column.clone();
        let db = self.data_source().with_rollup(self.table_name(), rollup);
        // `MemoryDB` fills the value as it reads; the lazy expression keeps
        // it and marks the column computed.
        let name = column.clone();
        self.with_data_source(db)
            .with_lazy_expression(&column, move |record| {
                let value = record
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| AnyMemoryType::untyped(CborValue::Null));
                async move { Ok(value) }
            })
    }
}

impl MemoryDB {
    /// A copy that computes `rollup` on table `table`, replacing one of the
    /// same column. Keyed by table name, as a reference target is built on
    /// its source's datasource.
    fn with_rollup(&self, table: &str, rollup: MemoryRollup) -> Self {
        let mut rollups: Vec<_> = self
            .rollups
            .iter()
            .filter(|(t, r)| !(t == table && r.column == rollup.column))
            .cloned()
            .collect();
        rollups.push((table.to_string(), rollup));
        Self {
            rollups: Arc::new(rollups),
            ..self.clone()
        }
    }

    fn rollups_of(&self, table: &str) -> Vec<&MemoryRollup> {
        self.rollups
            .iter()
            .filter(|(t, _)| t == table)
            .map(|(_, r)| r)
            .collect()
    }

    pub(crate) fn is_rollup(&self, table: &str, column: &str) -> bool {
        self.rollups_of(table).iter().any(|r| r.column == column)
    }

    /// The tables `table`'s roll-ups read, each once.
    pub(crate) fn rollup_targets(&self, table: &str) -> Vec<String> {
        let mut targets: Vec<String> = Vec::new();
        for rollup in self.rollups_of(table) {
            if !targets.contains(&rollup.target) {
                targets.push(rollup.target.clone());
            }
        }
        targets
    }

    /// Whether `q` reads one of `table`'s roll-up columns.
    fn reads_rollups(&self, table: &str, q: &Query) -> bool {
        let columns: Vec<&str> = self
            .rollups_of(table)
            .iter()
            .map(|r| r.column.as_str())
            .collect();
        reads_columns(q, &columns)
    }
}
