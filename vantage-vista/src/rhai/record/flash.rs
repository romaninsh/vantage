//! `flash()` and `delete()` on a [`RecordDraft`], with the draft's status
//! tracked around each write.

use ciborium::Value as CborValue;
use vantage_dataset::WritableValueSet;
use vantage_rhai::rhai::Dynamic;
use vantage_types::Record;

use super::{DraftStatus, RecordDraft};
use crate::rhai::bridge::block_on;
use crate::rhai::read::RhaiResult;
use crate::rhai::write::{delete_row, found, has_auto_id, insert_record, new_id};

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

/// Insert a new row, or patch the changed fields of an existing one; the
/// row's id either way. An existing row with nothing staged writes nothing.
pub(super) fn try_flash(r: &RecordDraft) -> RhaiResult<Dynamic> {
    let (handle, gate, id, changes) = {
        let guard = r.inner.lock().unwrap();
        (
            guard.handle.clone(),
            guard.gate.clone(),
            guard.id.clone(),
            guard.changes.clone(),
        )
    };
    match id {
        None => {
            let vista = gate.target(&handle)?;
            let id_col = vista.id_column().unwrap_or("id").to_string();
            let minted = {
                let mut g = r.inner.lock().unwrap();
                if g.minted_id.is_none() && !has_auto_id(&vista) && !changes.contains_key(&id_col) {
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
            let vista = gate.target(&handle)?;
            let rec = Record::from_indexmap(changes);
            let landed = found(block_on(vista.patch_value(id.clone(), &rec)))?;
            if !landed {
                return Err(format!("record `{id}` no longer exists").into());
            }
            let mut guard = r.inner.lock().unwrap();
            guard
                .baseline
                .get_or_insert_default()
                .extend(rec.into_inner());
            guard.changes.clear();
            Ok(Dynamic::from(id))
        }
    }
}

/// Delete the draft's row: `true` once it is gone. A draft never written
/// has nothing to delete and throws, as does a denied or refused delete.
pub(super) fn try_delete(r: &RecordDraft) -> RhaiResult<bool> {
    let (handle, gate, id) = {
        let guard = r.inner.lock().unwrap();
        (guard.handle.clone(), guard.gate.clone(), guard.id.clone())
    };
    let Some(id) = id else {
        return Err("record has no id; it was never flashed".into());
    };
    delete_row(&gate.target(&handle)?, id)
}
