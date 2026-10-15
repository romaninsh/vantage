//! [`Inventory`]: the registered kinds plus the leaf store, one leaf per
//! file found. `validate` holds the validation pass, `reload` the per-path
//! update a file watcher calls.

use std::any::TypeId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};

use crate::dyn_kind::DynKind;
use crate::kind::{Kind, Location, Missing, Stage};
use crate::leaf::{Failure, Leaf, Slot};
use crate::{load_document, walk};

mod reload;
mod validate;

/// The kinds of one project and the leaves loaded for them.
pub struct Inventory {
    root: PathBuf,
    kinds: Vec<Box<dyn DynKind>>,
    /// One map per entry of `kinds`, in the same order.
    leaves: Vec<BTreeMap<String, Slot>>,
}

impl Inventory {
    /// An inventory rooted at `root` with no kinds and no leaves.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            kinds: Vec::new(),
            leaves: Vec::new(),
        }
    }

    /// Register a kind. Panics like [`Inventory::with_dyn`].
    pub fn with<K: Kind>(self, kind: K) -> Self {
        self.with_dyn(Box::new(kind))
    }

    /// Register a boxed kind.
    ///
    /// # Panics
    /// If a kind with the same id, or reading the same folder or root file,
    /// is already registered.
    pub fn with_dyn(mut self, kind: Box<dyn DynKind>) -> Self {
        assert!(
            self.kind_index(kind.id()).is_none(),
            "inventory kind `{}` registered twice",
            kind.id()
        );
        if let Some(other) = self
            .kinds
            .iter()
            .find(|k| same_place(k.location(), kind.location()))
        {
            match kind.location() {
                Location::Folder { name, .. } => panic!(
                    "inventory kinds `{}` and `{}` both read folder `{name}/`",
                    other.id(),
                    kind.id()
                ),
                Location::RootFile(name) => panic!(
                    "inventory kinds `{}` and `{}` both read root file `{name}`",
                    other.id(),
                    kind.id()
                ),
            }
        }
        self.kinds.push(kind);
        self.leaves.push(BTreeMap::new());
        self
    }

    /// Register boxed kinds in order. Panics like [`Inventory::with_dyn`].
    pub fn with_all(self, kinds: impl IntoIterator<Item = Box<dyn DynKind>>) -> Self {
        kinds.into_iter().fold(self, Self::with_dyn)
    }

    /// The project root given to [`Inventory::new`].
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The registered kinds, in registration order.
    pub fn kinds(&self) -> impl Iterator<Item = &dyn DynKind> {
        self.kinds.iter().map(|k| k.as_ref())
    }

    /// Replace every leaf with what is on disk now, then validate them all.
    /// A file that fails to load or parse adds no leaf; it comes back as a
    /// [`Failure`] for the caller to report. The inventory logs nothing.
    ///
    /// Fails only when the root is not a directory. The inventory is then
    /// empty apart from its [`Missing::Default`] leaves, which always exist.
    pub fn load(&mut self) -> Result<Vec<Failure>> {
        self.leaves.iter_mut().for_each(BTreeMap::clear);
        let mut failures = Vec::new();
        let root_is_dir = walk::is_dir(&self.root);
        if root_is_dir {
            for at in 0..self.kinds.len() {
                for (key, path) in walk::files(&self.root, self.kinds[at].location()) {
                    failures.extend(self.load_leaf(at, key, path));
                }
            }
        }
        for at in 0..self.kinds.len() {
            self.ensure_default_leaf(at);
        }
        self.revalidate();
        if !root_is_dir {
            bail!("inventory root {} is not a directory", self.root.display());
        }
        Ok(failures)
    }

    fn load_leaf(&mut self, at: usize, key: String, path: PathBuf) -> Option<Failure> {
        let parsed = load_document(&path, &self.root)
            .map_err(|e| (Stage::Load, e, None))
            .and_then(|doc| match self.kinds[at].parse_any(&key, &doc) {
                Ok(value) => Ok((value, doc)),
                Err(e) => Err((Stage::Parse, e, Some(doc))),
            });
        let (slot, failure) = match parsed {
            Ok((value, doc)) => (Slot::new(path, Some(value), doc.hash, doc.includes), None),
            Err((stage, error, doc)) => {
                let failure = self.failure(at, &key, &path, stage, error);
                if self.kinds[at].on_missing() != Missing::Default {
                    return Some(failure);
                }
                let (hash, includes) = doc.map(|d| (d.hash, d.includes)).unwrap_or_default();
                (Slot::new(path, None, hash, includes), Some(failure))
            }
        };
        self.leaves[at].insert(key, slot);
        failure
    }

    fn failure(
        &self,
        at: usize,
        key: &str,
        path: &Path,
        stage: Stage,
        error: anyhow::Error,
    ) -> Failure {
        Failure {
            kind: self.kinds[at].id(),
            key: key.to_string(),
            path: path.to_path_buf(),
            stage,
            error,
        }
    }

    fn ensure_default_leaf(&mut self, at: usize) {
        let kind = &self.kinds[at];
        if let (Location::RootFile(name), Missing::Default) = (kind.location(), kind.on_missing())
            && let Some(key) = walk::root_key(name)
        {
            let path = self.root.join(name);
            self.leaves[at]
                .entry(key)
                .or_insert_with(|| Slot::new(path, None, 0, Vec::new()));
        }
    }

    /// The value of `key` for the first registered kind of type `K`.
    pub fn get<K: Kind>(&self, key: &str) -> Option<Arc<K::Value>> {
        self.leaf::<K>(key)?.value
    }

    /// The whole leaf of `key` for the first registered kind of type `K`.
    pub fn leaf<K: Kind>(&self, key: &str) -> Option<Leaf<K::Value>> {
        let at = self
            .kinds
            .iter()
            .position(|k| k.kind_type() == TypeId::of::<K>())?;
        Some(self.leaves[at].get(key)?.typed())
    }

    /// Every key of `kind_id`, sorted. Includes a [`Missing::Default`]
    /// root file's key whether or not it has a value.
    pub fn keys_of(&self, kind_id: &str) -> Vec<String> {
        self.kind_index(kind_id)
            .map(|at| self.leaves[at].keys().cloned().collect())
            .unwrap_or_default()
    }

    /// The leaf's errors; empty when it is valid or does not exist.
    pub fn errors_for(&self, kind_id: &str, key: &str) -> Vec<String> {
        self.slot(kind_id, key)
            .map(|slot| slot.errors.clone())
            .unwrap_or_default()
    }

    /// The kind and key `path` would load as, if any.
    pub fn classify(&self, path: &Path) -> Option<(&'static str, String)> {
        let (at, key) = walk::classify(&self.root, self.kinds.iter().map(|k| k.location()), path)?;
        Some((self.kinds[at].id(), key))
    }

    fn kind_index(&self, kind_id: &str) -> Option<usize> {
        self.kinds.iter().position(|k| k.id() == kind_id)
    }

    fn slot(&self, kind_id: &str, key: &str) -> Option<&Slot> {
        self.leaves[self.kind_index(kind_id)?].get(key)
    }
}

/// Whether two locations read the same files. A folder and a root file
/// never collide: one names a directory, the other a file.
fn same_place(a: Location, b: Location) -> bool {
    match (a, b) {
        (Location::Folder { name: a, .. }, Location::Folder { name: b, .. }) => a == b,
        (Location::RootFile(a), Location::RootFile(b)) => a == b,
        _ => false,
    }
}
