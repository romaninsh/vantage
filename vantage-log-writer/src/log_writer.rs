use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::writer_task::{WRITE_QUEUE_CAPACITY, WriteOp, spawn};

/// Append-only data source that writes JSONL records to files in `base_dir`.
///
/// Each table maps to one file: `{base_dir}/{table_name}.jsonl`. Inserts are
/// queued on a background tokio task and the call returns as soon as the
/// message lands on the channel — no fsync, no crash safety. Cloning shares
/// the same channel and worker.
#[derive(Clone)]
pub struct LogWriter {
    inner: Arc<Inner>,
}

struct Inner {
    base_dir: PathBuf,
    id_column: String,
    tx: mpsc::Sender<WriteOp>,
}

impl LogWriter {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel::<WriteOp>(WRITE_QUEUE_CAPACITY);
        spawn(rx);
        Self {
            inner: Arc::new(Inner {
                base_dir: base_dir.into(),
                id_column: "id".to_string(),
                tx,
            }),
        }
    }

    pub fn with_id_column(self, column: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Inner {
                base_dir: self.inner.base_dir.clone(),
                id_column: column.into(),
                tx: self.inner.tx.clone(),
            }),
        }
    }

    pub fn base_dir(&self) -> &std::path::Path {
        &self.inner.base_dir
    }

    pub fn id_column(&self) -> &str {
        &self.inner.id_column
    }

    pub(crate) fn file_path(&self, table_name: &str) -> PathBuf {
        self.inner.base_dir.join(format!("{}.jsonl", table_name))
    }

    pub(crate) fn sender(&self) -> &mpsc::Sender<WriteOp> {
        &self.inner.tx
    }
}

impl std::fmt::Debug for LogWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogWriter")
            .field("base_dir", &self.inner.base_dir)
            .field("id_column", &self.inner.id_column)
            .finish()
    }
}
