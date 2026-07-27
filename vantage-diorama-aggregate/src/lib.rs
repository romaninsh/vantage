//! Reactive client-side aggregation over a Diorama.
//!
//! A `Dio` keeps a live local copy of a data segment. This crate derives new
//! things from that copy — a count, a total, a table grouped by status — and
//! keeps them current as the source changes.
//!
//! ```ignore
//! let agg = AggregateLens::at("aggregates.redb")?.build()?;
//!
//! // A number the UI mounts like any other value.
//! let total = agg.value(&orders, "orders.total", Count::rows()).await?;
//!
//! // A table the UI mounts like any other table.
//! let by_status = agg
//!     .derive(&orders, "orders.by_status", GroupBy::column("status", revenue))
//!     .await?;
//! let grid = by_status.table_scenery().sort("revenue", SortDir::Desc).open().await?;
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
//! sorts a partially-loaded source cannot advertise `can_order` without lying,
//! because rows it has not seen may belong at the top. An aggregate holds its
//! *entire* output in memory, so its Vista advertises `can_order`,
//! `can_search` and `can_count` and then honours them — sorting a derived table
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
//! Two things keep the cost bounded. The engine debounces, so a burst collapses
//! into one recomputation plus one flush. And every output is compared against
//! the last before anything is published, so recomputing often does not mean
//! repainting often.
//!
//! # Caching
//!
//! One file, one table per aggregate — the same shape a datasource uses for its
//! tables. Derived results are reproducible from the source, so the cache buys
//! a warm start rather than durability: a restart paints the last known value
//! immediately and recomputes behind it.

mod aggregation;
mod builtin;
mod catalog;
mod cmp;
mod derived;
mod engine;
mod lens;
mod value;

pub use aggregation::{AggregateOutput, Aggregation, DerivedRows, Rows};
pub use builtin::{
    Avg, Conditions, Count, CountWhere, Distinct, GroupBy, GroupReducer, Max, Min, Reduce, Sum,
    Where,
};
pub use catalog::{
    AggregationBuilder, AggregationCatalog, AggregationSpec, ScalarAggregation,
};
pub use derived::DerivedDio;
pub use lens::{AggregateLens, AggregateLensBuilder};
pub use value::AggregateValue;

/// Common imports.
pub mod prelude {
    pub use crate::{
        AggregateLens, Aggregation, Count, CountWhere, DerivedDio, DerivedRows, GroupBy,
        GroupReducer, Reduce, Rows, Sum,
    };
}
