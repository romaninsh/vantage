//! Write terminals on the `Table` handle: `insert`, `upsert`, `patch`,
//! `delete`, and `import_from` (see [`super::import`]).
//!
//! Writes go to the whole narrowed handle and stay inside its set (see the
//! book's Writes chapter); a handle with `limit` refuses writes.

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_dataset::{InsertableValueSet, WritableValueSet};
use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult, Map as RhaiMap};
use vantage_types::cbor_id_to_string;

use super::convert::map_to_record;
use super::handle::Handle;
use super::import::{import_rows, import_rows_mapped};
use super::read::{RhaiResult, id_string, rhai_err, run, unsupported};
use super::vocab::{TargetResolver, Writes};
use crate::vista::Vista;

/// Register the write terminals. With [`Writes::Denied`] every verb exists
/// and throws the denial message.
pub(crate) fn register_writes(
    engine: &mut Engine,
    resolver: Option<TargetResolver>,
    writes: Writes,
) {
    match writes {
        Writes::Allowed => register_allowed(engine, resolver),
        Writes::Denied(msg) => register_denied(engine, msg),
    }
}

fn register_allowed(engine: &mut Engine, resolver: Option<TargetResolver>) {
    let r = resolver.clone();
    engine.register_fn(
        "insert",
        move |h: &mut Handle, map: RhaiMap| -> RhaiResult<String> {
            let vista = target(h, r.as_ref(), "insert", |c| c.can_insert)?;
            insert_record(&vista, map_to_record(map)?, None)
        },
    );

    let r = resolver.clone();
    engine.register_fn(
        "upsert",
        move |h: &mut Handle, id: Dynamic, map: RhaiMap| -> RhaiResult<String> {
            let vista = target(h, r.as_ref(), "upsert", |c| c.can_insert && c.can_update)?;
            let id = id_string(id)?;
            run(vista.upsert_value(id.clone(), &map_to_record(map)?))?;
            Ok(id)
        },
    );

    let r = resolver.clone();
    engine.register_fn(
        "patch",
        move |h: &mut Handle, id: Dynamic, map: RhaiMap| -> RhaiResult<bool> {
            let vista = target(h, r.as_ref(), "patch", |c| c.can_update)?;
            let rec = map_to_record(map)?;
            found(super::bridge::block_on(
                vista.patch_value(id_string(id)?, &rec),
            ))
        },
    );

    let r = resolver.clone();
    engine.register_fn(
        "delete",
        move |h: &mut Handle, id: Dynamic| -> RhaiResult<bool> {
            let vista = h.write_target(r.as_ref()).map_err(rhai_err)?;
            delete_row(&vista, id_string(id)?)
        },
    );

    let r = resolver.clone();
    engine.register_fn(
        "import_from",
        move |h: &mut Handle, source: Handle| -> RhaiResult<RhaiMap> {
            import_rows(&import_target(h, r.as_ref())?, &source, r.as_ref())
        },
    );

    let r = resolver;
    engine.register_fn(
        "import_from",
        move |h: &mut Handle, source: Handle, mapping: RhaiMap| -> RhaiResult<RhaiMap> {
            import_rows_mapped(
                &import_target(h, r.as_ref())?,
                &source,
                &mapping,
                r.as_ref(),
            )
        },
    );
}

fn register_denied(engine: &mut Engine, msg: String) {
    let deny = move || -> Box<EvalAltResult> { msg.clone().into() };
    let d = deny.clone();
    engine.register_fn(
        "insert",
        move |_: &mut Handle, _: RhaiMap| -> RhaiResult<String> { Err(d()) },
    );
    let d = deny.clone();
    engine.register_fn(
        "upsert",
        move |_: &mut Handle, _: Dynamic, _: RhaiMap| -> RhaiResult<String> { Err(d()) },
    );
    let d = deny.clone();
    engine.register_fn(
        "patch",
        move |_: &mut Handle, _: Dynamic, _: RhaiMap| -> RhaiResult<bool> { Err(d()) },
    );
    engine.register_fn("delete", |_: &mut Handle, _: Dynamic| -> RhaiResult<bool> {
        Ok(false)
    });
    let d = deny.clone();
    engine.register_fn(
        "import_from",
        move |_: &mut Handle, _: Handle| -> RhaiResult<RhaiMap> { Err(d()) },
    );
    engine.register_fn(
        "import_from",
        move |_: &mut Handle, _: Handle, _: RhaiMap| -> RhaiResult<RhaiMap> { Err(deny()) },
    );
}

/// The Vista a write goes to, refused when `allowed` says its capabilities
/// don't cover `verb`.
pub(crate) fn target(
    h: &Handle,
    resolver: Option<&TargetResolver>,
    verb: &str,
    allowed: impl Fn(&crate::VistaCapabilities) -> bool,
) -> RhaiResult<Vista> {
    let vista = h.write_target(resolver).map_err(rhai_err)?;
    if !allowed(vista.capabilities()) {
        return Err(unsupported(verb, &vista));
    }
    Ok(vista)
}

fn import_target(h: &Handle, resolver: Option<&TargetResolver>) -> RhaiResult<Vista> {
    target(h, resolver, "import_from", |c| c.can_import || c.can_insert)
}

/// Insert `rec` into `vista` and return the row's id.
///
/// The id is the record's own (the id column, when set); otherwise the
/// backend's, when the id column is flagged `auto` (`insert_return_id`, not
/// retry-safe); otherwise `minted`, or a fresh UUIDv7 — made before the first
/// attempt so a retry reuses it and lands on the idempotent insert.
pub(crate) fn insert_record(
    vista: &Vista,
    mut rec: vantage_types::Record<CborValue>,
    minted: Option<&str>,
) -> RhaiResult<String> {
    let id_col = vista.get_id_column().unwrap_or("id").to_string();
    let explicit = rec
        .get(&id_col)
        .filter(|v| !matches!(v, CborValue::Null))
        .and_then(cbor_id_to_string);
    let id = match explicit {
        Some(id) => id,
        None if vista.has_auto_id() => return run(vista.insert_return_id_value(&rec)),
        None => match minted {
            Some(id) => id.to_string(),
            None => new_id(vista, &id_col)?,
        },
    };
    rec.shift_remove(&id_col);
    run(vista.insert_value(id.clone(), &rec))?;
    Ok(id)
}

/// A client-made UUIDv7 for a row inserted without an id. A numeric id column
/// can't hold one: without the `auto` flag that is an error asking for either.
pub(crate) fn new_id(vista: &Vista, id_col: &str) -> RhaiResult<String> {
    if let Some(column) = vista.get_column(id_col)
        && is_numeric_type(&column.original_type)
    {
        return Err(rhai_err(error!(
            "id column holds numbers: declare server-made ids (flag `auto`) or pass an id",
            column = id_col,
            r#type = column.original_type.as_str()
        )));
    }
    Ok(uuid::Uuid::now_v7().to_string())
}

fn is_numeric_type(t: &str) -> bool {
    let t = t.trim().to_ascii_lowercase();
    let base = t.split('(').next().unwrap_or("");
    matches!(
        base,
        "int"
            | "integer"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "bigint"
            | "smallint"
            | "serial"
            | "bigserial"
            | "number"
            | "numeric"
            | "decimal"
            | "float"
            | "f32"
            | "f64"
            | "double"
            | "real"
    )
}

/// Delete `id` from `vista`'s set: `true` once the row is gone (missing or
/// outside the set counts), `false` when the Vista can't delete.
pub(crate) fn delete_row(vista: &Vista, id: String) -> RhaiResult<bool> {
    if !vista.capabilities().can_delete {
        return Ok(false);
    }
    match super::bridge::block_on(WritableValueSet::delete(vista, id)).and_then(|r| r) {
        Ok(()) => Ok(true),
        Err(e) if e.is_unsupported() => Ok(false),
        Err(e) => Err(rhai_err(e)),
    }
}

/// `true` when the write landed, `false` when the row was missing.
pub(crate) fn found<T>(out: vantage_core::Result<vantage_core::Result<T>>) -> RhaiResult<bool> {
    match out.and_then(|r| r) {
        Ok(_) => Ok(true),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(rhai_err(e)),
    }
}
