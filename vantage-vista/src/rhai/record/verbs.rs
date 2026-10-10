//! The `Record` type's Rhai registration: `record()`/`record(id)` on
//! `Table`, field access, staging and inspection verbs.

use vantage_rhai::rhai::{Array, Dynamic, Engine, EvalAltResult, Map as RhaiMap};

use super::RecordDraft;
use super::dotted::set_dotted;
use super::save::{tracked, tracked_delete, try_save};
use crate::rhai::convert::{cbor_to_dynamic, dynamic_to_cbor};
use crate::rhai::handle::Handle;
use crate::rhai::read::{RhaiResult, id_string, rhai_err};
use crate::rhai::vocab::{TargetResolver, Writes};

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
        tracked_delete(r)
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
