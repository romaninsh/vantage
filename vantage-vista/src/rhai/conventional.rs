//! Conventional Rhai vocabulary over the type-erased [`Vista`].
//!
//! vantage-vista owns Rhai engine construction with a *backend-agnostic*
//! vocabulary: `table(name)` resolves a fresh target [`Vista`] through an
//! injected [`TargetResolver`], and a small set of builder verbs narrow it in
//! place. Backends layer their vendor-specific vocabulary (expression syntax,
//! `with_condition`) on top by overriding
//! [`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions).
//!
//! This keeps Rhai out of vantage-table and lets engine-less datasources
//! (CSV/Mongo/REST) still script the conventional verbs — they only lose the
//! vendor expression syntax. Graceful degradation, not all-or-nothing.
//!
//! Everything here uses only [`Vista`]'s public API, preserving the one-way
//! `vantage-table → vantage-vista` dependency (Rhai is a leaf).

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use rhai::{Dynamic, Engine, EvalAltResult, Map as RhaiMap, Scope};
use vantage_core::{Result, error};
use vantage_types::Record;

use super::convert::{dynamic_to_cbor, map_to_record, record_to_dynamic};
use crate::{sort::SortDirection, vista::Vista};

/// A [`Vista`] handle usable from Rhai: `Clone + Send + Sync + 'static` via
/// `Arc<Mutex<…>>`, with interior mutability so the builder verbs narrow it in
/// place and return the same handle for chaining. The inner `Option` lets
/// [`eval_ref_script`] move the finished `Vista` out even if the script kept
/// extra references.
///
/// `Arc<Mutex<…>>` (not `Rc<RefCell<…>>`) so the type satisfies Rhai's
/// `Send + Sync` bound when a consumer compiles Rhai with its `sync` feature —
/// `Vista` is itself `Send + Sync` (its `TableShell` is), so nothing is lost.
#[derive(Clone)]
pub struct RhaiVista(pub Arc<Mutex<Option<Vista>>>);

impl RhaiVista {
    /// Wrap a `Vista` for use inside a script.
    pub fn wrap(vista: Vista) -> Self {
        RhaiVista(Arc::new(Mutex::new(Some(vista))))
    }

    /// Apply an in-place mutation to the wrapped `Vista` and return the same
    /// handle for chaining. Backends call this from
    /// [`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions)
    /// to add vendor verbs (e.g. `with_condition`) without re-deriving the
    /// borrow/`Option` bookkeeping.
    pub fn apply<F>(&self, f: F) -> std::result::Result<RhaiVista, Box<EvalAltResult>>
    where
        F: FnOnce(&mut Vista) -> Result<()>,
    {
        with_inner(self, f)
    }
}

/// Resolve a table name to a fresh, unconditioned target [`Vista`]. Injected by
/// the backend, which owns the by-name catalog; vantage-vista stays
/// backend-agnostic behind this boxed closure.
pub type TargetResolver = Arc<dyn Fn(&str) -> Result<Vista> + Send + Sync>;

/// Register the conventional `Vista` vocabulary onto `engine`.
///
/// Adds the `table(name)` constructor (backed by `resolver`) and the in-place
/// builder verbs (`with_id`, `add_condition_eq`, `add_order`, `add_search`,
/// `set_page_size`, `get_ref`). Each verb returns the same handle so scripts can
/// chain: `table("order").add_condition_eq("client", row.id).add_order("date", "desc")`.
pub fn register_conventional_onto(engine: &mut Engine, resolver: TargetResolver) {
    engine.register_type_with_name::<RhaiVista>("Vista");

    engine.register_fn(
        "table",
        move |name: &str| -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let vista = resolver(name).map_err(to_rhai_err)?;
            Ok(RhaiVista::wrap(vista))
        },
    );

    engine.register_fn(
        "with_id",
        |v: &mut RhaiVista, id: Dynamic| -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let cbor = dynamic_to_cbor(id)?;
            with_inner(v, |vista| vista.with_id(cbor).map(|_| ()))
        },
    );

    engine.register_fn(
        "add_condition_eq",
        |v: &mut RhaiVista,
         field: &str,
         value: Dynamic|
         -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let cbor = dynamic_to_cbor(value)?;
            let field = field.to_string();
            with_inner(v, move |vista| vista.add_condition_eq(field, cbor))
        },
    );

    engine.register_fn(
        "add_order",
        |v: &mut RhaiVista,
         column: &str,
         dir: &str|
         -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let direction = parse_dir(dir)?;
            let column = column.to_string();
            with_inner(v, move |vista| vista.add_order(&column, direction))
        },
    );

    // Single-arg form defaults to ascending.
    engine.register_fn(
        "add_order",
        |v: &mut RhaiVista, column: &str| -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let column = column.to_string();
            with_inner(v, move |vista| {
                vista.add_order(&column, SortDirection::Ascending)
            })
        },
    );

    engine.register_fn(
        "add_search",
        |v: &mut RhaiVista, text: &str| -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let text = text.to_string();
            with_inner(v, move |vista| vista.add_search(text))
        },
    );

    engine.register_fn(
        "set_page_size",
        |v: &mut RhaiVista, size: i64| -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            if size <= 0 {
                return Err("set_page_size: page size must be > 0".into());
            }
            with_inner(v, move |vista| vista.set_page_size(size as usize))
        },
    );

    engine.register_fn(
        "get_ref",
        |v: &mut RhaiVista,
         relation: &str,
         row: RhaiMap|
         -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let record = map_to_record(row)?;
            let guard = lock(v)?;
            let vista = guard
                .as_ref()
                .ok_or_else(|| Box::<EvalAltResult>::from("get_ref: vista already consumed"))?;
            let target = vista.get_ref(relation, &record).map_err(to_rhai_err)?;
            Ok(RhaiVista::wrap(target))
        },
    );
}

/// Evaluate a reference build-script and return the `Vista` it produced.
///
/// `engine` must already have the conventional vocabulary (via
/// [`register_conventional_onto`]) plus any vendor extensions registered. The
/// parent `row` is exposed to the script as the `row` map. The script's final
/// expression must evaluate to a `Vista` (e.g. `table("order").add_…(…)`).
pub fn eval_ref_script(engine: &Engine, code: &str, row: &Record<CborValue>) -> Result<Vista> {
    let mut scope = Scope::new();
    scope.push_dynamic("row", record_to_dynamic(row));

    let result: RhaiVista = engine
        .eval_with_scope(&mut scope, code)
        .map_err(|e| error!(format!("rhai reference build-script failed: {e}")))?;

    result
        .0
        .lock()
        .map_err(|_| error!("rhai reference build-script: result mutex poisoned"))?
        .take()
        .ok_or_else(|| error!("rhai reference build-script did not return a Vista"))
}

/// Evaluate a *modify* script against an already-built [`Vista`], applying extra
/// modifications in place and returning it. The vista is exposed to the script
/// as `self`.
///
/// Unlike [`eval_ref_script`] (which *builds* a target and returns it), this
/// runs the script for its side effects on `self` and ignores the script's
/// return value — the canonical "the YAML built the table, now a sneaky Rhai
/// tweak narrows it" use-case:
///
/// ```rhai
/// self.with_condition(ident("is_paying_client") == true)
/// ```
///
/// `engine` must already carry the conventional vocabulary (via
/// [`register_conventional_onto`]) plus any vendor extensions.
pub fn eval_modify_script(engine: &Engine, code: &str, vista: Vista) -> Result<Vista> {
    let handle = RhaiVista::wrap(vista);
    let mut scope = Scope::new();
    scope.push("self", handle.clone());

    engine
        .run_with_scope(&mut scope, code)
        .map_err(|e| error!(format!("rhai modify script failed: {e}")))?;

    // `take()` succeeds regardless of the scope's lingering `Arc` clone — it
    // empties the shared `Option`, not the `Arc`.
    handle
        .0
        .lock()
        .map_err(|_| error!("rhai modify script: result mutex poisoned"))?
        .take()
        .ok_or_else(|| error!("rhai modify script consumed `self`"))
}

/// A diorama augmentation *source* closure: given a master `row` and a freshly
/// resolved `base` detail [`Vista`], return the `base` narrowed for that row.
/// Hand-written Rust and Rhai both produce this same shape.
pub type AugmentSourceFn = Arc<dyn Fn(&Record<CborValue>, Vista) -> Result<Vista> + Send + Sync>;

/// Evaluate an augmentation *source* script: narrow a pre-built `base` Vista in
/// place using values from the master `row`, and return it. The base is exposed
/// to the script as `self`, the master row as `row` — so a one-liner like
///
/// ```rhai
/// self.add_condition_eq("key", row.key)
/// ```
///
/// is the canonical form. Mirrors [`eval_modify_script`] but with the parent row
/// in scope; the engine must already carry the conventional vocabulary (via
/// [`register_conventional_onto`]) plus any vendor extensions.
pub fn eval_augment_source(
    engine: &Engine,
    code: &str,
    base: Vista,
    row: &Record<CborValue>,
) -> Result<Vista> {
    let handle = RhaiVista::wrap(base);
    let mut scope = Scope::new();
    scope.push("self", handle.clone());
    scope.push_dynamic("row", record_to_dynamic(row));

    engine
        .run_with_scope(&mut scope, code)
        .map_err(|e| error!(format!("rhai augment source script failed: {e}")))?;

    handle
        .0
        .lock()
        .map_err(|_| error!("rhai augment source: result mutex poisoned"))?
        .take()
        .ok_or_else(|| error!("rhai augment source consumed `self`"))
}

/// Build a reusable [`AugmentSourceFn`] from a Rhai `code` string and a
/// `resolver` for `table(name)`. Keeps all Rhai engine assembly inside
/// vantage-vista: a consumer (diorama's augmentation lowering) only flips the
/// `rhai` feature and calls this — it never touches the `rhai` crate directly.
///
/// The engine is rebuilt per call (cheap; `rhai`'s `sync` feature is not assumed,
/// so an [`Engine`] cannot be stored in a `Send + Sync` closure). Vendor
/// extensions come from the supplied `base`'s shell, so scripted narrowing can
/// use a backend's expression syntax when present.
pub fn augment_source_closure(resolver: TargetResolver, code: String) -> AugmentSourceFn {
    Arc::new(
        move |row: &Record<CborValue>, base: Vista| -> Result<Vista> {
            let mut engine = Engine::new();
            base.source.register_rhai_extensions(&mut engine);
            register_conventional_onto(&mut engine, resolver.clone());
            eval_augment_source(&engine, code.as_str(), base, row)
        },
    )
}

// ---- helpers --------------------------------------------------------------

type Guard<'a> = std::sync::MutexGuard<'a, Option<Vista>>;

fn lock(v: &RhaiVista) -> std::result::Result<Guard<'_>, Box<EvalAltResult>> {
    v.0.lock()
        .map_err(|_| Box::<EvalAltResult>::from("RhaiVista mutex poisoned"))
}

/// Apply an in-place mutation to the wrapped `Vista` and return the same handle
/// for chaining.
fn with_inner<F>(v: &RhaiVista, f: F) -> std::result::Result<RhaiVista, Box<EvalAltResult>>
where
    F: FnOnce(&mut Vista) -> Result<()>,
{
    {
        let mut guard = lock(v)?;
        let vista = guard
            .as_mut()
            .ok_or_else(|| Box::<EvalAltResult>::from("vista already consumed in script"))?;
        f(vista).map_err(to_rhai_err)?;
    }
    Ok(v.clone())
}

fn parse_dir(dir: &str) -> std::result::Result<SortDirection, Box<EvalAltResult>> {
    match dir.to_ascii_lowercase().as_str() {
        "asc" | "ascending" => Ok(SortDirection::Ascending),
        "desc" | "descending" => Ok(SortDirection::Descending),
        other => Err(format!("invalid sort direction '{other}' (expected 'asc' or 'desc')").into()),
    }
}

fn to_rhai_err(e: vantage_core::VantageError) -> Box<EvalAltResult> {
    Box::<EvalAltResult>::from(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, VistaMetadata, mocks::MockShell};
    use vantage_dataset::ReadableValueSet;

    fn cbor_text(s: &str) -> CborValue {
        CborValue::Text(s.into())
    }

    fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    /// Fresh `users` Vista with three seeded rows (two VIPs). Built anew on each
    /// call so the resolver hands out an unconditioned target every time.
    fn users_vista() -> Vista {
        let source = MockShell::new()
            .with_record(
                "1",
                record(&[
                    ("id", cbor_text("1")),
                    ("name", cbor_text("Alice")),
                    ("vip_flag", CborValue::Bool(true)),
                ]),
            )
            .with_record(
                "2",
                record(&[
                    ("id", cbor_text("2")),
                    ("name", cbor_text("Bob")),
                    ("vip_flag", CborValue::Bool(false)),
                ]),
            )
            .with_record(
                "3",
                record(&[
                    ("id", cbor_text("3")),
                    ("name", cbor_text("Carol")),
                    ("vip_flag", CborValue::Bool(true)),
                ]),
            );
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String").with_flag("title"))
            .with_column(Column::new("vip_flag", "bool"))
            .with_id_column("id");
        Vista::new("users", Box::new(source.with_metadata(metadata)))
    }

    fn engine() -> Engine {
        let resolver: TargetResolver = Arc::new(|name: &str| {
            if name == "users" {
                Ok(users_vista())
            } else {
                Err(error!("unknown table in test resolver", table = name))
            }
        });
        let mut engine = Engine::new();
        register_conventional_onto(&mut engine, resolver);
        engine
    }

    #[tokio::test]
    async fn script_narrows_target_with_literal_condition() {
        let row = record(&[("id", cbor_text("1"))]);
        let vista = eval_ref_script(
            &engine(),
            r#"table("users").add_condition_eq("vip_flag", true)"#,
            &row,
        )
        .unwrap();

        let rows = vista.list_values().await.unwrap();
        assert_eq!(rows.len(), 2, "only the two VIP rows should survive");
        assert!(rows.contains_key("1") && rows.contains_key("3"));
    }

    #[tokio::test]
    async fn script_can_read_the_parent_row() {
        let row = record(&[("id", cbor_text("3"))]);
        let vista = eval_ref_script(
            &engine(),
            r#"table("users").add_condition_eq("id", row.id)"#,
            &row,
        )
        .unwrap();

        let rows = vista.list_values().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows["3"].get("name"), Some(&cbor_text("Carol")));
    }

    #[tokio::test]
    async fn modify_script_tweaks_an_existing_vista() {
        // The YAML built `users`; a post-build modify script narrows it in place
        // via `self`, with no parent row in scope.
        let vista = users_vista();
        let modified = eval_modify_script(
            &engine(),
            r#"self.add_condition_eq("vip_flag", true)"#,
            vista,
        )
        .unwrap();

        let rows = modified.list_values().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains_key("1") && rows.contains_key("3"));
    }

    #[test]
    fn unknown_table_surfaces_resolver_error() {
        let row = record(&[]);
        let err = match eval_ref_script(&engine(), r#"table("ghosts")"#, &row) {
            Ok(_) => panic!("expected the resolver to reject an unknown table"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("unknown table"));
    }
}
