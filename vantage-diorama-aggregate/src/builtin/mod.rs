//! Aggregations that ship with the crate.
//!
//! Each is a small [`Aggregation`](crate::Aggregation) impl and nothing more —
//! the same surface anything you write yourself uses.

mod count;
mod group;
mod stats;

pub use count::{Count, CountWhere};
pub use group::{GroupBy, GroupReducer, Reduce};
pub use stats::{Avg, Distinct, Max, Min, Sum};
