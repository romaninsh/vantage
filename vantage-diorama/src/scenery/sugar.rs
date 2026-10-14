//! Sugar: display-only columns merged into the rows one scenery reads.
//!
//! A scenery opened with a [`Sugar`] reads the Dio's cache through
//! `SugaredCache`, which passes rows through the sugar closure and merges
//! the declared outputs in. Nothing it adds reaches the cache — writes
//! through the wrapper strip the outputs — and other sceneries on the same
//! Dio read plain rows. A memo keyed by row id and a fingerprint of the row
//! keeps re-sorts from re-running the closure; a changed row misses it.

mod cache;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

pub(crate) use cache::SugaredCache;

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

    /// The output columns: the master has never heard of them, so a view's
    /// clause on one is evaluated over the rows the view reads.
    pub(crate) fn outputs(&self) -> Arc<HashSet<String>> {
        self.outputs.clone()
    }
}
