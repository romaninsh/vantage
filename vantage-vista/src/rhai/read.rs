//! Read terminals on the `Table` handle: `list`, `get`, `first`, `count`,
//! `ids`, `columns`, `references`, `capabilities`.

use ciborium::Value as CborValue;
use vantage_core::{Result, VantageError};
use vantage_dataset::ReadableValueSet;
use vantage_rhai::rhai::{Array, Dynamic, Engine, EvalAltResult, Map as RhaiMap};
use vantage_types::Record;

use super::bridge::block_on;
use super::convert::record_to_dynamic;
use super::handle::Handle;
use super::introspect::{capabilities_map, columns_array, references_array};
use super::vocab::TargetResolver;
use crate::vista::Vista;

pub(crate) type RhaiResult<T> = std::result::Result<T, Box<EvalAltResult>>;

/// Register the read terminals. `cap` bounds every `list()` and `ids()`; a
/// `limit(n)` step below it applies as well.
pub(crate) fn register_reads(
    engine: &mut Engine,
    resolver: Option<TargetResolver>,
    cap: Option<usize>,
) {
    let r = resolver.clone();
    engine.register_fn("list", move |h: &mut Handle| -> RhaiResult<Array> {
        let (vista, rows) = rows(h, r.as_ref(), cap)?;
        Ok(rows
            .iter()
            .map(|(id, rec)| row_dynamic(&vista, id, rec))
            .collect())
    });

    let r = resolver.clone();
    engine.register_fn("ids", move |h: &mut Handle| -> RhaiResult<Array> {
        let (_, rows) = rows(h, r.as_ref(), cap)?;
        Ok(rows.into_iter().map(|(id, _)| id.into()).collect())
    });

    let r = resolver.clone();
    engine.register_fn("first", move |h: &mut Handle| -> RhaiResult<Dynamic> {
        let (vista, rows) = rows(h, r.as_ref(), Some(1))?;
        Ok(rows
            .first()
            .map_or(Dynamic::UNIT, |(id, rec)| row_dynamic(&vista, id, rec)))
    });

    let r = resolver.clone();
    engine.register_fn(
        "get",
        move |h: &mut Handle, id: Dynamic| -> RhaiResult<Dynamic> {
            let id = id_string(id)?;
            let vista = h.resolve(r.as_ref()).map_err(rhai_err)?;
            Ok(match run(vista.get_value(id.clone()))? {
                Some(rec) => row_dynamic(&vista, &id, &rec),
                None => Dynamic::UNIT,
            })
        },
    );

    let r = resolver.clone();
    engine.register_fn("count", move |h: &mut Handle| -> RhaiResult<i64> {
        let vista = h.resolve(r.as_ref()).map_err(rhai_err)?;
        if !vista.capabilities().can_count {
            return Err(unsupported("count", &vista));
        }
        run(vista.get_count())
    });

    let r = resolver.clone();
    engine.register_fn(
        "capabilities",
        move |h: &mut Handle| -> RhaiResult<RhaiMap> {
            Ok(capabilities_map(&h.resolve(r.as_ref()).map_err(rhai_err)?))
        },
    );

    let r = resolver.clone();
    engine.register_fn("columns", move |h: &mut Handle| -> RhaiResult<Array> {
        Ok(columns_array(&h.resolve(r.as_ref()).map_err(rhai_err)?))
    });

    let r = resolver;
    engine.register_fn("references", move |h: &mut Handle| -> RhaiResult<Array> {
        Ok(references_array(&h.resolve(r.as_ref()).map_err(rhai_err)?))
    });
}

type Rows = Vec<(String, Record<CborValue>)>;

/// Resolve `h` and read at most the smaller of `cap` and its own `limit(n)`.
fn rows(
    h: &Handle,
    resolver: Option<&TargetResolver>,
    cap: Option<usize>,
) -> RhaiResult<(Vista, Rows)> {
    let vista = h.resolve(resolver).map_err(rhai_err)?;
    let limit = match (cap, h.row_limit()) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let rows = run(fetch_capped(&vista, limit))?;
    Ok((vista, rows))
}

/// Read at most `limit` rows. Uses `fetch_window(0, limit)` when the driver
/// advertises it, otherwise a full `list_values` truncated to `limit`.
pub(crate) async fn fetch_capped(vista: &Vista, limit: Option<usize>) -> Result<Rows> {
    let Some(limit) = limit else {
        return Ok(vista.list_values().await?.into_iter().collect());
    };
    if vista.capabilities().can_fetch_window {
        match vista.fetch_window(0, limit).await {
            Ok(mut rows) => {
                rows.truncate(limit);
                return Ok(rows);
            }
            // Some shells advertise the window but refuse it (the diorama
            // cache shell passes the flag through from its master).
            Err(e) if e.is_unsupported() => {}
            Err(e) => return Err(e),
        }
    }
    Ok(vista.list_values().await?.into_iter().take(limit).collect())
}

/// A row as a script map, with the id column filled from the row's key.
pub(crate) fn row_dynamic(vista: &Vista, id: &str, rec: &Record<CborValue>) -> Dynamic {
    let col = vista.get_id_column().unwrap_or("id");
    if rec.contains_key(col) {
        return record_to_dynamic(rec);
    }
    let mut rec = rec.clone();
    rec.insert(col.to_string(), CborValue::Text(id.to_string()));
    record_to_dynamic(&rec)
}

/// Run a Vista future through the bridge, flattening both error layers.
pub(crate) fn run<T>(fut: impl Future<Output = Result<T>>) -> RhaiResult<T> {
    block_on(fut).and_then(|r| r).map_err(rhai_err)
}

/// A script id: a string as is, a number in its decimal form.
pub(crate) fn id_string(id: Dynamic) -> RhaiResult<String> {
    if id.is_string() {
        return Ok(id.into_string().expect("string"));
    }
    if id.is_int() {
        return Ok(id.as_int().expect("int").to_string());
    }
    Err(format!(
        "an id must be a string or an integer, not {}",
        id.type_name()
    )
    .into())
}

pub(crate) fn unsupported(verb: &str, vista: &Vista) -> Box<EvalAltResult> {
    format!("`{verb}` isn't supported by table `{}`", vista.name()).into()
}

pub(crate) fn rhai_err(e: VantageError) -> Box<EvalAltResult> {
    e.to_string().into()
}
