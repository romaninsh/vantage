//! One Rhai host for every Vantage YAML script slot.
//!
//! Consumers use the re-exported [`rhai`] so every crate shares one version
//! and feature set.

pub use rhai;

mod error;
mod host;
mod limits;
mod resolver;
mod slot;
pub mod template;

pub use error::{Located, Result, RhaiError};
pub use host::{AST_CACHE_BOUND, Host, HostBuilder, Vocab};
pub use resolver::{Lookup, Resolver};
pub use limits::{BACKGROUND_MAX_OPERATIONS, Limits, UI_MAX_OPERATIONS};
pub use slot::{Block, Expr, Template};
