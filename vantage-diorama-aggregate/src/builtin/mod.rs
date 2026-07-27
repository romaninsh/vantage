//! Aggregations that ship with the crate.
//!
//! Each is a small [`Aggregation`](crate::Aggregation) impl and nothing more —
//! the same surface anything you write yourself uses.

mod as_rows;
mod count;
mod filter;
mod group;
mod stats;

pub use as_rows::AsRows;
pub use count::{Count, CountWhere};
pub use filter::{Conditions, Where};
pub use group::{GroupBy, GroupReducer, Reduce};
pub use stats::{Avg, Distinct, Max, Min, Sum};
