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
//! message as the write terminals (see [`super::write`]); this module calls
//! their helpers rather than duplicating the insert/patch/found logic.

mod dotted;

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result as CoreResult, error};
use vantage_dataset::{ReadableValueSet, WritableValueSet};
use vantage_rhai::rhai::{Array, Dynamic, Engine, EvalAltResult, Map as RhaiMap};
use vantage_types::Record;

use self::dotted::set_dotted;
use super::bridge::block_on;
use super::convert::{cbor_to_dynamic, dynamic_to_cbor};
use super::handle::Handle;
use super::read::{RhaiResult, id_string, rhai_err};
use super::vocab::{TargetResolver, Writes};
use super::write::{found, insert_record, target};
use crate::VistaCapabilities;
use crate::vista::Vista;

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

/// Register the `Record` type and its verbs, plus `record()`/`record(id)` on
/// `Table`. `writes` governs `save()`/`delete()`; pass [`Writes::Denied`]
/// for a `Terminals::Read` host so the record exists but every write throws.
pub(crate) fn register_record(
    engine: &mut Engine,
    resolver: Option<TargetResolver>,
    writes: Writes,
) {
    engine.register_type_with_name::<RecordDraft>("Record");

    let r = resolver.clone();
    let w = writes.clone();
    engine.register_fn("record", move |h: &mut Handle| -> RecordDraft {
        RecordDraft::new_row(h.clone(), r.clone(), w.clone())
    });

    let r = resolver;
    let w = writes;
    engine.register_fn(
        "record",
        move |h: &mut Handle, id: Dynamic| -> RhaiResult<RecordDraft> {
            let id = id_string(id)?;
            RecordDraft::load(h.clone(), r.clone(), w.clone(), &id).map_err(rhai_err)
        },
    );

    engine.register_indexer_get(|r: &mut RecordDraft, col: &str| -> Dynamic { get_field(r, col) });
    engine.register_indexer_set(
        |r: &mut RecordDraft, col: &str, value: Dynamic| -> RhaiResult<()> {
            set_field(r, col, value)
        },
    );
    engine.register_get("id", |r: &mut RecordDraft| -> Dynamic { id_of(r) });
    engine.register_set("id", |_: &mut RecordDraft, _: Dynamic| -> RhaiResult<()> {
        Err("`id` is read-only".into())
    });
    engine.register_fn("is_dirty", |r: &mut RecordDraft| -> bool {
        !r.inner.lock().unwrap().changes.is_empty()
    });
    engine.register_fn("dirty", |r: &mut RecordDraft, col: &str| -> bool {
        r.inner.lock().unwrap().changes.contains_key(col)
    });
    engine.register_fn("baseline", |r: &mut RecordDraft| -> RhaiMap {
        baseline_map(r)
    });
    engine.register_fn("revert", |r: &mut RecordDraft| {
        r.inner.lock().unwrap().changes.clear();
    });
    engine.register_fn("revert", |r: &mut RecordDraft, col: &str| {
        r.inner.lock().unwrap().changes.shift_remove(col);
    });
    // Returns the record so a new row chains: `table("t").record().set(#{…}).save()`.
    engine.register_fn(
        "set",
        |r: &mut RecordDraft, map: RhaiMap| -> RhaiResult<RecordDraft> {
            set_map(r, map)?;
            Ok(r.clone())
        },
    );
    engine.register_fn("status", |r: &mut RecordDraft| -> String {
        r.inner.lock().unwrap().status.as_str().to_string()
    });
    engine.register_fn("rejection", |r: &mut RecordDraft| -> Dynamic {
        rejection_of(r)
    });
    engine.register_fn("save", |r: &mut RecordDraft| -> RhaiResult<Dynamic> {
        tracked(r, try_save)
    });
    engine.register_fn("delete", |r: &mut RecordDraft| -> RhaiResult<bool> {
        tracked(r, try_delete)
    });
}

fn get_field(r: &RecordDraft, col: &str) -> Dynamic {
    let guard = r.inner.lock().unwrap();
    guard
        .changes
        .get(col)
        .or_else(|| guard.baseline.as_ref().and_then(|b| b.get(col)))
        .map(cbor_to_dynamic)
        .unwrap_or(Dynamic::UNIT)
}

fn id_column(r: &RecordDraft) -> RhaiResult<String> {
    let (handle, resolver) = {
        let guard = r.inner.lock().unwrap();
        if let Some(col) = &guard.id_column {
            return Ok(col.clone());
        }
        (guard.handle.clone(), guard.resolver.clone())
    };
    // Resolved without the lock: the resolver may block on I/O.
    let vista = handle.write_target(resolver.as_ref()).map_err(rhai_err)?;
    let col = vista.get_id_column().unwrap_or("id").to_string();
    r.inner.lock().unwrap().id_column = Some(col.clone());
    Ok(col)
}

fn set_field(r: &RecordDraft, col: &str, value: Dynamic) -> RhaiResult<()> {
    if col == id_column(r)? {
        return Err(format!("`{col}` is the id column and can't be set").into());
    }
    let cbor = dynamic_to_cbor(value)
        .map_err(|e| -> Box<EvalAltResult> { format!("`{col}`: {e}").into() })?;
    r.inner
        .lock()
        .unwrap()
        .changes
        .insert(col.to_string(), cbor);
    Ok(())
}

fn set_map(r: &RecordDraft, map: RhaiMap) -> RhaiResult<()> {
    let id_col = id_column(r)?;
    let mut staged = Vec::with_capacity(map.len());
    for (k, v) in map {
        if k.as_str() == id_col {
            return Err(format!("`{id_col}` is the id column and can't be set").into());
        }
        let cbor = dynamic_to_cbor(v)
            .map_err(|e| -> Box<EvalAltResult> { format!("`{k}`: {e}").into() })?;
        staged.push((k.to_string(), cbor));
    }
    let mut guard = r.inner.lock().unwrap();
    for (k, cbor) in staged {
        set_dotted(&mut guard.changes, &k, cbor);
    }
    Ok(())
}

fn id_of(r: &RecordDraft) -> Dynamic {
    r.inner
        .lock()
        .unwrap()
        .id
        .clone()
        .map(Dynamic::from)
        .unwrap_or(Dynamic::UNIT)
}

fn baseline_map(r: &RecordDraft) -> RhaiMap {
    let guard = r.inner.lock().unwrap();
    let mut map = RhaiMap::new();
    if let Some(baseline) = &guard.baseline {
        for (k, v) in baseline {
            map.insert(k.as_str().into(), cbor_to_dynamic(v));
        }
    }
    map
}

fn rejection_of(r: &RecordDraft) -> Dynamic {
    let guard = r.inner.lock().unwrap();
    match &guard.rejection {
        None => Dynamic::UNIT,
        Some(msg) => {
            let mut map = RhaiMap::new();
            map.insert("message".into(), Dynamic::from(msg.clone()));
            map.insert("fields".into(), Dynamic::from_array(Array::new()));
            Dynamic::from_map(map)
        }
    }
}

/// `writes` gates `save()`/`delete()` the same way it gates the write
/// terminals: [`Writes::Denied`] throws its message before any capability
/// check, [`Writes::Allowed`] defers to the Vista's own capabilities.
fn checked_target(
    handle: &Handle,
    resolver: Option<&TargetResolver>,
    writes: &Writes,
    verb: &str,
    allowed: impl Fn(&VistaCapabilities) -> bool,
) -> RhaiResult<Vista> {
    match writes {
        Writes::Denied(msg) => Err(msg.clone().into()),
        Writes::Allowed => target(handle, resolver, verb, allowed),
    }
}

/// Run `write` with the draft `pending`, then mark it `tracking`, or
/// `failed` with the error as its rejection.
fn tracked<T>(r: &RecordDraft, write: impl FnOnce(&RecordDraft) -> RhaiResult<T>) -> RhaiResult<T> {
    r.inner.lock().unwrap().status = DraftStatus::Pending;
    let result = write(r);
    let mut guard = r.inner.lock().unwrap();
    match &result {
        Ok(_) => {
            guard.status = DraftStatus::Tracking;
            guard.rejection = None;
        }
        Err(e) => {
            guard.status = DraftStatus::Failed;
            guard.rejection = Some(e.to_string());
        }
    }
    result
}

fn try_save(r: &RecordDraft) -> RhaiResult<Dynamic> {
    let (handle, resolver, writes, id, changes) = {
        let guard = r.inner.lock().unwrap();
        (
            guard.handle.clone(),
            guard.resolver.clone(),
            guard.writes.clone(),
            guard.id.clone(),
            guard.changes.clone(),
        )
    };
    match id {
        None => {
            let vista = checked_target(&handle, resolver.as_ref(), &writes, "save", |c| {
                c.can_insert
            })?;
            let new_id = insert_record(&vista, Record::from_indexmap(changes.clone()))?;
            let id_col = vista.get_id_column().unwrap_or("id").to_string();
            let mut baseline = changes;
            baseline.insert(id_col, CborValue::Text(new_id.clone()));
            let mut guard = r.inner.lock().unwrap();
            guard.id = Some(new_id.clone());
            guard.baseline = Some(baseline);
            guard.changes.clear();
            Ok(Dynamic::from(new_id))
        }
        Some(id) => {
            if changes.is_empty() {
                return Ok(Dynamic::from(id));
            }
            let vista = checked_target(&handle, resolver.as_ref(), &writes, "save", |c| {
                c.can_update
            })?;
            let rec = Record::from_indexmap(changes.clone());
            let landed = found(block_on(vista.patch_value(id.clone(), &rec)))?;
            if !landed {
                return Err(format!("record `{id}` no longer exists").into());
            }
            let mut guard = r.inner.lock().unwrap();
            let mut baseline = guard.baseline.clone().unwrap_or_default();
            for (k, v) in changes {
                baseline.insert(k, v);
            }
            guard.baseline = Some(baseline);
            guard.changes.clear();
            Ok(Dynamic::from(id))
        }
    }
}

fn try_delete(r: &RecordDraft) -> RhaiResult<bool> {
    let (handle, resolver, writes, id) = {
        let guard = r.inner.lock().unwrap();
        (
            guard.handle.clone(),
            guard.resolver.clone(),
            guard.writes.clone(),
            guard.id.clone(),
        )
    };
    let Some(id) = id else {
        return Err("record has no id; it was never saved".into());
    };
    let vista = checked_target(&handle, resolver.as_ref(), &writes, "delete", |c| {
        c.can_delete
    })?;
    found(block_on(WritableValueSet::delete(&vista, id)))
}
