//! Event source abstraction.
//!
//! `LiveTable` consumes [`LiveEvent`]s from anything that implements
//! [`LiveStream`]. The most useful real-world implementor is SurrealDB's
//! LIVE query (see `vantage-surrealdb`), but the trait is deliberately
//! generic — Kafka, Postgres LISTEN/NOTIFY, Mongo change streams, or any
//! ad-hoc event bus can drive a LiveTable.
//!
//! In v1 every event invalidates the entire `cache_key`, so the variant
//! distinction is informational; we keep the per-id variants for forward
//! compatibility with surgical invalidation.

use futures_util::Stream;
use std::pin::Pin;

mod manual;

pub use manual::ManualLiveStream;

/// A single change event observed at the master. Variants are forward-
/// compatible with future surgical invalidation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveEvent {
    /// "Something moved." Use this when the source can't tell us which row
    /// changed. v1 LiveTable behaves the same regardless of the variant.
    Changed,
    Inserted {
        id: String,
    },
    Updated {
        id: String,
    },
    Deleted {
        id: String,
    },
}

/// Source of [`LiveEvent`]s. Subscribers each get an independent stream.
pub trait LiveStream: Send + Sync {
    fn subscribe(&self) -> Pin<Box<dyn Stream<Item = LiveEvent> + Send>>;
}
