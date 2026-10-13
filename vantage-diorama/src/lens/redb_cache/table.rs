//! [`RedbCacheTable`]: one Dio's rows in a redb table of the Lens's file.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use redb::{Database, ReadOnlyTable, ReadableTable, ReadableTableMetadata, Table};
use vantage_core::{Result, error};
use vantage_types::Record;

use super::{blocking, meta, status, table_def};
use crate::lens::cache_backend::{CacheStatus, CacheTable};

type Row = (Record<CborValue>, CacheStatus);

pub struct RedbCacheTable {
    db: Arc<Database>,
    name: String,
}

impl RedbCacheTable {
    pub(super) fn new(db: Arc<Database>, name: String) -> Self {
        Self { db, name }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// Run `read` over the redb table `table` in a read transaction. A table
    /// nothing was ever written to doesn't exist yet, and reads as `absent`.
    pub(super) async fn read<T, F>(&self, table: String, absent: T, read: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&ReadOnlyTable<&'static str, &'static [u8]>) -> Result<T> + Send + 'static,
    {
        let db = self.db.clone();
        blocking(move || {
            let txn = db
                .begin_read()
                .map_err(|e| error!("redb begin_read failed", detail = e.to_string()))?;
            let opened = match txn.open_table(table_def(&table)) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(absent),
                Err(e) => {
                    return Err(error!(
                        "redb open_table failed",
                        table = table,
                        detail = e.to_string()
                    ));
                }
            };
            read(&opened)
        })
        .await
    }

    /// Run `write` over the redb table `table` in one committed write
    /// transaction, creating the table if needed.
    pub(super) async fn write<F>(&self, table: String, write: F) -> Result<()>
    where
        F: for<'t> FnOnce(&mut Table<'t, &'static str, &'static [u8]>) -> Result<()>
            + Send
            + 'static,
    {
        let db = self.db.clone();
        blocking(move || {
            let txn = db
                .begin_write()
                .map_err(|e| error!("redb begin_write failed", detail = e.to_string()))?;
            {
                let mut opened = txn.open_table(table_def(&table)).map_err(|e| {
                    error!(
                        "redb open_table failed",
                        table = table,
                        detail = e.to_string()
                    )
                })?;
                write(&mut opened)?;
            }
            txn.commit()
                .map_err(|e| error!("redb commit failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn list_rows(&self) -> Result<IndexMap<String, Row>> {
        self.read(self.name.clone(), IndexMap::new(), |table| {
            let mut out = IndexMap::new();
            let iter = table
                .iter()
                .map_err(|e| error!("redb iter failed", detail = e.to_string()))?;
            for entry in iter {
                let (k, v) =
                    entry.map_err(|e| error!("redb iter entry failed", detail = e.to_string()))?;
                out.insert(k.value().to_string(), status::decode(v.value())?);
            }
            Ok(out)
        })
        .await
    }

    async fn get_row(&self, id: &str) -> Result<Option<Row>> {
        let id = id.to_string();
        self.read(self.name.clone(), None, move |table| {
            let row = table
                .get(id.as_str())
                .map_err(|e| error!("redb get failed", detail = e.to_string()))?;
            row.map(|v| status::decode(v.value())).transpose()
        })
        .await
    }

    async fn put_rows(&self, rows: Vec<(String, Vec<u8>)>) -> Result<()> {
        self.write(self.name.clone(), move |table| {
            for (id, bytes) in &rows {
                table
                    .insert(id.as_str(), bytes.as_slice())
                    .map_err(|e| error!("redb insert failed", detail = e.to_string()))?;
            }
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl CacheTable for RedbCacheTable {
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(self
            .list_rows()
            .await?
            .into_iter()
            .map(|(id, (record, _))| (id, record))
            .collect())
    }

    async fn get_value(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self.get_row(id).await?.map(|(record, _)| record))
    }

    async fn insert_value(&self, id: &str, record: &Record<CborValue>) -> Result<()> {
        self.insert_value_with_status(id, record, CacheStatus::Complete)
            .await
    }

    async fn insert_values(&self, rows: IndexMap<String, Record<CborValue>>) -> Result<()> {
        let encoded = rows
            .into_iter()
            .map(|(id, record)| Ok((id, status::encode(&record, CacheStatus::Complete)?)))
            .collect::<Result<Vec<_>>>()?;
        self.put_rows(encoded).await
    }

    async fn delete_value(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.write(self.name.clone(), move |table| {
            table
                .remove(id.as_str())
                .map_err(|e| error!("redb remove failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
    }

    /// The rows and the refresh time go in one transaction, so no refresh
    /// can stamp the table between the two and leave it empty but fresh.
    async fn clear(&self) -> Result<()> {
        let db = self.db.clone();
        let rows = self.name.clone();
        let meta_table = meta::meta_table_name(&self.name);
        blocking(move || {
            let txn = db
                .begin_write()
                .map_err(|e| error!("redb begin_write failed", detail = e.to_string()))?;
            {
                let mut opened = txn.open_table(table_def(&meta_table)).map_err(|e| {
                    error!(
                        "redb open_table failed",
                        table = meta_table,
                        detail = e.to_string()
                    )
                })?;
                opened
                    .remove(meta::REFRESHED_AT)
                    .map_err(|e| error!("redb remove failed", detail = e.to_string()))?;
            }
            txn.delete_table(table_def(&rows))
                .map_err(|e| error!("redb delete_table failed", detail = e.to_string()))?;
            txn.commit()
                .map_err(|e| error!("redb commit failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn count(&self) -> Result<i64> {
        self.read(self.name.clone(), 0, |table| {
            Ok(table
                .len()
                .map_err(|e| error!("redb len failed", detail = e.to_string()))?
                as i64)
        })
        .await
    }

    async fn meta_total(&self) -> Result<Option<u64>> {
        self.stored_total().await
    }

    async fn set_meta_total(&self, total: u64) -> Result<()> {
        self.store_total(total).await
    }

    async fn meta_refreshed_at(&self) -> Result<Option<SystemTime>> {
        self.stored_refreshed_at().await
    }

    async fn set_meta_refreshed_at(&self, at: SystemTime) -> Result<()> {
        self.store_refreshed_at(at).await
    }

    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Record<CborValue>,
        status: CacheStatus,
    ) -> Result<()> {
        let bytes = status::encode(record, status)?;
        self.put_rows(vec![(id.to_string(), bytes)]).await
    }

    async fn get_value_with_status(&self, id: &str) -> Result<Option<Row>> {
        self.get_row(id).await
    }

    async fn list_values_with_status(&self) -> Result<IndexMap<String, Row>> {
        self.list_rows().await
    }
}
