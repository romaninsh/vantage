//! Reactive client-side aggregation over a Diorama.
//!
//! A `Dio` keeps a live local copy of a data segment. This module derives new
//! things from that copy — a count, a total, a table grouped by status — and
//! keeps them current as the source changes.
//!
//! ```ignore
//! // A number the UI mounts like any other value.
//! let total = orders.value_scenery(Count::rows()).open().await?;
//!
//! // A table the UI mounts like any other table.
//! let by_status = orders.aggregate(GroupBy::column("status", revenue)).await?;
//! let grid = by_status
//!     .table_scenery()
//!     .order("revenue", SortDirection::Descending)
//!     .open()
//!     .await?;
//! ```
//!
//! # Why a derived Dio, and not a decorator
//!
//! Re-ordering a table produces the same rows with the same schema, so it
//! belongs in a view over the existing set. Aggregating produces a *different
//! row set with a different schema* — genuinely a new table, which deserves its
//! own Vista, its own cache and its own sceneries.
//!
//! That distinction also decides what can be claimed honestly. A layer that
//! sorts a partially-loaded source cannot advertise `Order` without lying,
//! because rows it has not seen may belong at the top. An aggregate holds its
//! *entire* output in memory, so its Vista advertises `Order`, `Search` and
//! `Window` and then honours them — sorting a derived table
//! is ordinary push-down into its own Vista, not a client-side special case.
//!
//! # Recomputation, not incremental updates
//!
//! Every change recomputes the whole aggregation. There is no accumulator, no
//! retraction, and no delta plumbing. This is a deliberate trade of cycles for
//! correctness: with no retained state there is nothing to drift, a dropped
//! change notification is a non-event, and reductions that cannot retract a row
//! — `max`, `min`, `distinct` — need no special case.
//!
//! Two things keep the cost bounded. The engine debounces
//! ([`Tuning::aggregate_debounce`](crate::Tuning::aggregate_debounce)), so a
//! burst collapses into one recomputation plus one flush. And every output is
//! compared against the last before anything is published, so recomputing
//! often does not mean repainting often.
//!
//! # Caching
//!
//! Nothing is persisted: the source's cache is the warm start. A value scenery
//! opens holding the value computed over it, and a derived Dio opens holding
//! its rows, kept in memory on a Lens of its own.

mod aggregation;
mod builtin;
mod catalog;
mod cmp;
mod coerce;
mod derived;
mod engine;
mod mount;
mod source;
mod value;

pub use aggregation::{AggregateOutput, Aggregation, DerivedRows, Reads, Rows};
pub use builtin::{
    AsRows, Avg, Conditions, Count, Distinct, GroupBy, GroupReducer, Max, Min, Reduce, Sum, Where,
};
pub use catalog::{
    AggregationBuilder, AggregationCatalog, AggregationSpec, GroupReduction, ScalarAggregation,
};

pub(crate) use mount::open_value;
