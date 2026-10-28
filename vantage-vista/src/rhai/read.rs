//! Read terminals on the `Table` handle: `list`, `get`, `first`, `count`,
//! `ids`, `columns`, `references`, `capabilities`.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, VantageError};
use vantage_dataset::ReadableValueSet;
use vantage_rhai::rhai::{Array, Dynamic, Engine, EvalAltResult, Map as RhaiMap};
use vantage_types::Record;

use super::bridge::block_on;
use super::convert::record_to_dynamic;
use super::handle::{Handle, RowWindow};
use super::introspect::{capabilities_map, columns_array, references_array};
use crate::{Capability, VistaResolver, vista::Vista};

pub(crate) type RhaiResult<T> = std::result::Result<T, Box<EvalAltResult>>;

/// Register the read terminals. `cap` bounds every `list()` and `ids()`; a
/// `limit(n)` step below it applies as well, and an `offset(n)` step skips
/// rows before either.
pub(crate) fn register_reads(
    engine: &mut Engine,
    resolver: Option<Arc<dyn VistaResolver>>,
    cap: Option<usize>,
) {
    let r = resolver.clone();
    engine.register_fn("list", move |h: &mut Handle| -> RhaiResult<Array> {
        let (vista, rows) = rows(h, r.as_deref(), cap)?;
        Ok(rows
            .iter()
            .map(|(id, rec)| row_dynamic(&vista, id, rec))
            .collect())
    });

    let r = resolver.clone();
    engine.register_fn("ids", move |h: &mut Handle| -> RhaiResult<Array> {
        let (_, rows) = rows(h, r.as_deref(), cap)?;
        Ok(rows.into_iter().map(|(id, _)| id.into()).collect())
    });

    let r = resolver.clone();
    engine.register_fn("first", move |h: &mut Handle| -> RhaiResult<Dynamic> {
        let (vista, rows) = rows(h, r.as_deref(), Some(1))?;
        Ok(rows
            .first()
            .map_or(Dynamic::UNIT, |(id, rec)| row_dynamic(&vista, id, rec)))
    });

    let r = resolver.clone();
    engine.register_fn(
        "get",
        move |h: &mut Handle, id: Dynamic| -> RhaiResult<Dynamic> {
            let id = id_string(id)?;
            let vista = h.resolve(r.as_deref()).map_err(rhai_err)?;
            Ok(match run(vista.get_value(id.clone()))? {
                Some(rec) => row_dynamic(&vista, &id, &rec),
                None => Dynamic::UNIT,
            })
        },
    );

    let r = resolver.clone();
    engine.register_fn("count", move |h: &mut Handle| -> RhaiResult<i64> {
        let vista = h.resolve(r.as_deref()).map_err(rhai_err)?;
        // `offset(n)` and `limit(n)` pick the rows `list` would return;
        // count agrees.
        Ok(h.row_window().count(run(vista.count())?))
    });

    let r = resolver.clone();
    engine.register_fn(
        "capabilities",
        move |h: &mut Handle| -> RhaiResult<RhaiMap> {
            Ok(capabilities_map(
                &h.resolve(r.as_deref()).map_err(rhai_err)?,
            ))
        },
    );

    let r = resolver.clone();
    engine.register_fn("columns", move |h: &mut Handle| -> RhaiResult<Array> {
        Ok(columns_array(&h.resolve(r.as_deref()).map_err(rhai_err)?))
    });

    let r = resolver;
    engine.register_fn("references", move |h: &mut Handle| -> RhaiResult<Array> {
        Ok(references_array(
            &h.resolve(r.as_deref()).map_err(rhai_err)?,
        ))
    });
}

type Rows = Vec<(String, Record<CborValue>)>;

/// Resolve `h` and read its row window, taking at most `cap` rows.
fn rows(
    h: &Handle,
    resolver: Option<&dyn VistaResolver>,
    cap: Option<usize>,
) -> RhaiResult<(Vista, Rows)> {
    let vista = h.resolve(resolver).map_err(rhai_err)?;
    let rows = run(fetch_rows(&vista, h.row_window().capped(cap)))?;
    Ok((vista, rows))
}

/// The rows of `vista` in `window`.
///
/// With a limit, a shell advertising [`Window`](Capability::Window) reads
/// `fetch_window(offset, limit)`; any other reads the leading window
/// `fetch_window(0, offset + limit)`, which every shell that lists answers,
/// and drops the first `offset` rows. Without a limit there is no window to
/// ask for, so the set is listed and the first `offset` rows dropped.
pub(crate) async fn fetch_rows(vista: &Vista, window: RowWindow) -> Result<Rows> {
    let RowWindow { offset, limit } = window;
    let Some(limit) = limit else {
        let rows = vista.list_values().await?;
        return Ok(rows.into_iter().skip(offset).collect());
    };
    let mut rows = if offset == 0 || vista.capabilities().has(Capability::Window) {
        vista.fetch_window(offset, limit).await?.rows
    } else {
        let mut rows = vista
            .fetch_window(0, offset.saturating_add(limit))
            .await?
            .rows;
        rows.drain(..offset.min(rows.len()));
        rows
    };
    rows.truncate(limit);
    Ok(rows)
}

/// A row as a script map, with the id column filled from the row's key.
pub(crate) fn row_dynamic(vista: &Vista, id: &str, rec: &Record<CborValue>) -> Dynamic {
    let col = vista.id_column().unwrap_or("id");
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

pub(crate) fn rhai_err(e: VantageError) -> Box<EvalAltResult> {
    e.to_string().into()
}
