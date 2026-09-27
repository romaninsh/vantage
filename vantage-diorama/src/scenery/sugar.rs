//! Sugar: display-only columns merged into the rows one scenery reads.
//!
//! A scenery opened with a [`Sugar`] reads the Dio's cache through
//! `SugaredCache`, which passes rows through the sugar closure and merges
//! the declared outputs in. Nothing it adds reaches the cache — writes
//! through the wrapper strip the outputs — and other sceneries on the same
//! Dio read plain rows. A memo keyed by row id and a fingerprint of the row
//! keeps re-sorts from re-running the closure; a changed row misses it.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use crate::lens::cache_backend::{CacheStatus, CacheTable};

pub type SugarFuture = Pin<Box<dyn Future<Output = Vec<Result<Record<CborValue>>>> + Send>>;
/// Per input row, in order: that row's output columns only.
pub type SugarFn = Arc<dyn Fn(Vec<Record<CborValue>>) -> SugarFuture + Send + Sync>;

/// A sugar closure and the output columns it may add.
#[derive(Clone)]
pub struct Sugar {
    apply: SugarFn,
    outputs: Arc<HashSet<String>>,
}

impl Sugar {
    pub fn new(apply: SugarFn, outputs: impl IntoIterator<Item = impl Into<String>>) -> Sugar {
        Sugar {
            apply,
            outputs: Arc::new(outputs.into_iter().map(Into::into).collect()),
        }
    }
}

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
        let rows = rows
            .into_iter()
            .map(|(k, v)| {
                let v = self.strip(&v);
                (k, v)
            })
            .collect();
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
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::lens::cache_backend::CacheBackend as _;
    use crate::lens::memory_cache::MemoryCache;

    fn rec(n: i64) -> Record<CborValue> {
        let mut r = Record::new();
        r.insert("n".to_string(), CborValue::Integer(n.into()));
        r
    }

    /// Doubles `n` into `twice`, emits an undeclared `extra`, counts calls.
    fn doubling(calls: Arc<AtomicUsize>) -> Sugar {
        let apply: SugarFn = Arc::new(move |rows| {
            calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                rows.into_iter()
                    .map(|r| {
                        let n = r
                            .get("n")
                            .and_then(|v| v.as_integer())
                            .map(i128::from)
                            .unwrap_or(0);
                        let mut out = Record::new();
                        out.insert("twice".into(), CborValue::Integer(((n * 2) as i64).into()));
                        out.insert("extra".into(), CborValue::Bool(true));
                        Ok(out)
                    })
                    .collect()
            })
        });
        Sugar::new(apply, ["twice"])
    }

    async fn table() -> Arc<dyn CacheTable> {
        MemoryCache::new().open_table("t").await.unwrap()
    }

    #[tokio::test]
    async fn reads_carry_outputs_and_undeclared_keys_are_dropped() {
        let inner = table().await;
        inner.insert_value("a", &rec(2)).await.unwrap();
        let sugared = SugaredCache::wrap(inner.clone(), doubling(Arc::default()));
        let row = sugared.get_value("a").await.unwrap().unwrap();
        assert_eq!(row.get("twice"), Some(&CborValue::Integer(4.into())));
        assert!(row.get("extra").is_none());
        assert!(
            inner
                .get_value("a")
                .await
                .unwrap()
                .unwrap()
                .get("twice")
                .is_none(),
            "cache untouched"
        );
    }

    #[tokio::test]
    async fn writes_through_the_wrapper_strip_outputs() {
        let inner = table().await;
        let sugared = SugaredCache::wrap(inner.clone(), doubling(Arc::default()));
        let mut r = rec(1);
        r.insert("twice".into(), CborValue::Integer(99.into()));
        sugared.insert_value("a", &r).await.unwrap();
        assert!(
            inner
                .get_value("a")
                .await
                .unwrap()
                .unwrap()
                .get("twice")
                .is_none()
        );
    }

    #[tokio::test]
    async fn the_memo_serves_unchanged_rows_and_recomputes_changed_ones() {
        let inner = table().await;
        inner.insert_value("a", &rec(2)).await.unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let sugared = SugaredCache::wrap(inner.clone(), doubling(calls.clone()));
        sugared.list_values().await.unwrap();
        sugared.list_values().await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "second read from the memo");
        inner.insert_value("a", &rec(5)).await.unwrap(); // a refresh behind the wrapper's back
        let row = sugared.get_value("a").await.unwrap().unwrap();
        assert_eq!(row.get("twice"), Some(&CborValue::Integer(10.into())));
    }

    #[tokio::test]
    async fn a_failed_row_reads_without_outputs() {
        let inner = table().await;
        inner.insert_value("a", &rec(1)).await.unwrap();
        let apply: SugarFn = Arc::new(|rows| {
            Box::pin(async move {
                rows.iter()
                    .map(|_| Err(vantage_core::error!("boom")))
                    .collect()
            })
        });
        let sugared = SugaredCache::wrap(inner, Sugar::new(apply, ["twice"]));
        let row = sugared.get_value("a").await.unwrap().unwrap();
        assert!(row.get("twice").is_none());
        assert!(row.get("n").is_some());
    }

    #[tokio::test]
    async fn a_failed_row_is_tried_again_on_the_next_read() {
        let inner = table().await;
        inner.insert_value("a", &rec(3)).await.unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let apply: SugarFn = Arc::new(move |rows| {
            let first = counter.fetch_add(1, Ordering::SeqCst) == 0;
            Box::pin(async move {
                rows.iter()
                    .map(|_| {
                        if first {
                            return Err(vantage_core::error!("boom"));
                        }
                        let mut out = Record::new();
                        out.insert("twice".into(), CborValue::Integer(6.into()));
                        Ok(out)
                    })
                    .collect()
            })
        });
        let sugared = SugaredCache::wrap(inner, Sugar::new(apply, ["twice"]));
        sugared.get_value("a").await.unwrap();
        let row = sugared.get_value("a").await.unwrap().unwrap();
        assert_eq!(row.get("twice"), Some(&CborValue::Integer(6.into())));
    }
}
