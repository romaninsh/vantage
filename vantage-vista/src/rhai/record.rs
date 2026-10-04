//! The `Record` draft type: `table(name).record(id)` (existing row) or
//! `table(name).record()` (new row), staged edits, and `save()`/`delete()`.
//!
//! A draft tracks its own `id`, a `baseline` (the last saved state, `None`
//! for a never-saved row) and `changes` (staged but unsaved edits). `save()`
//! inserts a new row or patches only the changed fields of an existing one;
//! `delete()` requires a saved id. Both report through `status()`
//! (`"tracking"` | `"pending"` | `"failed"`) and `rejection()`.
//!
//! Writes go through the same capability checks and [`Writes::Denied`]
//! message as the write terminals (see [`super::write`]); [`save`] calls
//! their helpers rather than duplicating the insert/patch/found logic.
//!
//! This file holds the draft state; [`verbs`] registers the Rhai surface.

mod dotted;
mod save;
mod verbs;

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result as CoreResult, error};
use vantage_dataset::ReadableValueSet;

use super::bridge::block_on;
use super::handle::Handle;
use super::vocab::{TargetResolver, Writes};

pub(crate) use verbs::register_record;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftStatus {
    Tracking,
    Pending,
    Failed,
}

impl DraftStatus {
    fn as_str(self) -> &'static str {
        match self {
            DraftStatus::Tracking => "tracking",
            DraftStatus::Pending => "pending",
            DraftStatus::Failed => "failed",
        }
    }
}

struct DraftState {
    handle: Handle,
    resolver: Option<TargetResolver>,
    writes: Writes,
    id: Option<String>,
    baseline: Option<IndexMap<String, CborValue>>,
    changes: IndexMap<String, CborValue>,
    status: DraftStatus,
    rejection: Option<String>,
    /// The write target's id column, resolved on the first field set.
    id_column: Option<String>,
}

/// A staged, single-row edit over a [`Handle`]'s table. Rhai type `"Record"`.
#[derive(Clone)]
pub struct RecordDraft {
    inner: Arc<Mutex<DraftState>>,
}

impl RecordDraft {
    pub fn new_row(handle: Handle, resolver: Option<TargetResolver>, writes: Writes) -> Self {
        Self::build(handle, resolver, writes, None, None)
    }

    /// A draft of an existing row, fetched through `handle`. Errors (marked
    /// not-found) when no row has `id`.
    pub fn load(
        handle: Handle,
        resolver: Option<TargetResolver>,
        writes: Writes,
        id: &str,
    ) -> CoreResult<Self> {
        let vista = handle.resolve(resolver.as_ref())?;
        let record = block_on(vista.get_value(id.to_string()))??
            .ok_or_else(|| error!("record not found", id = id).mark_not_found())?;
        Ok(Self::from_row(
            handle,
            resolver,
            writes,
            id.to_string(),
            record.into_inner(),
        ))
    }

    /// A draft of a row already in hand, without a fetch.
    pub fn from_row(
        handle: Handle,
        resolver: Option<TargetResolver>,
        writes: Writes,
        id: String,
        row: IndexMap<String, CborValue>,
    ) -> Self {
        Self::build(handle, resolver, writes, Some(id), Some(row))
    }

    fn build(
        handle: Handle,
        resolver: Option<TargetResolver>,
        writes: Writes,
        id: Option<String>,
        baseline: Option<IndexMap<String, CborValue>>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(DraftState {
                handle,
                resolver,
                writes,
                id,
                baseline,
                changes: IndexMap::new(),
                status: DraftStatus::Tracking,
                rejection: None,
                id_column: None,
            })),
        }
    }

    /// The currently staged, unsaved edits.
    pub fn changes(&self) -> IndexMap<String, CborValue> {
        self.inner.lock().unwrap().changes.clone()
    }

    /// The row's id; `None` until a new row is saved.
    pub fn id(&self) -> Option<String> {
        self.inner.lock().unwrap().id.clone()
    }

    /// The row as a script reads it: the baseline with staged edits on top.
    pub fn values(&self) -> IndexMap<String, CborValue> {
        let guard = self.inner.lock().unwrap();
        let mut out = guard.baseline.clone().unwrap_or_default();
        for (k, v) in &guard.changes {
            out.insert(k.clone(), v.clone());
        }
        out
    }
}
