//! Redb-backed [`CacheBackend`] — one file per Lens, one table per Dio.
//!
//! Rows are stored as `id (str) -> cbor body (bytes)`. Encoding uses
//! [`ciborium`] directly; the cache doesn't know anything about the
//! master Vista's schema, just opaque records keyed by id.
//!
//! Redb operations are synchronous, so the async methods wrap each unit
//! of work in `tokio::task::spawn_blocking`. The cache shares the
//! `Arc<redb::Database>` across opened tables.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use vantage_core::{Result, error};
use vantage_types::Record;

use super::cache_backend::{CacheBackend, CacheTable};

/// `RedbCache` opens (or creates) a redb database at the configured
/// path. Each Dio under the owning Lens claims a named table within it.
pub struct RedbCache {
    db: Arc<Database>,
    path: PathBuf,
    opened: Mutex<IndexMap<String, Arc<RedbCacheTable>>>,
}

impl RedbCache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let db = Database::create(&path).map_err(|e| {
            error!(
                "Failed to open redb cache",
                path = path.display(),
                detail = e.to_string()
            )
        })?;
        Ok(Self {
            db: Arc::new(db),
            path,
            opened: Mutex::new(IndexMap::new()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl CacheBackend for RedbCache {
    async fn open_table(&self, name: &str) -> Result<Arc<dyn CacheTable>> {
        let mut opened = self.opened.lock().expect("RedbCache mutex poisoned");
        if let Some(existing) = opened.get(name) {
            return Ok(existing.clone() as Arc<dyn CacheTable>);
        }
        let table = Arc::new(RedbCacheTable {
            db: self.db.clone(),
            name: name.to_string(),
        });
        opened.insert(name.to_string(), table.clone());
        Ok(table as Arc<dyn CacheTable>)
    }

    fn name(&self) -> &'static str {
        "redb"
    }
}

pub struct RedbCacheTable {
    db: Arc<Database>,
    name: String,
}

impl RedbCacheTable {
    fn table_def(&self) -> TableDefinition<'_, &'static str, &'static [u8]> {
        TableDefinition::new(&self.name)
    }
}

fn encode(record: &Record<CborValue>) -> Result<Vec<u8>> {
    let map: Vec<(CborValue, CborValue)> = record
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), v.clone()))
        .collect();
    let value = CborValue::Map(map);
    let mut buf = Vec::with_capacity(64);
    ciborium::into_writer(&value, &mut buf)
        .map_err(|e| error!("Failed to encode cache row", detail = e.to_string()))?;
    Ok(buf)
}

fn decode(bytes: &[u8]) -> Result<Record<CborValue>> {
    let value: CborValue = ciborium::from_reader(bytes)
        .map_err(|e| error!("Failed to decode cache row", detail = e.to_string()))?;
    match value {
        CborValue::Map(entries) => {
            let mut record = Record::new();
            for (k, v) in entries {
                let CborValue::Text(key) = k else {
                    return Err(error!("cache row had non-text key"));
                };
                record.insert(key, v);
            }
            Ok(record)
        }
        other => Err(error!(
            "cache row not a cbor map",
            shape = format!("{:?}", other)
        )),
    }
}

#[async_trait]
impl CacheTable for RedbCacheTable {
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let db = self.db.clone();
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || -> Result<IndexMap<String, Record<CborValue>>> {
            let txn = db
                .begin_read()
                .map_err(|e| error!("redb begin_read failed", detail = e.to_string()))?;
            let table =
                match txn.open_table(TableDefinition::<&'static str, &'static [u8]>::new(&name)) {
                    Ok(t) => t,
                    Err(redb::TableError::TableDoesNotExist(_)) => return Ok(IndexMap::new()),
                    Err(e) => {
                        return Err(error!(
                            "redb open_table failed",
                            table = name,
                            detail = e.to_string()
                        ));
                    }
                };
            let mut out = IndexMap::new();
            let iter = table
                .iter()
                .map_err(|e| error!("redb iter failed", detail = e.to_string()))?;
            for entry in iter {
                let (k, v) =
                    entry.map_err(|e| error!("redb iter entry failed", detail = e.to_string()))?;
                let id = k.value().to_string();
                let record = decode(v.value())?;
                out.insert(id, record);
            }
            Ok(out)
        })
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
    }

    async fn get_value(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let db = self.db.clone();
        let name = self.name.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Record<CborValue>>> {
            let txn = db
                .begin_read()
                .map_err(|e| error!("redb begin_read failed", detail = e.to_string()))?;
            let table =
                match txn.open_table(TableDefinition::<&'static str, &'static [u8]>::new(&name)) {
                    Ok(t) => t,
                    Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
                    Err(e) => {
                        return Err(error!(
                            "redb open_table failed",
                            table = name,
                            detail = e.to_string()
                        ));
                    }
                };
            let row = table
                .get(id.as_str())
                .map_err(|e| error!("redb get failed", detail = e.to_string()))?;
            row.map(|v| decode(v.value())).transpose()
        })
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
    }

    async fn insert_value(&self, id: &str, record: &Record<CborValue>) -> Result<()> {
        let db = self.db.clone();
        let name = self.name.clone();
        let id = id.to_string();
        let bytes = encode(record)?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let txn = db
                .begin_write()
                .map_err(|e| error!("redb begin_write failed", detail = e.to_string()))?;
            {
                let mut table = txn
                    .open_table(TableDefinition::<&'static str, &'static [u8]>::new(&name))
                    .map_err(|e| {
                        error!(
                            "redb open_table failed",
                            table = name,
                            detail = e.to_string()
                        )
                    })?;
                table
                    .insert(id.as_str(), bytes.as_slice())
                    .map_err(|e| error!("redb insert failed", detail = e.to_string()))?;
            }
            txn.commit()
                .map_err(|e| error!("redb commit failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
    }

    async fn insert_values(&self, rows: IndexMap<String, Record<CborValue>>) -> Result<()> {
        let db = self.db.clone();
        let name = self.name.clone();
        let encoded: Vec<(String, Vec<u8>)> = rows
            .into_iter()
            .map(|(id, record)| Ok::<_, vantage_core::VantageError>((id, encode(&record)?)))
            .collect::<Result<Vec<_>>>()?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let txn = db
                .begin_write()
                .map_err(|e| error!("redb begin_write failed", detail = e.to_string()))?;
            {
                let mut table = txn
                    .open_table(TableDefinition::<&'static str, &'static [u8]>::new(&name))
                    .map_err(|e| {
                        error!(
                            "redb open_table failed",
                            table = name,
                            detail = e.to_string()
                        )
                    })?;
                for (id, bytes) in &encoded {
                    table
                        .insert(id.as_str(), bytes.as_slice())
                        .map_err(|e| error!("redb insert failed", detail = e.to_string()))?;
                }
            }
            txn.commit()
                .map_err(|e| error!("redb commit failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
    }

    async fn delete_value(&self, id: &str) -> Result<()> {
        let db = self.db.clone();
        let name = self.name.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let txn = db
                .begin_write()
                .map_err(|e| error!("redb begin_write failed", detail = e.to_string()))?;
            {
                let mut table = txn
                    .open_table(TableDefinition::<&'static str, &'static [u8]>::new(&name))
                    .map_err(|e| {
                        error!(
                            "redb open_table failed",
                            table = name,
                            detail = e.to_string()
                        )
                    })?;
                table
                    .remove(id.as_str())
                    .map_err(|e| error!("redb remove failed", detail = e.to_string()))?;
            }
            txn.commit()
                .map_err(|e| error!("redb commit failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
    }

    async fn clear(&self) -> Result<()> {
        let db = self.db.clone();
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let txn = db
                .begin_write()
                .map_err(|e| error!("redb begin_write failed", detail = e.to_string()))?;
            txn.delete_table(TableDefinition::<&'static str, &'static [u8]>::new(&name))
                .map_err(|e| error!("redb delete_table failed", detail = e.to_string()))?;
            txn.commit()
                .map_err(|e| error!("redb commit failed", detail = e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
    }

    async fn count(&self) -> Result<i64> {
        let db = self.db.clone();
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || -> Result<i64> {
            let txn = db
                .begin_read()
                .map_err(|e| error!("redb begin_read failed", detail = e.to_string()))?;
            let table =
                match txn.open_table(TableDefinition::<&'static str, &'static [u8]>::new(&name)) {
                    Ok(t) => t,
                    Err(redb::TableError::TableDoesNotExist(_)) => return Ok(0),
                    Err(e) => {
                        return Err(error!(
                            "redb open_table failed",
                            table = name,
                            detail = e.to_string()
                        ));
                    }
                };
            Ok(table
                .len()
                .map_err(|e| error!("redb len failed", detail = e.to_string()))?
                as i64)
        })
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
    }
}
