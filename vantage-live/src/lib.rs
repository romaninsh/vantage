//! # vantage-live
//!
//! A write-through cache layer that wraps any `AnyTable` (the "master") and
//! adds a local cache plus an optional event stream. Reads consult the cache
//! first; misses fall through to the master and populate the cache on the way
//! back. Writes are queued on a worker task and applied to the master, then
//! the cache is invalidated. An optional [`LiveStream`] keeps the cache in
//! sync with out-of-band changes (SurrealDB LIVE, Kafka, etc.).
//!
//! See `DESIGN.md` in this crate for the architectural rationale.

pub mod cache;
pub mod live_stream;
pub mod prelude;

pub use cache::{Cache, CachedRows};
pub use live_stream::{LiveEvent, LiveStream};
