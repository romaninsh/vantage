//! Rhai scripting surface over the type-erased [`Vista`](crate::vista::Vista).
//!
//! Scripts touch data through one vocabulary, [`DataVocab`]:
//!
//! - `table(name)` returns a [`Handle`], an immutable description of a set;
//!   `where`, `sort`, `search`, `limit` and `ref` narrow it into new handles
//!   (see `narrow`);
//! - read terminals (`list`, `get`, `first`, `count`, `ids`, `columns`,
//!   `references`, `capabilities`) resolve the handle and read;
//! - write terminals (`insert`, `upsert`, `patch`, `delete`, `import_from`) write
//!   to the handle's table.
//!
//! [`Terminals`] picks which terminals a host gets. Backends add their own verbs
//! through [`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions).
//! Terminal futures run through [`block_on`].
//!
//! [`run_script`] and [`preview_script`] are the agent-tool runners; the
//! `eval_*` functions evaluate YAML script slots into Vistas.

mod bridge;
mod convert;
mod eval;
mod handle;
mod import;
mod introspect;
mod lazy;
mod narrow;
mod no_rows;
mod read;
mod record;
mod runtime;
mod traverse;
mod vocab;
mod write;

pub use bridge::block_on;
pub use convert::{
    cbor_to_dynamic, dynamic_to_cbor, map_to_record, record_to_dynamic, record_to_map,
};
pub use eval::{
    AugmentSourceFn, augment_source_closure, eval_augment_source, eval_modify_script,
    eval_ref_script,
};
pub use handle::{Handle, Step};
pub use import::{IMPORT_CANCELLED, IMPORT_CANCELLED_SKIPPED};
pub use lazy::{LazyValueFn, eval_lazy_expression, lazy_value_closure};
pub use record::RecordDraft;
pub use runtime::{DEFAULT_LIMIT, MAX_LIMIT, MIN_LIMIT, preview_script, run_script};
pub use vocab::{DataVocab, TargetResolver, Terminals, Writes};

#[cfg(test)]
mod tests {
    mod bridge;
    mod eval;
    mod narrow;
    mod runtime;
}
