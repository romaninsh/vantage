//! Shared between the two binaries — the reactive server (`main.rs`) and the
//! standalone `mutator` — so both talk to the same `product` table through the
//! same model and connection code.

pub mod db;
pub mod product;
