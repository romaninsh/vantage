//! `save()` and `delete()` on a [`RecordDraft`], with the draft's status
//! tracked around each write.

use ciborium::Value as CborValue;
use vantage_dataset::WritableValueSet;
use vantage_rhai::rhai::Dynamic;
use vantage_types::Record;

use super::{DraftStatus, RecordDraft};
use crate::VistaCapabilities;
use crate::rhai::bridge::block_on;
use crate::rhai::handle::Handle;
use crate::rhai::read::{RhaiResult, rhai_err};
use crate::rhai::vocab::{TargetResolver, Writes};
use crate::rhai::write::{delete_row, found, insert_record, new_id, target};
use crate::vista::Vista;

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
pub(super) fn tracked<T>(
    r: &RecordDraft,
    write: impl FnOnce(&RecordDraft) -> RhaiResult<T>,
) -> RhaiResult<T> {
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

pub(super) fn try_save(r: &RecordDraft) -> RhaiResult<Dynamic> {
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
            let id_col = vista.get_id_column().unwrap_or("id").to_string();
            let minted = {
                let mut g = r.inner.lock().unwrap();
                if g.minted_id.is_none() && !vista.has_auto_id() && !changes.contains_key(&id_col) {
                    g.minted_id = Some(new_id(&vista, &id_col)?);
                }
                g.minted_id.clone()
            };
            let new_id = insert_record(
                &vista,
                Record::from_indexmap(changes.clone()),
                minted.as_deref(),
            )?;
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

pub(super) fn try_delete(r: &RecordDraft) -> RhaiResult<bool> {
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
    if matches!(writes, Writes::Denied(_)) {
        return Ok(false);
    }
    let vista = handle.write_target(resolver.as_ref()).map_err(rhai_err)?;
    delete_row(&vista, id)
}
