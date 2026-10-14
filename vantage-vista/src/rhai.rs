// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
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
//! [`Terminals`] picks which terminals a host gets, and a
//! [`VistaResolver`](crate::VistaResolver) given through
//! [`DataVocab::with_resolver`] backs `table(name)`; a driver builds one over
//! its factory with [`script_tables`]. Backends add their own
//! verbs through [`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions).
//! Terminal futures run through [`block_on`].
//!
//! A denied or unsupported write throws: [`Writes::Denied`] throws its
//! message, and a write the Vista refuses throws the Vista's error.
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
mod tables;
mod traverse;
mod vocab;
mod write;

pub use bridge::block_on;
pub use convert::{
    cbor_to_dynamic, dynamic_to_cbor, map_to_record, record_to_dynamic, record_to_map,
};
pub use eval::{eval_modify_script, eval_ref_script, reject_dropped_narrowing};
pub use handle::Handle;
pub use import::{IMPORT_CANCELLED, IMPORT_CANCELLED_SKIPPED};
pub(crate) use lazy::lazy_value_closure;
pub use record::RecordDraft;
pub use runtime::{DEFAULT_LIMIT, MAX_LIMIT, MIN_LIMIT, preview_script, run_script};
pub use tables::script_tables;
pub use vocab::{DataVocab, Terminals, Writes};
