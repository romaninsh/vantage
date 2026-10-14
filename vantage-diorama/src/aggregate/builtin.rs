//! Aggregations that ship with diorama.
//!
//! Each is a small [`Aggregation`](crate::aggregate::Aggregation) impl and nothing more —
//! the same surface anything you write yourself uses.

mod as_rows;
mod count;
mod filter;
#[cfg(test)]
mod fixtures;
mod group;
mod stats;

pub use as_rows::AsRows;
pub use count::Count;
pub use filter::{Conditions, Where};
pub use group::{GroupBy, GroupReducer, Reduce};
pub use stats::{Avg, Distinct, Max, Min, Sum};
