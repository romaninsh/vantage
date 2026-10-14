use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use crate::lens::cache_backend::{CacheStatus, CacheTable};

use super::Sugar;

pub(crate) struct SugaredCache {
    inner: Arc<dyn CacheTable>,
    sugar: Sugar,
    memo: Mutex<HashMap<String, (u64, Record<CborValue>)>>,
    /// Messages already logged, so a failing script logs once per cause.
    logged: Mutex<HashSet<String>>,
}

impl SugaredCache {
    pub(crate) fn wrap(inner: Arc<dyn CacheTable>, sugar: Sugar) -> Arc<dyn CacheTable> {
        Arc::new(SugaredCache {
            inner,
            sugar,
            memo: Mutex::default(),
            logged: Mutex::default(),
        })
    }

    fn log_once(&self, msg: String) {
        if self.logged.lock().unwrap().insert(msg.clone()) {
            tracing::warn!(target: "vantage_diorama::sugar", "{msg}");
        }
    }

    fn strip(&self, record: &Record<CborValue>) -> Record<CborValue> {
        let mut r = record.clone();
        for name in self.sugar.outputs.iter() {
            r.shift_remove(name);
        }
        r
    }

    /// Merge outputs into `rows`, from the memo where the row is unchanged.
    async fn sweeten(&self, rows: &mut [(&str, &mut Record<CborValue>)]) {
        let mut misses = Vec::new();
        {
            let memo = self.memo.lock().unwrap();
            for (i, (id, row)) in rows.iter_mut().enumerate() {
                let print = fingerprint(row);
                match memo.get(*id) {
                    Some((p, outputs)) if *p == print => merge(row, outputs),
                    _ => misses.push((i, print)),
                }
            }
        }
        if misses.is_empty() {
            return;
        }
        let inputs = misses.iter().map(|(i, _)| rows[*i].1.clone()).collect();
        let answers = (self.sugar.apply)(inputs).await;
        let mut memo = self.memo.lock().unwrap();
        for ((i, print), answer) in misses.into_iter().zip(answers) {
            // A failed row reads without outputs and is not memoized, so the
            // next read tries it again.
            let outputs = match answer {
                Ok(emitted) => {
                    let mut kept = Record::new();
                    for (k, v) in emitted {
                        if self.sugar.outputs.contains(&k) {
                            kept.insert(k, v);
                        } else {
                            self.log_once(format!(
                                "sugar output `{k}` is not declared in `outputs:`; dropped"
                            ));
                        }
                    }
                    kept
                }
                Err(e) => {
                    self.log_once(format!("sugar failed on a row: {e}"));
                    continue;
                }
            };
            let (id, row) = &mut rows[i];
            merge(row, &outputs);
            memo.insert(id.to_string(), (print, outputs));
        }
    }
}

fn merge(row: &mut Record<CborValue>, outputs: &Record<CborValue>) {
    for (k, v) in outputs.iter() {
        row.insert(k.clone(), v.clone());
    }
}

/// Stable within the process: the row's keys and CBOR-encoded values.
fn fingerprint(row: &Record<CborValue>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (k, v) in row.iter() {
        k.hash(&mut h);
        let mut bytes = Vec::new();
        let _ = ciborium::into_writer(v, &mut bytes);
        bytes.hash(&mut h);
    }
    h.finish()
}

#[async_trait]
impl CacheTable for SugaredCache {
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let mut rows = self.inner.list_values().await?;
        let mut refs: Vec<(&str, &mut Record<CborValue>)> =
            rows.iter_mut().map(|(k, v)| (k.as_str(), v)).collect();
        self.sweeten(&mut refs).await;
        Ok(rows)
    }

    async fn get_value(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let Some(mut row) = self.inner.get_value(id).await? else {
            return Ok(None);
        };
        self.sweeten(&mut [(id, &mut row)]).await;
        Ok(Some(row))
    }

    async fn get_value_with_status(
        &self,
        id: &str,
    ) -> Result<Option<(Record<CborValue>, CacheStatus)>> {
        let Some((mut row, status)) = self.inner.get_value_with_status(id).await? else {
            return Ok(None);
        };
        self.sweeten(&mut [(id, &mut row)]).await;
        Ok(Some((row, status)))
    }

    async fn list_values_with_status(
        &self,
    ) -> Result<IndexMap<String, (Record<CborValue>, CacheStatus)>> {
        let mut rows = self.inner.list_values_with_status().await?;
        let mut refs: Vec<(&str, &mut Record<CborValue>)> =
            rows.iter_mut().map(|(k, (v, _))| (k.as_str(), v)).collect();
        self.sweeten(&mut refs).await;
        Ok(rows)
    }

    async fn insert_value(&self, id: &str, record: &Record<CborValue>) -> Result<()> {
        self.inner.insert_value(id, &self.strip(record)).await
    }

    async fn insert_values(&self, rows: IndexMap<String, Record<CborValue>>) -> Result<()> {
        let rows = rows.into_iter().map(|(k, v)| (k, self.strip(&v))).collect();
        self.inner.insert_values(rows).await
    }

    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Record<CborValue>,
        status: CacheStatus,
    ) -> Result<()> {
        self.inner
            .insert_value_with_status(id, &self.strip(record), status)
            .await
    }

    async fn delete_value(&self, id: &str) -> Result<()> {
        self.memo.lock().unwrap().remove(id);
        self.inner.delete_value(id).await
    }

    async fn clear(&self) -> Result<()> {
        self.memo.lock().unwrap().clear();
        self.inner.clear().await
    }

    async fn count(&self) -> Result<i64> {
        self.inner.count().await
    }

    async fn meta_total(&self) -> Result<Option<u64>> {
        self.inner.meta_total().await
    }

    async fn set_meta_total(&self, total: u64) -> Result<()> {
        self.inner.set_meta_total(total).await
    }

    async fn meta_refreshed_at(&self) -> Result<Option<SystemTime>> {
        self.inner.meta_refreshed_at().await
    }

    async fn set_meta_refreshed_at(&self, at: SystemTime) -> Result<()> {
        self.inner.set_meta_refreshed_at(at).await
    }

    async fn meta_copied_at(&self) -> Result<Option<SystemTime>> {
        self.inner.meta_copied_at().await
    }

    async fn set_meta_copied_at(&self, at: SystemTime) -> Result<()> {
        self.inner.set_meta_copied_at(at).await
    }
}
