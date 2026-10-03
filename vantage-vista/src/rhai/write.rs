//! Write terminals on the `Table` handle: `insert`, `upsert`, `patch`,
//! `delete`, and `import_from` (see [`super::import`]).
//!
//! Writes go to the handle's base table (or the target of its last `ref`);
//! `where`/`sort`/`search`/`limit` don't filter them.

use ciborium::Value as CborValue;
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
            insert_record(&vista, map_to_record(map)?)
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
            let vista = target(h, r.as_ref(), "delete", |c| c.can_delete)?;
            found(super::bridge::block_on(WritableValueSet::delete(
                &vista,
                id_string(id)?,
            )))
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
    let d = deny.clone();
    engine.register_fn(
        "delete",
        move |_: &mut Handle, _: Dynamic| -> RhaiResult<bool> { Err(d()) },
    );
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

/// Insert `rec` into `vista`: an explicit, non-null value in the id column is
/// used as given (an existing row with that id is an error); otherwise the
/// backend assigns one. Shared by `insert(#{…})` and a new record's `save()`.
pub(crate) fn insert_record(
    vista: &Vista,
    mut rec: vantage_types::Record<CborValue>,
) -> RhaiResult<String> {
    let id_col = vista.get_id_column().unwrap_or("id").to_string();
    let explicit = rec
        .get(&id_col)
        .filter(|v| !matches!(v, CborValue::Null))
        .and_then(cbor_id_to_string);
    match explicit {
        Some(id) => {
            rec.shift_remove(&id_col);
            run(vista.insert_value(id.clone(), &rec))?;
            Ok(id)
        }
        None => run(vista.insert_return_id_value(&rec)),
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
