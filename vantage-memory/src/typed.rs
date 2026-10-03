//! Typed layer: `MemoryDB` as a vantage `TableSource`.

mod aggregate;
pub mod convert;
pub mod operation;
mod table_source;

use vantage_core::{Result, error};
use vantage_expressions::{
    DeferredFn, Expression, ExpressiveEnum,
    traits::datasource::{DataSource, ExprDataSource},
};
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{Entity, Record};

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

    /// Store `record` as row `id` and return the stored row. The row must
    /// already exist when `replace`, and must not otherwise.
    pub(crate) fn put<E: Entity<AnyMemoryType>>(
        &self,
        table: &Table<Self, E>,
        id: &str,
        record: &Record<AnyMemoryType>,
        replace: bool,
    ) -> Result<Record<AnyMemoryType>> {
        let store = self.store_table(table)?;
        let record = to_cbor_record(record);
        let row = match replace {
            false => store.insert_as(id, record)?,
            true => store
                .replace(id, record)
                .ok_or_else(|| error!("Row not found", table = table.table_name(), id = id))?,
        };
        Ok(from_cbor_record(&row))
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
