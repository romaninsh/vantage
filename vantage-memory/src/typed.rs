//! Typed layer: `MemoryDB` as a vantage `TableSource`.

mod aggregate;
pub mod convert;
pub mod operation;
mod table_source;

use ciborium::Value as CborValue;
use vantage_core::{Result, VantageError, error};
use vantage_expressions::{
    DeferredFn, Expression, ExpressiveEnum,
    traits::datasource::{DataSource, ExprDataSource},
};
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{Entity, Record};

use crate::eval::matches_all;
use crate::store::{MemoryStore, MemoryTableHandle, Row, TableDef};
use crate::types::AnyMemoryType;
use convert::{from_cbor_record, table_query, to_cbor_record};

/// A `TableSource` over a shared [`MemoryStore`]. Clones share the store.
#[derive(Clone, Default)]
pub struct MemoryDB {
    store: MemoryStore,
}

impl MemoryDB {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_store(store: MemoryStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &MemoryStore {
        &self.store
    }

    /// The store table behind `table`. A table seen for the first time is
    /// created with the typed table's id column; an existing table whose id
    /// column differs from the typed table's is an error.
    pub(crate) fn store_table<E: Entity<AnyMemoryType>>(
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

    /// Rows matching the table's conditions and orders; `windowed` also
    /// applies its pagination.
    pub(crate) async fn rows<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        windowed: bool,
    ) -> Result<Vec<(String, Row)>> {
        let mut q = table_query(table).await?;
        if !windowed {
            q = q.window(0, None);
        }
        self.store_table(table)?.query(&q)
    }

    /// Whether the store row `row` is in `table`'s set.
    pub(crate) async fn in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        row: &Record<CborValue>,
    ) -> Result<bool> {
        matches_all(&table_query(table).await?, row)
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
    fn row_for<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: &Record<AnyMemoryType>,
    ) -> Record<CborValue> {
        let mut row = to_cbor_record(record);
        row.insert(self.id_column(table), CborValue::Text(id.to_string()));
        row
    }

    /// The row already stored under `id` when it is in the set; `outside`
    /// when it is held by a row outside the set.
    async fn stored_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
    ) -> Result<Option<Row>> {
        let Some(existing) = self.store_table(table)?.get(id) else {
            return Ok(None);
        };
        match self.in_set(table, &existing).await? {
            true => Ok(Some(existing)),
            false => Err(Self::outside(table.table_name(), id)),
        }
    }

    /// Insert `record` as `id` and return the stored row. An id already held
    /// by a row in the set returns that row; one held outside it is a
    /// `Conflict`.
    pub(crate) async fn insert_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: &Record<AnyMemoryType>,
    ) -> Result<Record<AnyMemoryType>> {
        if let Some(existing) = self.stored_in_set(table, id).await? {
            return Ok(from_cbor_record(&existing));
        }
        let row = self.row_for(table, id, record);
        if !self.in_set(table, &row).await? {
            return Err(Self::misfit(table.table_name(), id));
        }
        match self.store_table(table)?.insert_as(id, row) {
            Ok(stored) => Ok(from_cbor_record(&stored)),
            // Lost a race to another writer: take whatever it stored.
            Err(e) => match self.stored_in_set(table, id).await? {
                Some(existing) => Ok(from_cbor_record(&existing)),
                None => Err(e),
            },
        }
    }

    /// Replace row `id` with `record`, creating it when missing.
    pub(crate) async fn replace_in_set<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: &Record<AnyMemoryType>,
    ) -> Result<Record<AnyMemoryType>> {
        let existing = self.stored_in_set(table, id).await?;
        let row = self.row_for(table, id, record);
        if !self.in_set(table, &row).await? {
            return Err(Self::misfit(table.table_name(), id));
        }
        let store = self.store_table(table)?;
        let stored = match existing.and_then(|_| store.replace(id, row.clone())) {
            Some(stored) => stored,
            None => store.insert_as(id, row)?,
        };
        Ok(from_cbor_record(&stored))
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
        if expr.parameters.is_empty() {
            return Ok(AnyMemoryType::untyped(ciborium::Value::Text(
                expr.template.clone(),
            )));
        }
        if expr.parameters.len() != 1 {
            return Err(vantage_core::error!(
                "MemoryDB does not support multi-parameter expression execution"
            ));
        }
        match &expr.parameters[0] {
            ExpressiveEnum::Nested(inner) => match inner.parameters.as_slice() {
                [ExpressiveEnum::Nested(_)] => Err(vantage_core::error!(
                    "MemoryDB execute: only one level of nesting supported"
                )),
                [single] => scalar_of(single).await,
                _ => Err(vantage_core::error!(
                    "MemoryDB execute: nested expression must have exactly one parameter"
                )),
            },
            other => scalar_of(other).await,
        }
    }

    fn defer(&self, expr: Expression<AnyMemoryType>) -> DeferredFn<AnyMemoryType>
    where
        AnyMemoryType: Clone + Send + Sync + 'static,
    {
        let db = self.clone();
        DeferredFn::new(move || {
            let db = db.clone();
            let expr = expr.clone();
            Box::pin(async move { Ok(ExpressiveEnum::Scalar(db.execute(&expr).await?)) })
        })
    }
}

/// The scalar behind a scalar or deferred parameter.
async fn scalar_of(param: &ExpressiveEnum<AnyMemoryType>) -> vantage_core::Result<AnyMemoryType> {
    match param {
        ExpressiveEnum::Scalar(v) => Ok(v.clone()),
        ExpressiveEnum::Deferred(d) => match d.call().await? {
            ExpressiveEnum::Scalar(v) => Ok(v),
            _ => Err(vantage_core::error!(
                "Deferred resolved to non-scalar in MemoryDB execute"
            )),
        },
        ExpressiveEnum::Nested(_) => Err(vantage_core::error!(
            "MemoryDB execute: only one level of nesting supported"
        )),
    }
}
