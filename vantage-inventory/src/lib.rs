//! The YAML inventory: a project folder of YAML files, loaded into typed
//! values and kept current as the files change.
//!
//! Two layers:
//! - the load pipeline for one file: read → parse → `!include` → hash →
//!   JSON → JSON-schema validate → deserialize ([`load_document`],
//!   [`validate_into`]). Generic over the target type; it knows nothing
//!   about the documents it loads;
//! - the registry for a whole project: each [`Kind`] says where its files
//!   live and how they parse and cross-check, and an [`Inventory`] walks
//!   the folders, holds one [`Leaf`] per file, reloads single paths
//!   ([`Inventory::reload_path`]) and writes schemas, READMEs and a
//!   `.gitignore` block for editors. With the `watch` feature, `watch`
//!   supplies the changed paths.

mod dyn_kind;
mod emit;
mod include;
mod keys;
mod kind;
mod leaf;
mod loader;
mod registry;
mod walk;
#[cfg(feature = "watch")]
mod watch;

pub use dyn_kind::{AnyValue, DynKind};
pub use emit::{EmitReport, GITIGNORE_BEGIN, GITIGNORE_END, ensure_gitignore, schema_file_name};
pub use include::Document;
pub use keys::{IndexSink, KeySnapshot};
pub use kind::{Depth, Kind, Location, Missing, OnFailure, Stage};
pub use leaf::{Change, Failure, Leaf, Reload};
pub use loader::{
    CachedSchema, load_document, load_yaml_validated, parse_document, parse_yaml_validated,
    schema_for_type, validate_into,
};
pub use registry::Inventory;
#[cfg(feature = "watch")]
pub use watch::{Watcher, watch};
