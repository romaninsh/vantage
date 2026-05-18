use ciborium::Value as CborValue;
use vantage_types::Record;

/// One unit of work for the Dio write-queue worker.
///
/// Each variant mirrors a `TableShell` write method, carrying the
/// arguments the user's `on_write` callback (or the default
/// write-to-master path) needs. The queue is fire-and-forget — the
/// originating `dio.vista().insert(...)` returns as soon as the op is
/// enqueued, and failures land on the event bus as
/// [`DioEvent::WriteFailed`](crate::dio::DioEvent::WriteFailed).
#[derive(Debug, Clone)]
pub enum WriteOp {
    /// Insert a row with the given id.
    Insert {
        id: String,
        record: Record<CborValue>,
    },
    /// Replace (upsert) the row at `id`.
    Replace {
        id: String,
        record: Record<CborValue>,
    },
    /// Merge `partial` into the existing row at `id`.
    Patch {
        id: String,
        partial: Record<CborValue>,
    },
    /// Delete the row at `id`.
    Delete { id: String },
    /// Delete every row in the master table.
    DeleteAll,
}

impl WriteOp {
    /// Id the op targets, if any. `DeleteAll` returns `None`.
    pub fn id(&self) -> Option<&str> {
        match self {
            WriteOp::Insert { id, .. }
            | WriteOp::Replace { id, .. }
            | WriteOp::Patch { id, .. }
            | WriteOp::Delete { id } => Some(id.as_str()),
            WriteOp::DeleteAll => None,
        }
    }
}
