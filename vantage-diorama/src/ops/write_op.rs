use ciborium::Value as CborValue;
use vantage_types::Record;

/// One unit of work for the Dio write-queue worker.
///
/// Stage 1 holds the shape only. Stage 3 wires this through the worker
/// and adds reply channels for callers that want to await completion.
#[derive(Debug, Clone)]
pub enum WriteOp {
    Insert(Record<CborValue>),
    Update(String, Record<CborValue>),
    Replace(String, Record<CborValue>),
    Delete(String),
}
