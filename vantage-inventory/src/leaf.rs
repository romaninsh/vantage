//! The leaf store's records: one [`Leaf`] per loaded file, and what a
//! reload reports: the [`Change`]s and [`Failure`]s in a [`Reload`].

use std::path::PathBuf;
use std::sync::Arc;

use crate::dyn_kind::AnyValue;
use crate::kind::Stage;

/// One loaded file, as read back from an [`Inventory`](crate::Inventory).
#[derive(Debug)]
pub struct Leaf<V> {
    /// The last value that parsed. `None` only for a
    /// [`Missing::Default`](crate::Missing::Default) root file that is
    /// absent or broken.
    pub value: Option<Arc<V>>,
    /// Starts at 1 and bumps on every reload that parses and validates
    /// cleanly. A broken or invalid edit leaves it where it was.
    pub serial: u64,
    /// Composite hash over the file and every file it `!include`s.
    pub hash: u64,
    /// Canonical paths of every file this leaf `!include`d. An edit to any
    /// of them reloads the leaf.
    pub includes: Vec<PathBuf>,
    /// Cross-reference diagnostics from the last validation pass, or the
    /// messages of an [`OnFailure::Report`](crate::OnFailure::Report).
    /// Empty means valid.
    pub errors: Vec<String>,
    /// The file the leaf was read from.
    pub path: PathBuf,
}

impl<V> Clone for Leaf<V> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            serial: self.serial,
            hash: self.hash,
            includes: self.includes.clone(),
            errors: self.errors.clone(),
            path: self.path.clone(),
        }
    }
}

/// One leaf that a reload added, updated or removed. Revalidation of other
/// leaves is not reported here; see [`Reload::revalidated`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: &'static str,
    pub key: String,
    /// The leaf's serial after the change (its last serial when removed).
    pub serial: u64,
    pub removed: bool,
}

/// A file that failed to load or parse. The inventory logs nothing itself;
/// [`Inventory::load`](crate::Inventory::load) and
/// [`Inventory::reload_path`](crate::Inventory::reload_path) return these
/// for the caller to report.
#[derive(Debug)]
pub struct Failure {
    pub kind: &'static str,
    pub key: String,
    pub path: PathBuf,
    pub stage: Stage,
    pub error: anyhow::Error,
}

/// What one [`Inventory::reload_path`](crate::Inventory::reload_path)
/// did.
#[derive(Debug, Default)]
pub struct Reload {
    /// The leaves added, updated or removed.
    pub changes: Vec<Change>,
    /// The files that failed to load or parse.
    pub failures: Vec<Failure>,
    /// Whether a validation pass changed the errors of any leaf, including
    /// leaves that did not change themselves.
    pub revalidated: bool,
}

/// A leaf as stored: the value is type-erased.
pub(crate) struct Slot {
    pub value: Option<AnyValue>,
    pub serial: u64,
    pub hash: u64,
    pub includes: Vec<PathBuf>,
    pub errors: Vec<String>,
    pub path: PathBuf,
    /// Set while `errors` holds an [`OnFailure::Report`](crate::OnFailure)
    /// rather than diagnostics, so revalidation leaves them alone.
    pub reported: bool,
}

impl Slot {
    pub fn new(path: PathBuf, value: Option<AnyValue>, hash: u64, includes: Vec<PathBuf>) -> Self {
        Self {
            value,
            serial: 1,
            hash,
            includes,
            errors: Vec::new(),
            path,
            reported: false,
        }
    }

    pub fn typed<V: Send + Sync + 'static>(&self) -> Leaf<V> {
        Leaf {
            value: self.value.clone().and_then(|v| v.downcast::<V>().ok()),
            serial: self.serial,
            hash: self.hash,
            includes: self.includes.clone(),
            errors: self.errors.clone(),
            path: self.path.clone(),
        }
    }
}
