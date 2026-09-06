//! One Rhai host for every Vantage YAML script slot.
//!
//! A YAML struct holds an [`Expr`], [`Template`] or [`Block`] (source only).
//! A [`Host`] — one `rhai::Engine` with closed [`Limits`] plus a bounded AST
//! cache — compiles it into a [`Compiled`] slot, which evaluates against an
//! [`Env`]: pushed variables plus an optional lazy [`Resolver`].
//!
//! ```ignore
//! let host = Host::builder(Limits::Ui).vocab(MyVerbs).build();
//! let when = host.compile(&Expr::from("row.status == \"placed\""))?;
//! let open = when.eval_bool(&Env::new().var("row", row))?;
//! ```
//!
//! Discovery ([`Compiled::discover`]) evaluates once with every resolver read
//! recorded and freezes the read-set, which is how framework pages learn what
//! an expression depends on. Consumers use the re-exported [`rhai`] so every
//! crate shares one version and feature set.

pub use rhai;

mod compiled;
mod error;
mod host;
mod json;
mod limits;
mod resolver;
mod slot;
pub mod template;

pub use compiled::{Compiled, Env, Slot};
pub use error::{Located, Result, RhaiError};
pub use host::{AST_CACHE_BOUND, Host, HostBuilder, Mode, Vocab, background_host, ui_host};
pub use json::{from_json, to_json};
pub use limits::{BACKGROUND_MAX_OPERATIONS, Limits, UI_MAX_OPERATIONS};
pub use resolver::{Lookup, Resolver};
pub use slot::{Block, Expr, Template};
