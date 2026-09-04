//! One Rhai host for every Vantage YAML script slot.
//!
//! Consumers use the re-exported [`rhai`] so every crate shares one version
//! and feature set.

pub use rhai;

mod slot;

pub use slot::{Block, Expr, Template};
