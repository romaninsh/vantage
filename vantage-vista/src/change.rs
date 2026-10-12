use std::pin::Pin;

use ciborium::Value as CborValue;
use futures_core::Stream;
use vantage_core::Result;
use vantage_types::Record;

/// A single change observed on the underlying set by a live subscription.
///
/// A driver whose backend can stream changes (SurrealDB LIVE, Postgres
/// `LISTEN/NOTIFY`, a Mongo change stream) emits one of these per affected
/// row. The `value` carried on `Inserted`/`Updated` is the record in the
/// **same projected, id-keyed shape** the driver returns from `list`, so
/// consumers can drop it straight into a cache without re-reading.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum VistaChange {
    /// A row entered the set.
    Inserted {
        id: String,
        value: Record<CborValue>,
    },
    /// An existing row's contents changed.
    Updated {
        id: String,
        value: Record<CborValue>,
    },
    /// A row left the set.
    Deleted { id: String },
    /// "Something changed, but I can't say what" — a coarse invalidation. The
    /// consumer should reconcile by re-reading the whole set. This is what a
    /// payload-less push (Postgres `LISTEN/NOTIFY`) can offer; drivers that
    /// carry the row (SurrealDB LIVE) emit the fine-grained variants instead.
    Invalidated,
}

impl VistaChange {
    /// The id of the affected row, or `None` for a coarse [`Invalidated`](Self::Invalidated).
    pub fn id(&self) -> Option<&str> {
        match self {
            VistaChange::Inserted { id, .. }
            | VistaChange::Updated { id, .. }
            | VistaChange::Deleted { id } => Some(id),
            VistaChange::Invalidated => None,
        }
    }
}

/// A stream of [`VistaChange`]s from a live subscription. `'static` and `Send`
/// so it can be handed to a background task.
pub type VistaChangeStream = Pin<Box<dyn Stream<Item = Result<VistaChange>> + Send>>;
