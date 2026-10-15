// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
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
//! An aggregation is recomputed whole on every change, debounced by
//! [`Tuning::aggregate_debounce`](crate::Tuning::aggregate_debounce), and an
//! output equal to the last one publishes nothing. A derived Dio holds its whole
//! output in memory on a Lens of its own, so its Vista advertises and honours
//! `Order`, `Search` and `Window`. Nothing is persisted: the source's cache is
//! the warm start. The reasoning is in `README_models.md`, "Aggregates".

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
