//! Typed layer: `MemoryDB` as a vantage `TableSource`.

mod aggregate;
pub mod convert;
pub mod operation;
mod rollup;
mod table_source;

pub(crate) use rollup::Fill;
pub use rollup::{MemoryRollup, MemoryRollupExt};

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, VantageError, error};
use vantage_expressions::{
    DeferredFn, Expression, defer_execute, execute_strict,
    traits::datasource::{DataSource, ExprDataSource},
};
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{Entity, Record};
use vantage_vista::Transaction;

use crate::eval::{Query, matches_all};
use crate::store::{MemoryStore, MemoryTableHandle, Row, TableDef};
use crate::tx::MemoryTx;
use crate::types::AnyMemoryType;
use convert::{from_cbor_record, table_query, to_cbor_record};

/// A `TableSource` over a shared [`MemoryStore`]. Clones share the store.
#[derive(Clone, Default)]
pub struct MemoryDB {
    store: MemoryStore,
    tx: Option<Arc<MemoryTx>>,
    /// Roll-up columns, by table name.
    rollups: Arc<Vec<(String, rollup::RollupColumn)>>,
}

impl MemoryDB {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_store(store: MemoryStore) -> Self {
        Self {
            store,
            ..Self::default()
        }
    }

    /// A copy whose reads and writes go through `tx`. Nothing is claimed or
    /// copied until the first read or write.
    pub fn in_transaction(&self, tx: &Transaction) -> Result<MemoryDB> {
        let joined = tx.participant(MemoryTx::key(&self.store), || {
            MemoryTx::new(&self.store, tx)
        })?;
        Ok(Self {
            tx: Some(joined),
            ..self.clone()
        })
    }

    /// A copy bound to `tx` as an [unclaimed](MemoryTx::unclaimed) store
    /// transaction: its writes land whole at commit, and it neither waits
    /// on nor blocks a transaction holding the store. `tx` must be fresh.
    pub(crate) fn in_unclaimed_transaction(&self, tx: &Transaction) -> Result<MemoryDB> {
        let joined = tx.participant(MemoryTx::key(&self.store), || {
            MemoryTx::unclaimed(&self.store, tx)
        })?;
        Ok(Self {
            tx: Some(joined),
            ..self.clone()
        })
    }

    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    /// The table every read and write of `table` goes to: the store's, or
    /// the transaction's copy of it when bound.
    pub(crate) async fn store_table<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<MemoryTableHandle> {
        let live = self.settle(self.live_table(table))?;
        match &self.tx {
            Some(tx) => tx.work_copy(&live).await,
            None => Ok(live),
        }
    }

    /// Runs the write `op` on the table every write of `table` goes to.
    /// Inside a transaction it runs under the transaction's lock, so a write
    /// still in flight when another task commits fails ("transaction
    /// finished") instead of landing on a copy already replayed.
    pub(crate) async fn write<E: Entity<AnyMemoryType>, R>(
        &self,
        table: &Table<Self, E>,
        op: impl FnOnce(&MemoryTableHandle) -> Result<R> + Send,
    ) -> Result<R> {
        let live = self.settle(self.live_table(table))?;
        match &self.tx {
            Some(tx) => tx.run(&live, op).await,
            None => op(&live),
        }
    }

    /// Passes `out` through; inside a transaction an `Err` fails the commit.
    pub(crate) fn settle<R>(&self, out: Result<R>) -> Result<R> {
        match &self.tx {
            Some(tx) => tx.settle(out),
            None => out,
        }
    }

    /// The store table behind `table`. A table seen for the first time is
    /// created with the typed table's id column; an existing table whose id
    /// column differs from the typed table's is an error.
    pub(crate) fn live_table<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<MemoryTableHandle> {
        let Some(id) = table.id_field() else {
            return Ok(self.store.table(table.table_name()));
        };
        let id_column = ColumnLike::name(id);
        let store = self.store.define(
            table.table_name(),
            TableDef {
                id_column: id_column.to_string(),
                ..TableDef::default()
            },
        );
        if store.id_column() != id_column {
            return Err(error!(
                "Table already exists with another id column",
                table = table.table_name(),
                existing = store.id_column(),
                requested = id_column
            ));
        }
        Ok(store)
    }

    /// Rows matching the table's conditions and orders, with the roll-ups
    /// `fill` asks for; `windowed` also applies its pagination.
    pub(crate) async fn rows<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        windowed: bool,
        fill: Fill<'_>,
    ) -> Result<Vec<(String, Row)>> {
        self.settle(
            async {
                let mut q = table_query(self, table).await?;
                if !windowed {
                    q = q.window(0, None);
                }
                self.query_rows(table, &q, fill).await
            }
            .await,
        )
    }

    /// Whether the store row `row` is in `table`'s set.
    pub(crate) async fn in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        row: &Record<CborValue>,
    ) -> Result<bool> {
        matches_all(&table_query(self, table).await?, row)
    }

    pub(crate) fn outside(table: &str, id: &str) -> VantageError {
        error!(
            "id is held by a row outside this set",
            table = table,
            id = id
        )
        .mark_conflict()
    }

    pub(crate) fn misfit(table: &str, id: &str) -> VantageError {
        error!("record does not belong to this set", table = table, id = id).mark_conflict()
    }

    /// `record` as a store row for `id`: the id column set to `id`.
    pub(crate) fn row_for<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: &Record<AnyMemoryType>,
    ) -> Record<CborValue> {
        let mut row = to_cbor_record(record);
        row.insert(self.id_column(table), CborValue::Text(id.to_string()));
        row
    }

    /// Row `id` with its roll-ups filled when it is in the set: a condition
    /// on a roll-up column reads the computed value, as reads do.
    pub(crate) async fn fit<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        row: Row,
    ) -> Result<Option<Row>> {
        let q = table_query(self, table).await?;
        let row = self.filled_row(table, &q, id, row).await?;
        Ok(matches_all(&q, &row)?.then_some(row))
    }

    /// Stored row `id` as a read returns it.
    pub(crate) async fn returned<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        stored: Row,
    ) -> Result<Record<AnyMemoryType>> {
        let row = self.filled_row(table, &Query::new(), id, stored).await?;
        Ok(from_cbor_record(&row))
    }

    /// The row already stored under `id`, roll-ups filled, when it is in the
    /// set; `outside` when it is held by a row outside the set.
    pub(crate) async fn row_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<Option<Row>> {
        let Some(existing) = self.store_table(table).await?.get(id) else {
            return Ok(None);
        };
        match self.fit(table, id, existing).await? {
            Some(row) => Ok(Some(row)),
            None => Err(Self::outside(table.table_name(), id)),
        }
    }

    /// The typed table's id field, else the store table's id column.
    pub(crate) fn id_column<E: Entity<AnyMemoryType>>(&self, table: &Table<Self, E>) -> String {
        match table.id_field() {
            Some(id) => ColumnLike::name(id).to_string(),
            None => self.store.table(table.table_name()).id_column().to_string(),
        }
    }
}

impl DataSource for MemoryDB {}

/// Resolves deferred parameters only (used by `column_table_values_expr`
/// and relationship traversal); there is no general expression engine.
impl ExprDataSource<AnyMemoryType> for MemoryDB {
    async fn execute(
        &self,
        expr: &Expression<AnyMemoryType>,
    ) -> vantage_core::Result<AnyMemoryType> {
        execute_strict(
            expr,
            |template| AnyMemoryType::untyped(CborValue::Text(template.to_string())),
            "MemoryDB",
        )
        .await
    }

    fn defer(&self, expr: Expression<AnyMemoryType>) -> DeferredFn<AnyMemoryType>
    where
        AnyMemoryType: Clone + Send + Sync + 'static,
    {
        defer_execute(self, expr)
    }
}
