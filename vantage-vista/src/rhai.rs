//! Rhai scripting surface over the type-erased [`Vista`](crate::vista::Vista).
//!
//! vantage-vista owns Rhai engine construction with a *backend-agnostic*
//! vocabulary, in two layers:
//!
//! - [`conventional`] — the chainable query *builder*: `table(name)` resolves a
//!   fresh target through an injected [`TargetResolver`], and builder verbs
//!   (`add_condition_eq`, `add_order`, `get_ref`…) narrow it in place. Backends
//!   layer vendor-specific verbs on top via
//!   [`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions).
//! - [`fetch`] — the read-only *terminal* verbs (`list`, `get_some`, `count`,
//!   `capabilities`, `columns`, `references`) that actually read data, plus the
//!   [`runtime::run_script`] runner that drives their async fetches from
//!   synchronous Rhai.
//!
//! [`convert`] and [`introspect`] are shared internals (value round-tripping and
//! schema/capability map building).

mod conventional;
mod convert;
mod fetch;
mod introspect;
mod runtime;

pub use conventional::{
    AugmentSourceFn, LazyValueFn, RhaiVista, TargetResolver, augment_source_closure,
    eval_augment_source, eval_lazy_expression, eval_modify_script, eval_ref_script,
    lazy_value_closure, register_conventional_onto,
};
pub use fetch::register_fetch_verbs;
pub use runtime::{DEFAULT_LIMIT, MAX_LIMIT, MIN_LIMIT, run_script};
