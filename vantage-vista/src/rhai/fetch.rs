//! Read-only **terminal** fetch verbs over a [`Vista`].
//!
//! [`register_conventional_onto`](crate::register_conventional_onto) gives Rhai
//! a chainable *query builder* — `table(name)`, `add_condition_eq`, `add_order`,
//! `add_search`, `set_page_size`, `get_ref(relation, row)` — but those verbs
//! only *narrow* a [`RhaiVista`]; they never fetch. This module adds the missing
//! terminal verbs that actually read data and hand it back to the script:
//! `list()`, `get_some()`, `count()`, plus the introspection verbs
//! `capabilities()`, `columns()`, `references()`.
//!
//! It stays backend-agnostic: it knows only [`Vista`] and the injected
//! `TargetResolver`. Per-driver vocabulary (SurrealDB/SQL expression syntax) is
//! layered inside Vista via `TableShell::register_rhai_extensions`, so nothing
//! vendor-specific leaks here.
//!
//! Rhai is synchronous; the Vista fetch methods are async. The verbs bridge that
//! with [`block_on`], which is only legal because [`run_script`](crate::run_script)
//! evaluates inside `tokio::task::spawn_blocking` — see that runner for why.

use ciborium::Value as CborValue;
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map as RhaiMap};
use vantage_dataset::ReadableValueSet;
use vantage_types::Record;

use super::conventional::RhaiVista;
use super::convert::record_to_dynamic;
use super::introspect::{capabilities_map, columns_array, references_array};
use crate::vista::Vista;

type RhaiResult<T> = Result<T, Box<EvalAltResult>>;

/// Register the terminal fetch + introspection verbs onto `engine`. The engine
/// must already carry the conventional vocabulary (see
/// [`register_conventional_onto`](crate::register_conventional_onto)). `limit`
/// caps every `list()`. Each verb is read-only.
pub fn register_fetch_verbs(engine: &mut Engine, limit: usize) {
    // `list()` → array of record-maps, capped at `limit`.
    engine.register_fn("list", move |v: &mut RhaiVista| -> RhaiResult<Array> {
        read_vista(v, |vista| {
            let rows = block_on(fetch_capped(vista, limit)).map_err(vantage_err)?;
            Ok(rows
                .iter()
                .map(|(_id, rec)| record_to_dynamic(rec))
                .collect())
        })
    });

    // `get_some()` → one record-map, or `()` when the set is empty. This is the
    // `load_some` half of the traversal idiom.
    engine.register_fn("get_some", |v: &mut RhaiVista| -> RhaiResult<Dynamic> {
        read_vista(v, |vista| {
            match block_on(vista.get_some_value()).map_err(vantage_err)? {
                Some((_id, rec)) => Ok(record_to_dynamic(&rec)),
                None => Ok(Dynamic::UNIT),
            }
        })
    });

    // `count()` → grand total (i64). Errors when the backend can't count.
    engine.register_fn("count", |v: &mut RhaiVista| -> RhaiResult<i64> {
        read_vista(v, |vista| {
            if !vista.capabilities().can_count {
                return Err("count: this backend does not support counting".into());
            }
            block_on(vista.get_count()).map_err(vantage_err)
        })
    });

    // `capabilities()` → map of the Vista's capability flags.
    engine.register_fn("capabilities", |v: &mut RhaiVista| -> RhaiResult<RhaiMap> {
        read_vista(v, |vista| Ok(capabilities_map(vista)))
    });

    // `columns()` → array of `{ name, type, flags }`.
    engine.register_fn("columns", |v: &mut RhaiVista| -> RhaiResult<Array> {
        read_vista(v, |vista| Ok(columns_array(vista)))
    });

    // `references()` → array of `{ name, kind, contained }`.
    engine.register_fn("references", |v: &mut RhaiVista| -> RhaiResult<Array> {
        read_vista(v, |vista| Ok(references_array(vista)))
    });
}

/// Fetch at most `limit` rows. Prefers `fetch_window(0, limit)` when the driver
/// advertises it (true random-access lazy loading — REST/SQL window); otherwise
/// falls back to a full `list_values` truncated to `limit`. The truncation keeps
/// the *returned* payload bounded even when the backend over-fetches; a script
/// that wants the backend itself to limit can call `.set_page_size(n)` first.
async fn fetch_capped(
    vista: &Vista,
    limit: usize,
) -> vantage_core::Result<Vec<(String, Record<CborValue>)>> {
    if vista.capabilities().can_fetch_window {
        match vista.fetch_window(0, limit).await {
            Ok(rows) => return Ok(rows),
            // Some shells advertise the window capability but refuse it in
            // practice (notably the diorama cache shell, which passes the
            // flag through from its master). Fall back to a truncated list
            // rather than failing the script.
            Err(e) if e.is_unsupported() => {}
            Err(e) => return Err(e),
        }
    }
    let all = vista.list_values().await?;
    Ok(all.into_iter().take(limit).collect())
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Handle::current().block_on(fut)
}

/// Lock a [`RhaiVista`] and run `f` with the borrowed [`Vista`]. The lock is
/// held across `f` (and thus across any `block_on` inside it) — fine, since the
/// guard is a plain `std::sync::Mutex` and evaluation is single-threaded.
fn read_vista<T>(v: &RhaiVista, f: impl FnOnce(&Vista) -> RhaiResult<T>) -> RhaiResult<T> {
    let guard =
        v.0.lock()
            .map_err(|_| Box::<EvalAltResult>::from("RhaiVista mutex poisoned"))?;
    let vista = guard
        .as_ref()
        .ok_or_else(|| Box::<EvalAltResult>::from("vista already consumed in script"))?;
    f(vista)
}

fn vantage_err(e: vantage_core::VantageError) -> Box<EvalAltResult> {
    Box::<EvalAltResult>::from(e.to_string())
}
