//! One Rhai host for every Vantage YAML script slot.
//!
//! Consumers use the re-exported [`rhai`] so every crate shares one version
//! and feature set.

pub use rhai;

mod error;
mod slot;

pub use error::{Located, Result, RhaiError};
pub use slot::{Block, Expr, Template};
