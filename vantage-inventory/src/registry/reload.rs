//! Bringing the leaf store up to date after one path changed on disk.
//!
//! The rules, in order:
//! - every leaf that `!include`d the path reloads first, so an edit to a
//!   shared script or fragment reaches the documents that use it;
//! - a path that is not a leaf of any kind stops there;
//! - a deleted file removes its leaf (a [`Missing::Default`] root file
//!   keeps the leaf and loses its value);
//! - a file whose composite hash matches its leaf is left alone, so a
//!   watcher's duplicate events and same-content saves change nothing;
//! - a parsed file replaces the leaf's value. Its serial bumps only when
//!   validation finds nothing, so a broken state stays at the previous
//!   serial until it is fixed;
//! - a file that fails to load or parse is handled by the kind's
//!   [`on_reload_failure`](crate::Kind::on_reload_failure);
//! - after anything but a deduplicated save, every leaf is revalidated, so
//!   adding a referenced file clears the errors of the files that
//!   reference it.

use std::collections::btree_map::Entry;
use std::path::{Path, PathBuf};

use super::Inventory;
use crate::dyn_kind::AnyValue;
use crate::kind::{Location, Missing, OnFailure, Stage};
use crate::leaf::{Change, Reload, Slot};
use crate::{Document, load_document, walk};

impl Inventory {
    /// Update the store for a created, modified or deleted `path` and
    /// report the leaves that changed, the files that failed and whether
    /// revalidation changed any errors. Paths outside every kind (other
    /// than files some leaf includes) change nothing.
    pub fn reload_path(&mut self, path: &Path) -> Reload {
        let mut reload = Reload::default();
        for dependent in self.dependents_of(path) {
            self.reload_leaf_at(&dependent, &mut reload);
        }
        self.reload_leaf_at(path, &mut reload);
        reload
    }

    /// Paths of every leaf that `!include`d `changed`. Derived by scanning
    /// the leaves on demand rather than kept as a reverse map that could
    /// go stale.
    fn dependents_of(&self, changed: &Path) -> Vec<PathBuf> {
        let changed = walk::canonical_or_same(changed);
        let mut out: Vec<PathBuf> = self
            .leaves
            .iter()
            .flat_map(|leaves| leaves.values())
            .filter(|slot| slot.includes.contains(&changed))
            .map(|slot| slot.path.clone())
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn reload_leaf_at(&mut self, path: &Path, reload: &mut Reload) {
        let locations = self.kinds.iter().map(|k| k.location());
        let Some((at, key)) = walk::classify(&self.root, locations, path) else {
            return;
        };
        if !walk::exists(path) {
            self.remove(at, &key, &mut reload.changes);
        } else {
            let failed = |stage, error, doc| Failed {
                at,
                key: &key,
                path,
                stage,
                error,
                doc,
            };
            match load_document(path, &self.root) {
                Err(error) => self.failed(failed(Stage::Load, error, None), reload),
                Ok(doc) if self.hash_of(at, &key) == Some(doc.hash) => return,
                Ok(doc) => match self.kinds[at].parse_any(&key, &doc) {
                    Ok(value) => {
                        self.upsert(at, key.clone(), path, value, doc, &mut reload.changes)
                    }
                    Err(error) => self.failed(failed(Stage::Parse, error, Some(doc)), reload),
                },
            }
        }
        reload.revalidated |= self.revalidate();
    }

    /// Store a freshly parsed value. Validation runs against the keys as
    /// they were before this leaf changed; the pass that follows sees the
    /// update.
    fn upsert(
        &mut self,
        at: usize,
        key: String,
        path: &Path,
        value: AnyValue,
        doc: Document,
        changes: &mut Vec<Change>,
    ) {
        let errors = self.kinds[at].validate_any(&key, &value, &self.snapshot());
        let kind = self.kinds[at].id();
        let slot = match self.leaves[at].entry(key.clone()) {
            Entry::Occupied(entry) => {
                let slot = entry.into_mut();
                if errors.is_empty() {
                    slot.serial += 1;
                }
                slot.value = Some(value);
                slot.path = path.to_path_buf();
                slot.hash = doc.hash;
                slot.includes = doc.includes;
                slot.reported = false;
                slot
            }
            Entry::Vacant(entry) => entry.insert(Slot::new(
                path.to_path_buf(),
                Some(value),
                doc.hash,
                doc.includes,
            )),
        };
        slot.errors = errors;
        changes.push(Change {
            kind,
            key,
            serial: slot.serial,
            removed: false,
        });
    }

    /// Drop a leaf whose file was deleted or whose kind asked for
    /// [`OnFailure::Remove`]. A [`Missing::Default`] root file's leaf stays
    /// and reverts to no value.
    fn remove(&mut self, at: usize, key: &str, changes: &mut Vec<Change>) {
        let kind = self.kinds[at].id();
        if self.is_default_root(at) {
            if let Some(slot) = self.leaves[at].get_mut(key)
                && slot.value.is_some()
            {
                let path = std::mem::take(&mut slot.path);
                *slot = Slot {
                    serial: slot.serial + 1,
                    ..Slot::new(path, None, 0, Vec::new())
                };
                changes.push(Change {
                    kind,
                    key: key.to_string(),
                    serial: slot.serial,
                    removed: false,
                });
            }
            return;
        }
        if let Some(slot) = self.leaves[at].remove(key) {
            changes.push(Change {
                kind,
                key: key.to_string(),
                serial: slot.serial,
                removed: true,
            });
        }
    }

    /// A reload that failed. The failure is always reported; what happens
    /// to the leaf depends on the kind.
    fn failed(&mut self, failed: Failed<'_>, reload: &mut Reload) {
        let Failed {
            at,
            key,
            path,
            stage,
            error,
            doc,
        } = failed;
        let action = self.kinds[at].on_reload_failure(stage, &error);
        reload
            .failures
            .push(self.failure(at, key, path, stage, error));
        let changes = &mut reload.changes;
        if self.is_default_root(at) {
            return self.remove(at, key, changes);
        }
        let kind = self.kinds[at].id();
        let Some(slot) = self.leaves[at].get_mut(key) else {
            return;
        };
        match action {
            OnFailure::Keep => {}
            OnFailure::Remove => self.remove(at, key, changes),
            OnFailure::Report(messages) => {
                match doc {
                    Some(doc) => {
                        slot.hash = doc.hash;
                        slot.includes = doc.includes;
                    }
                    // No document to hash: hash the broken bytes, so that
                    // restoring the last good text is not deduplicated.
                    None => slot.hash = walk::raw_hash(path),
                }
                slot.errors = messages;
                slot.reported = true;
                changes.push(Change {
                    kind,
                    key: key.to_string(),
                    serial: slot.serial,
                    removed: false,
                });
            }
        }
    }

    fn hash_of(&self, at: usize, key: &str) -> Option<u64> {
        self.leaves[at].get(key).map(|slot| slot.hash)
    }

    fn is_default_root(&self, at: usize) -> bool {
        let kind = &self.kinds[at];
        matches!(kind.location(), Location::RootFile(_)) && kind.on_missing() == Missing::Default
    }
}

/// A reload that failed, and where.
struct Failed<'a> {
    at: usize,
    key: &'a str,
    path: &'a Path,
    stage: Stage,
    error: anyhow::Error,
    /// The loaded document, when only the parse failed.
    doc: Option<Document>,
}
