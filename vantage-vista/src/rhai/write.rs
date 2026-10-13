//! Write terminals on the `Table` handle: `insert`, `upsert`, `patch`,
//! `delete`, and `import_from` (see [`super::import`]).
//!
//! Writes go to the whole narrowed handle and stay inside its set (see the
//! book's Writes chapter); a handle with `limit` refuses writes.
//!
//! The [`Vista`] gates each write on the capability its method names. A
//! write it refuses, like a write [`Writes::Denied`] turns off, throws.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_dataset::{InsertableValueSet, WritableValueSet};
use vantage_rhai::rhai::{Dynamic, Engine, Map as RhaiMap};
use vantage_types::cbor_id_to_string;

use super::bridge::block_on;
use super::convert::map_to_record;
use super::handle::Handle;
use super::import::{import_rows, import_rows_mapped};
use super::read::{RhaiResult, id_string, rhai_err, run};
use super::vocab::Writes;
use crate::{ColumnFlag, VistaResolver, vista::Vista};

/// Register the write terminals. With [`Writes::Denied`] every verb exists
/// and throws the denial message.
pub(crate) fn register_writes(
    engine: &mut Engine,
    resolver: Option<Arc<dyn VistaResolver>>,
    writes: Writes,
) {
    let gate = Gate::new(resolver, writes);

    let w = gate.clone();
    engine.register_fn(
        "insert",
        move |h: &mut Handle, map: RhaiMap| -> RhaiResult<String> {
            insert_record(&w.target(h)?, map_to_record(map)?, None)
        },
    );

    let w = gate.clone();
    engine.register_fn(
        "upsert",
        move |h: &mut Handle, id: Dynamic, map: RhaiMap| -> RhaiResult<String> {
            let vista = w.target(h)?;
            let id = id_string(id)?;
            run(vista.upsert_value(id.clone(), &map_to_record(map)?))?;
            Ok(id)
        },
    );

    let w = gate.clone();
    engine.register_fn(
        "patch",
        move |h: &mut Handle, id: Dynamic, map: RhaiMap| -> RhaiResult<bool> {
            let vista = w.target(h)?;
            let rec = map_to_record(map)?;
            found(block_on(vista.patch_value(id_string(id)?, &rec)))
        },
    );

    let w = gate.clone();
    engine.register_fn(
        "delete",
        move |h: &mut Handle, id: Dynamic| -> RhaiResult<bool> {
            delete_row(&w.target(h)?, id_string(id)?)
        },
    );

    let w = gate.clone();
    engine.register_fn(
        "import_from",
        move |h: &mut Handle, source: Handle| -> RhaiResult<RhaiMap> {
            import_rows(&w.target(h)?, &source, w.resolver())
        },
    );

    let w = gate;
    engine.register_fn(
        "import_from",
        move |h: &mut Handle, source: Handle, mapping: RhaiMap| -> RhaiResult<RhaiMap> {
            import_rows_mapped(&w.target(h)?, &source, &mapping, w.resolver())
        },
    );
}

/// What a write verb or a record's write goes through: the host's
/// [`Writes`] setting first, then the handle's write target.
#[derive(Clone)]
pub(crate) struct Gate {
    resolver: Option<Arc<dyn VistaResolver>>,
    writes: Writes,
}

impl Gate {
    pub(crate) fn new(resolver: Option<Arc<dyn VistaResolver>>, writes: Writes) -> Self {
        Self { resolver, writes }
    }

    pub(crate) fn resolver(&self) -> Option<&dyn VistaResolver> {
        self.resolver.as_deref()
    }

    /// The Vista `h` writes to; [`Writes::Denied`] throws its message before
    /// anything resolves.
    pub(crate) fn target(&self, h: &Handle) -> RhaiResult<Vista> {
        match &self.writes {
            Writes::Denied(msg) => Err(msg.clone().into()),
            Writes::Allowed => h.write_target(self.resolver()).map_err(rhai_err),
        }
    }
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
    let id_col = vista.id_column().unwrap_or("id").to_string();
    let explicit = rec
        .get(&id_col)
        .filter(|v| !matches!(v, CborValue::Null))
        .and_then(cbor_id_to_string);
    let id = match explicit {
        Some(id) => id,
        None if has_auto_id(vista) => return run(vista.insert_return_id_value(&rec)),
        None => match minted {
            Some(id) => id.to_string(),
            None => new_id(vista, &id_col)?,
        },
    };
    rec.shift_remove(&id_col);
    run(vista.insert_value(id.clone(), &rec))?;
    Ok(id)
}

/// Whether the backend makes this table's ids: the id column is flagged
/// [`Auto`](ColumnFlag::Auto).
pub(crate) fn has_auto_id(vista: &Vista) -> bool {
    vista
        .id_column()
        .and_then(|c| vista.column(c))
        .is_some_and(|c| c.has_flag(ColumnFlag::Auto))
}

/// A client-made UUIDv7 for a row inserted without an id. A numeric id column
/// can't hold one: without the `auto` flag that is an error asking for either.
/// An id column with no declared type isn't numeric.
pub(crate) fn new_id(vista: &Vista, id_col: &str) -> RhaiResult<String> {
    if let Some(ty) = vista
        .column(id_col)
        .and_then(|c| c.original_type.as_deref())
        && is_numeric_type(ty)
    {
        return Err(rhai_err(error!(
            "id column holds numbers: declare server-made ids (flag `auto`) or pass an id",
            column = id_col,
            r#type = ty
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
/// outside the set counts). A Vista that can't delete throws.
pub(crate) fn delete_row(vista: &Vista, id: String) -> RhaiResult<bool> {
    run(WritableValueSet::delete(vista, id))?;
    Ok(true)
}

/// `true` when the write landed, `false` when the row was missing.
pub(crate) fn found<T>(out: vantage_core::Result<vantage_core::Result<T>>) -> RhaiResult<bool> {
    match out.and_then(|r| r) {
        Ok(_) => Ok(true),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(rhai_err(e)),
    }
}
