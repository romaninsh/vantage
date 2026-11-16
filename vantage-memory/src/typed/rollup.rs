//! Roll-up columns: each row's aggregate over the child rows whose foreign
//! key holds its id, computed by `MemoryDB` when a read wants the column. A
//! database runs the same aggregate as a correlated subquery
//! (`Table::with_rollup`).

mod fill;

use std::borrow::Cow;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
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

/// Builds a roll-up for the `MemoryDB` doing a read. See
/// [`MemoryRollupExt::with_lazy_memory_rollup`].
type LazyPlan = Arc<dyn Fn(&MemoryDB) -> Result<MemoryRollup> + Send + Sync>;

/// A roll-up column as a table's `MemoryDB` keeps it.
#[derive(Clone)]
pub(super) struct RollupColumn {
    column: String,
    plan: Plan,
}

#[derive(Clone)]
enum Plan {
    Fixed(MemoryRollup),
    Lazy(LazyPlan),
}

impl RollupColumn {
    /// The roll-up `db` computes for this column: a lazy plan runs now.
    fn resolve(&self, db: &MemoryDB) -> Result<Cow<'_, MemoryRollup>> {
        match &self.plan {
            Plan::Fixed(rollup) => Ok(Cow::Borrowed(rollup)),
            Plan::Lazy(plan) => {
                let mut rollup = plan(db)?;
                rollup.column = self.column.clone();
                Ok(Cow::Owned(rollup))
            }
        }
    }
}

pub trait MemoryRollupExt {
    /// Compute `rollup` on every read that wants its column, from the
    /// target's rows as the table's datasource sees them: an open
    /// transaction's included. The column is computed, so writes drop a
    /// value given for it, and reads, conditions and orders see the computed
    /// value, never a stored one.
    ///
    /// The roll-up is kept on the table's `MemoryDB`, which its transaction
    /// and reference copies inherit; swapping in an unrelated `MemoryDB`
    /// drops it.
    fn with_memory_rollup(self, rollup: MemoryRollup) -> Self;

    /// [`with_memory_rollup`](Self::with_memory_rollup) for a roll-up that
    /// is known only once its target table is built. `plan` runs on each
    /// read that wants `column`, never when the table is built, and gets the
    /// `MemoryDB` doing the read: build the target on it, so the target's
    /// conditions read through the same transaction. Its error fails the
    /// read, and [`MemoryDB::check_rollups`].
    fn with_lazy_memory_rollup(
        self,
        column: &str,
        plan: impl Fn(&MemoryDB) -> Result<MemoryRollup> + Send + Sync + 'static,
    ) -> Self;
}

impl<E: Entity<AnyMemoryType> + 'static> MemoryRollupExt for Table<MemoryDB, E> {
    fn with_memory_rollup(self, rollup: MemoryRollup) -> Self {
        let column = rollup.column.clone();
        with_rollup_column(self, column, Plan::Fixed(rollup))
    }

    fn with_lazy_memory_rollup(
        self,
        column: &str,
        plan: impl Fn(&MemoryDB) -> Result<MemoryRollup> + Send + Sync + 'static,
    ) -> Self {
        with_rollup_column(self, column.to_string(), Plan::Lazy(Arc::new(plan)))
    }
}

fn with_rollup_column<E: Entity<AnyMemoryType> + 'static>(
    table: Table<MemoryDB, E>,
    column: String,
    plan: Plan,
) -> Table<MemoryDB, E> {
    let rollup = RollupColumn {
        column: column.clone(),
        plan,
    };
    let db = table.data_source().with_rollup(table.table_name(), rollup);
    // `MemoryDB` fills the value as it reads; the lazy expression keeps
    // it and marks the column computed.
    let name = column.clone();
    table
        .with_data_source(db)
        .with_lazy_expression(&column, move |record| {
            let value = record
                .get(&name)
                .cloned()
                .unwrap_or_else(|| AnyMemoryType::untyped(CborValue::Null));
            async move { Ok(value) }
        })
}

/// Which of a table's roll-up columns a read fills, besides those its
/// conditions, search and orders read.
#[derive(Clone, Copy)]
pub(crate) enum Fill<'a> {
    /// Those in the table's active columns: a read of rows.
    Active,
    /// This one, when it is a roll-up: a read of one column, as a condition
    /// on a related table makes.
    Column(&'a str),
    /// No other: a read that returns no cells.
    Nothing,
}

impl MemoryDB {
    /// A copy that computes `rollup` on table `table`, replacing one of the
    /// same column. Keyed by table name, as a reference target is built on
    /// its source's datasource.
    fn with_rollup(&self, table: &str, rollup: RollupColumn) -> Self {
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

    fn rollups_of(&self, table: &str) -> impl Iterator<Item = &RollupColumn> {
        self.rollups
            .iter()
            .filter(move |(t, _)| t == table)
            .map(|(_, r)| r)
    }

    pub(crate) fn is_rollup(&self, table: &str, column: &str) -> bool {
        self.rollups_of(table).any(|r| r.column == column)
    }

    /// Resolve every roll-up of `table` once and discard it, so one that
    /// can't be built (an unknown child column, say) fails here instead of
    /// at the first read. Resolving builds the related tables: call this on
    /// a finished table, not while another is being built.
    pub fn check_rollups<E: Entity<AnyMemoryType>>(&self, table: &Table<Self, E>) -> Result<()> {
        for rollup in self.rollups_of(table.table_name()) {
            rollup.resolve(self)?;
        }
        Ok(())
    }

    /// The tables `table`'s roll-ups read, each once.
    pub(crate) fn rollup_targets(&self, table: &str) -> Result<Vec<String>> {
        let mut targets: Vec<String> = Vec::new();
        for rollup in self.rollups_of(table) {
            let target = rollup.resolve(self)?.target.clone();
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    /// The roll-up columns of `table` that `q` reads, and those `fill` asks
    /// for besides.
    fn wanted_rollups<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        q: &Query,
        fill: Fill<'_>,
    ) -> (Vec<&RollupColumn>, Vec<&RollupColumn>) {
        let (read, other): (Vec<_>, Vec<_>) = self
            .rollups_of(table.table_name())
            .partition(|r| reads_columns(q, &[r.column.as_str()]));
        let asked = other
            .into_iter()
            .filter(|r| match fill {
                Fill::Active => table.is_active_column(&r.column),
                Fill::Column(column) => r.column == column,
                Fill::Nothing => false,
            })
            .collect();
        (read, asked)
    }
}
