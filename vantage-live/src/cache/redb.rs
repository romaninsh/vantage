//! redb-backed cache. Persists `CachedRows` on disk so cache state
//! survives process restarts.
//!
//! ## Layout
//!
//! One redb file (`vlive.redb`) inside the caller-supplied folder.
//! Inside the file, **one redb table per `cache_key`**, namespaced as
//! `__vlive__{cache_key}` so cache tables can't collide with whatever
//! else the user keeps in the same folder. Sub-keys inside that table
//! are the part after the first `/` of the cache key (`page_1`,
//! `id/foo`, etc.). Values are CBOR-encoded `CachedRows`.
//!
//! Per-table layout makes the hot invalidation path cheap:
//! `invalidate_prefix("clients")` matches the whole cache_key →
//! `delete_table("__vlive__clients")` is O(1)-ish (just unlinks the
//! tree root). Sub-prefix invalidates fall back to scan + delete inside
//! the table, but v1 of LiveTable always passes the bare cache_key.
//!
//! ## Concurrency
//!
//! redb takes an OS-level exclusive lock on its file — only one
//! process can open the cache folder at a time. Trying to open the
//! same folder from a second process returns
//! `redb::DatabaseError::DatabaseAlreadyOpen`. If you need cross-process
//! cache sharing, this isn't the layer for it (network cache: Redis).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use std::time::SystemTime;
use vantage_core::{Result, error};
use vantage_types::Record;

use super::{Cache, CachedRows};

const CACHE_FILE: &str = "vlive.redb";
const TABLE_PREFIX: &str = "__vlive__";

/// Redb-backed cache. Cheap to clone — the inner `Arc<Database>` is
/// shared.
#[derive(Clone)]
pub struct RedbCache {
    db: Arc<Database>,
    folder: PathBuf,
}

impl std::fmt::Debug for RedbCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedbCache")
            .field("folder", &self.folder)
            .finish()
    }
}

impl RedbCache {
    /// Open or create a cache in `folder`. Creates the folder if it
    /// doesn't exist; opens (or creates) `vlive.redb` inside.
    ///
    /// Returns an error if another process already has the cache open
    /// (redb's exclusive file lock — see module docs).
    pub fn open(folder: impl AsRef<Path>) -> Result<Self> {
        let folder = folder.as_ref().to_path_buf();
        std::fs::create_dir_all(&folder)
            .map_err(|e| error!("Failed to create cache folder", details = e.to_string()))?;
        let path = folder.join(CACHE_FILE);
        let db = Database::create(&path)
            .map_err(|e| error!("Failed to open cache redb", details = e.to_string()))?;
        Ok(Self {
            db: Arc::new(db),
            folder,
        })
    }

    /// Folder this cache lives in. Useful for diagnostics; do not use
    /// to open a second handle while this one is alive.
    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

/// Split a cache key like `clients/page_1` into `("clients", "page_1")`.
/// If there's no `/`, the whole key is treated as the root and the
/// sub-key is empty (the `__root__` sentinel is used internally so
/// every entry has a non-empty redb sub-key).
fn split_key(key: &str) -> (&str, String) {
    match key.find('/') {
        Some(i) => (&key[..i], key[i + 1..].to_string()),
        None => (key, String::from("__root__")),
    }
}

fn redb_table_name(root: &str) -> String {
    format!("{}{}", TABLE_PREFIX, root)
}

fn cache_table_def(name: &str) -> TableDefinition<'_, &'static str, &'static [u8]> {
    TableDefinition::new(name)
}

fn encode_rows(rows: &CachedRows) -> Result<Vec<u8>> {
    // CachedRows isn't directly Serialize, so encode field-by-field as a
    // CBOR map. fetched_at is stored as seconds-since-epoch so the
    // representation is portable / compact.
    let secs = rows
        .fetched_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let row_pairs: Vec<(CborValue, CborValue)> = rows
        .rows
        .iter()
        .map(|(id, rec)| {
            let entries: Vec<(CborValue, CborValue)> = rec
                .iter()
                .map(|(k, v)| (CborValue::Text(k.clone()), v.clone()))
                .collect();
            (CborValue::Text(id.clone()), CborValue::Map(entries))
        })
        .collect();

    let envelope = CborValue::Map(vec![
        (
            CborValue::Text("fetched_at".into()),
            CborValue::Integer(secs.into()),
        ),
        (CborValue::Text("rows".into()), CborValue::Map(row_pairs)),
    ]);

    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&envelope, &mut bytes)
        .map_err(|e| error!("CBOR encode failed", details = e.to_string()))?;
    Ok(bytes)
}

fn decode_rows(bytes: &[u8]) -> Result<CachedRows> {
    let parsed: CborValue = ciborium::de::from_reader(bytes)
        .map_err(|e| error!("CBOR decode failed", details = e.to_string()))?;
    let mut secs: i64 = 0;
    let mut rows: IndexMap<String, Record<CborValue>> = IndexMap::new();

    let pairs = match parsed {
        CborValue::Map(p) => p,
        _ => return Err(error!("RedbCache: expected envelope to be a map")),
    };
    for (k, v) in pairs {
        let key = match k {
            CborValue::Text(s) => s,
            _ => continue,
        };
        match (key.as_str(), v) {
            ("fetched_at", CborValue::Integer(i)) => {
                secs = i64::try_from(i).unwrap_or(0);
            }
            ("rows", CborValue::Map(row_pairs)) => {
                for (rk, rv) in row_pairs {
                    let id = match rk {
                        CborValue::Text(s) => s,
                        _ => continue,
                    };
                    let mut rec: Record<CborValue> = Record::new();
                    if let CborValue::Map(field_pairs) = rv {
                        for (fk, fv) in field_pairs {
                            if let CborValue::Text(name) = fk {
                                rec.insert(name, fv);
                            }
                        }
                    }
                    rows.insert(id, rec);
                }
            }
            _ => {}
        }
    }

    let fetched_at =
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs.try_into().unwrap_or(0));
    Ok(CachedRows { rows, fetched_at })
}

#[async_trait]
impl Cache for RedbCache {
    async fn get(&self, key: &str) -> Result<Option<CachedRows>> {
        let (root, sub) = split_key(key);
        let table_name = redb_table_name(root);

        let txn = self
            .db
            .begin_read()
            .map_err(|e| error!("redb begin_read failed", details = e.to_string()))?;
        let table = match txn.open_table(cache_table_def(&table_name)) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => {
                return Err(error!(
                    "Failed to open cache table",
                    table = table_name,
                    details = e.to_string()
                ));
            }
        };
        let bytes = table
            .get(sub.as_str())
            .map_err(|e| error!("redb cache get failed", details = e.to_string()))?;
        match bytes {
            Some(b) => Ok(Some(decode_rows(b.value())?)),
            None => Ok(None),
        }
    }

    async fn put(&self, key: &str, rows: CachedRows) -> Result<()> {
        let (root, sub) = split_key(key);
        let table_name = redb_table_name(root);
        let bytes = encode_rows(&rows)?;

        let txn = self
            .db
            .begin_write()
            .map_err(|e| error!("redb begin_write failed", details = e.to_string()))?;
        {
            let mut table = txn.open_table(cache_table_def(&table_name)).map_err(|e| {
                error!(
                    "Failed to open cache table for write",
                    table = table_name,
                    details = e.to_string()
                )
            })?;
            table
                .insert(sub.as_str(), bytes.as_slice())
                .map_err(|e| error!("redb cache insert failed", details = e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| error!("redb cache commit failed", details = e.to_string()))?;
        Ok(())
    }

    async fn invalidate_prefix(&self, prefix: &str) -> Result<()> {
        // The fast path: prefix is exactly a cache_key (no '/') →
        // drop the whole table.
        if !prefix.contains('/') {
            let table_name = redb_table_name(prefix);
            let txn = self
                .db
                .begin_write()
                .map_err(|e| error!("redb begin_write failed", details = e.to_string()))?;
            // delete_table returns Ok(false) if the table didn't exist,
            // Ok(true) if it was dropped. Either way, swallow.
            let _ = txn
                .delete_table(cache_table_def(&table_name))
                .map_err(|e| error!("delete_table failed", details = e.to_string()))?;
            txn.commit()
                .map_err(|e| error!("redb cache commit failed", details = e.to_string()))?;
            return Ok(());
        }

        // Sub-prefix: scan inside the appropriate table and delete the
        // matching sub-keys. Used by future surgical-invalidation paths;
        // v1 LiveTable doesn't currently call this.
        let (root, sub_prefix) = split_key(prefix);
        let table_name = redb_table_name(root);

        let txn = self
            .db
            .begin_write()
            .map_err(|e| error!("redb begin_write failed", details = e.to_string()))?;
        {
            let mut table = match txn.open_table(cache_table_def(&table_name)) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(e) => {
                    return Err(error!(
                        "Failed to open cache table for invalidate",
                        table = table_name,
                        details = e.to_string()
                    ));
                }
            };
            // Collect keys matching prefix first (can't mutate during iter).
            let mut to_delete: Vec<String> = Vec::new();
            {
                let iter = table
                    .iter()
                    .map_err(|e| error!("redb cache iter failed", details = e.to_string()))?;
                for entry in iter {
                    let (k, _) = entry.map_err(|e| {
                        error!("redb cache iter entry failed", details = e.to_string())
                    })?;
                    if k.value().starts_with(&sub_prefix) {
                        to_delete.push(k.value().to_string());
                    }
                }
            }
            for k in to_delete {
                table
                    .remove(k.as_str())
                    .map_err(|e| error!("redb cache remove failed", details = e.to_string()))?;
            }
            // If the table is now empty, drop it so we don't leak empties.
            if ReadableTableMetadata::len(&table).map_err(|e: redb::StorageError| {
                error!("redb cache len failed", details = e.to_string())
            })? == 0
            {
                drop(table);
                let _ = txn
                    .delete_table(cache_table_def(&table_name))
                    .map_err(|e| error!("delete_table failed", details = e.to_string()))?;
            }
        }
        txn.commit()
            .map_err(|e| error!("redb cache commit failed", details = e.to_string()))?;
        Ok(())
    }
}
