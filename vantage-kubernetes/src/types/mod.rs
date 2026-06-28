//! Value parsing for Kubernetes records.
//!
//! Kubernetes keeps everything internal to Vantage as [`ciborium::Value`];
//! this module holds the pure parsers the projector uses to turn K8s'
//! string-encoded quantities and timestamps into numbers and ages.

pub mod datetime;
pub mod quantity;

pub use datetime::{age_from, parse as parse_timestamp};
pub use quantity::{parse_cpu_millicores, parse_memory_bytes};
