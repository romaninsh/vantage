//! Internal write-queue message type. Not part of the public surface —
//! callers use `WritableValueSet::insert_value`, `replace_value`, etc.
//! and never see `WriteOp`.

use ciborium::Value as CborValue;
use tokio::sync::oneshot;
use vantage_core::Result;
use vantage_types::Record;

/// One unit of work for the write-queue worker. Each variant carries a
/// `oneshot::Sender` so the originating call site can `.await` the
/// outcome without polling.
pub enum WriteOp {
    Insert {
        id: String,
        record: Record<CborValue>,
        reply: oneshot::Sender<Result<Record<CborValue>>>,
    },
    Replace {
        id: String,
        record: Record<CborValue>,
        reply: oneshot::Sender<Result<Record<CborValue>>>,
    },
    Patch {
        id: String,
        partial: Record<CborValue>,
        reply: oneshot::Sender<Result<Record<CborValue>>>,
    },
    Delete {
        id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    DeleteAll {
        reply: oneshot::Sender<Result<()>>,
    },
}
