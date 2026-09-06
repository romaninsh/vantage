//! Conventional Rhai vocabulary over the type-erased [`Vista`].
//!
//! vantage-vista owns the *backend-agnostic* vocabulary: `table(name)` resolves
//! a fresh target [`Vista`] through an injected [`TargetResolver`], and a small
//! set of builder verbs narrow it in place. Backends layer their vendor-specific
//! vocabulary (expression syntax, `with_condition`) on top by overriding
//! [`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions).
//!
//! Everything runs on a [`vantage_rhai::Host`]: the vocabulary is a
//! [`Vocab`] ([`ConventionalVocab`]), every script slot compiles once through
//! the host's cache, and each evaluation pushes its variables through an
//! [`Env`] — the parent `row`, the vista as `self`.
//!
//! This keeps Rhai out of vantage-table and lets engine-less datasources
//! (CSV/Mongo/REST) still script the conventional verbs — they only lose the
//! vendor expression syntax. Graceful degradation, not all-or-nothing.
//!
//! Everything here uses only [`Vista`]'s public API, preserving the one-way
//! `vantage-table → vantage-vista` dependency (Rhai is a leaf).

use std::sync::{Arc, Mutex, OnceLock};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult, Map as RhaiMap};
use vantage_rhai::{Block, Compiled, Env, Host, Limits, Vocab};
use vantage_types::Record;

use super::convert::{dynamic_to_cbor, map_to_record, record_to_dynamic};
use crate::{sort::SortDirection, vista::Vista};

/// A [`Vista`] handle usable from Rhai: `Clone + Send + Sync + 'static` via
/// `Arc<Mutex<…>>`, with interior mutability so the builder verbs narrow it in
/// place and return the same handle for chaining. The inner `Option` lets
/// [`eval_ref_script`] move the finished `Vista` out even if the script kept
/// extra references.
///
/// `Arc<Mutex<…>>` and not `Rc<RefCell<…>>`: `vantage-rhai` compiles rhai with
/// its `sync` feature, so anything a script holds must be `Send + Sync`.
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

    /// Take the `Vista` out, leaving the handle empty. Errors if a script
    /// already consumed it.
    pub fn take(&self, what: &str) -> Result<Vista> {
        self.0
            .lock()
            .map_err(|_| error!(format!("{what}: result mutex poisoned")))?
            .take()
            .ok_or_else(|| error!(format!("{what}: vista already consumed")))
    }
}

/// Resolve a table name to a fresh, unconditioned target [`Vista`]. Injected by
/// the backend, which owns the by-name catalog; vantage-vista stays
/// backend-agnostic behind this boxed closure.
pub type TargetResolver = Arc<dyn Fn(&str) -> Result<Vista> + Send + Sync>;

/// The conventional vocabulary as a [`Vocab`]: `table(name)` backed by the
/// resolver plus the builder verbs. Register it *after* a vendor vocabulary
/// so its `table` wins over SurrealDB's `table` alias for `ident`.
pub struct ConventionalVocab(pub TargetResolver);

impl Vocab for ConventionalVocab {
    fn register(&self, engine: &mut Engine) {
        register_conventional_onto(engine, self.0.clone());
    }
}

/// A shell's vendor vocabulary
/// ([`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions))
/// as a [`Vocab`], so a host can be built from a vista in hand.
pub struct ShellVocab<'a>(pub &'a Vista);

impl Vocab for ShellVocab<'_> {
    fn register(&self, engine: &mut Engine) {
        self.0.source.register_rhai_extensions(engine);
    }
}

/// Register the conventional `Vista` vocabulary onto `engine`.
///
/// Adds the `table(name)` constructor (backed by `resolver`) and the in-place
/// builder verbs (`with_id`, `add_condition_eq`, `add_order`, `add_search`,
/// `set_page_size`, `get_ref`). Each verb returns the same handle so scripts can
/// chain: `table("order").add_condition_eq("client", row.id).add_order("date", "desc")`.
///
/// Prefer [`ConventionalVocab`] on a [`Host`] — this is the registration behind it.
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

    // `add_condition("col", "ne", value)` — the operator form. `op` is a
    // symbolic name (`ne`/`gt`/`in`/…); `in`/`not_in` take a list value. A
    // driver that can't push the operator returns Unimplemented, which surfaces
    // as a Rhai error the caller can handle (or gate on `can_filter_operators`).
    engine.register_fn(
        "add_condition",
        |v: &mut RhaiVista,
         field: &str,
         op: &str,
         value: Dynamic|
         -> std::result::Result<RhaiVista, Box<EvalAltResult>> {
            let op = parse_op(op)?;
            let cbor = dynamic_to_cbor(value)?;
            let field = field.to_string();
            with_inner(v, move |vista| vista.add_condition(field, op, cbor))
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

/// Compile a script slot through the host's cache, wording the error for the
/// slot that failed.
fn compile(host: &Host, what: &str, code: &str) -> Result<Compiled<Block>> {
    host.compile(&Block::from(code))
        .map_err(|e| error!(format!("{what} failed to compile: {e}")))
}

/// Evaluate a reference build-script and return the `Vista` it produced.
///
/// `host` must carry the conventional vocabulary ([`ConventionalVocab`]) plus
/// any vendor extensions. `env` is the vendor's base environment (see
/// [`TableShell::rhai_env`](crate::TableShell::rhai_env)); the parent `row` is
/// pushed onto it. The script's final expression must evaluate to a `Vista`
/// (e.g. `table("order").add_…(…)`).
pub fn eval_ref_script(
    host: &Host,
    code: &str,
    env: Env,
    row: &Record<CborValue>,
) -> Result<Vista> {
    let script = compile(host, "rhai reference build-script", code)?;
    let env = env.var("row", record_to_dynamic(row));
    let result = script
        .eval(&env)
        .map_err(|e| error!(format!("rhai reference build-script failed: {e}")))?;
    let handle: RhaiVista = result
        .try_cast::<RhaiVista>()
        .ok_or_else(|| error!("rhai reference build-script did not return a Vista"))?;
    handle.take("rhai reference build-script")
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
/// `host` must carry the conventional vocabulary plus any vendor extensions;
/// the vista's own shell contributes its constants via `rhai_env`.
pub fn eval_modify_script(host: &Host, code: &str, vista: Vista) -> Result<Vista> {
    let script = compile(host, "rhai modify script", code)?;
    let env = vista.source.rhai_env(Env::new());
    let handle = RhaiVista::wrap(vista);
    script
        .run(&env.var("self", Dynamic::from(handle.clone())))
        .map_err(|e| error!(format!("rhai modify script failed: {e}")))?;
    // `take()` succeeds regardless of the scope's lingering `Arc` clone — it
    // empties the shared `Option`, not the `Arc`.
    handle.take("rhai modify script")
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
/// in scope.
pub fn eval_augment_source(
    host: &Host,
    code: &str,
    base: Vista,
    row: &Record<CborValue>,
) -> Result<Vista> {
    let script = compile(host, "rhai augment source script", code)?;
    eval_augment_compiled(&script, base, row)
}

fn eval_augment_compiled(
    script: &Compiled<Block>,
    base: Vista,
    row: &Record<CborValue>,
) -> Result<Vista> {
    let env = base.source.rhai_env(Env::new());
    let handle = RhaiVista::wrap(base);
    script
        .run(
            &env.var("self", Dynamic::from(handle.clone()))
                .var("row", record_to_dynamic(row)),
        )
        .map_err(|e| error!(format!("rhai augment source script failed: {e}")))?;
    handle.take("rhai augment source")
}

/// A lazy-expression value closure: given the record as built so far, compute
/// one column value. This is the CBOR-carrier form of
/// `vantage_table::Table::with_lazy_expression`'s callback — driver factories
/// adapt it to their native value type when lowering a spec's `lazy:` script.
pub type LazyValueFn = Arc<dyn Fn(&Record<CborValue>) -> Result<CborValue> + Send + Sync>;

/// Evaluate a lazy-expression script for one record. The record as built so
/// far — source columns plus any lazy columns declared earlier — is exposed
/// as the `row` map; the script's final expression becomes the column's
/// value:
///
/// ```rhai
/// row.contents.split("\n").len() - 1
/// ```
pub fn eval_lazy_expression(host: &Host, code: &str, row: &Record<CborValue>) -> Result<CborValue> {
    let script = compile(host, "rhai lazy expression", code)?;
    eval_lazy_compiled(&script, row)
}

fn eval_lazy_compiled(script: &Compiled<Block>, row: &Record<CborValue>) -> Result<CborValue> {
    let result = script
        .eval(&Env::new().var("row", record_to_dynamic(row)))
        .map_err(|e| error!(format!("rhai lazy expression failed: {e}")))?;
    dynamic_to_cbor(result).map_err(|e| error!(format!("rhai lazy expression result: {e}")))
}

/// Build a reusable [`LazyValueFn`] from a Rhai `code` string. Compiles once,
/// here — a script that does not parse fails the table build, not the first
/// row.
pub fn lazy_value_closure(code: &str) -> Result<LazyValueFn> {
    let script = compile(
        vantage_rhai::background_host(),
        "rhai lazy expression",
        code,
    )?;
    Ok(Arc::new(
        move |row: &Record<CborValue>| -> Result<CborValue> { eval_lazy_compiled(&script, row) },
    ))
}

/// Build a reusable [`AugmentSourceFn`] from a Rhai `code` string and a
/// `resolver` for `table(name)`. Keeps all Rhai host assembly inside
/// vantage-vista: a consumer (diorama's augmentation lowering) only flips the
/// `rhai` feature and calls this — it never touches the `rhai` crate directly.
///
/// Vendor extensions come from the `base` vista's shell, which is only in hand
/// per call, so the host is built on the first call and reused after: one
/// augmentation always narrows the same detail table.
pub fn augment_source_closure(resolver: TargetResolver, code: String) -> AugmentSourceFn {
    let compiled: OnceLock<Result<Compiled<Block>>> = OnceLock::new();
    Arc::new(
        move |row: &Record<CborValue>, base: Vista| -> Result<Vista> {
            let script = compiled.get_or_init(|| {
                // Vendor vocab first, conventional second, so `table(name)`
                // resolves a Vista rather than a vendor identifier.
                let host = Host::builder(Limits::background())
                    .vocab(ShellVocab(&base))
                    .vocab(ConventionalVocab(resolver.clone()))
                    .build();
                compile(&host, "rhai augment source script", &code)
            });
            match script {
                Ok(script) => eval_augment_compiled(script, base, row),
                Err(e) => Err(error!(e.to_string())),
            }
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

fn parse_op(op: &str) -> std::result::Result<crate::FilterOp, Box<EvalAltResult>> {
    use crate::FilterOp;
    Ok(match op.to_ascii_lowercase().as_str() {
        "eq" | "=" | "==" => FilterOp::Eq,
        "ne" | "!=" | "<>" => FilterOp::Ne,
        "gt" | ">" => FilterOp::Gt,
        "gte" | ">=" => FilterOp::Gte,
        "lt" | "<" => FilterOp::Lt,
        "lte" | "<=" => FilterOp::Lte,
        "in" | "in_set" => FilterOp::InSet,
        "not_in" | "not_in_set" | "nin" | "!in" => FilterOp::NotInSet,
        other => {
            return Err(format!(
                "invalid filter operator '{other}' (expected eq/ne/gt/gte/lt/lte/in/not_in)"
            )
            .into());
        }
    })
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

    fn resolver() -> TargetResolver {
        Arc::new(|name: &str| {
            if name == "users" {
                Ok(users_vista())
            } else {
                Err(error!("unknown table in test resolver", table = name))
            }
        })
    }

    fn host() -> Host {
        Host::builder(Limits::background())
            .vocab(ConventionalVocab(resolver()))
            .build()
    }

    #[tokio::test]
    async fn script_narrows_target_with_literal_condition() {
        let row = record(&[("id", cbor_text("1"))]);
        let vista = eval_ref_script(
            &host(),
            r#"table("users").add_condition_eq("vip_flag", true)"#,
            Env::new(),
            &row,
        )
        .unwrap();

        let rows = vista.list_values().await.unwrap();
        assert_eq!(rows.len(), 2, "only the two VIP rows should survive");
        assert!(rows.contains_key("1") && rows.contains_key("3"));
    }

    #[tokio::test]
    async fn add_condition_verb_dispatches_via_operator_name() {
        // `add_condition(.., "eq", ..)` routes through the same eq path as the
        // dedicated verb (MockShell only pushes equality) — proving the verb,
        // `parse_op`, and dispatch are wired.
        let row = record(&[("id", cbor_text("1"))]);
        let vista = eval_ref_script(
            &host(),
            r#"table("users").add_condition("vip_flag", "eq", true)"#,
            Env::new(),
            &row,
        )
        .unwrap();
        let rows = vista.list_values().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains_key("1") && rows.contains_key("3"));
    }

    #[test]
    fn add_condition_rejects_unknown_operator() {
        let row = record(&[("id", cbor_text("1"))]);
        let result = eval_ref_script(
            &host(),
            r#"table("users").add_condition("vip_flag", "wat", true)"#,
            Env::new(),
            &row,
        );
        match result {
            Ok(_) => panic!("expected an error for an unknown operator"),
            Err(e) => assert!(e.to_string().contains("invalid filter operator")),
        }
    }

    #[tokio::test]
    async fn script_can_read_the_parent_row() {
        let row = record(&[("id", cbor_text("3"))]);
        let vista = eval_ref_script(
            &host(),
            r#"table("users").add_condition_eq("id", row.id)"#,
            Env::new(),
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
        let modified =
            eval_modify_script(&host(), r#"self.add_condition_eq("vip_flag", true)"#, vista)
                .unwrap();

        let rows = modified.list_values().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains_key("1") && rows.contains_key("3"));
    }

    #[test]
    fn unknown_table_surfaces_resolver_error() {
        let row = record(&[]);
        let err = match eval_ref_script(&host(), r#"table("ghosts")"#, Env::new(), &row) {
            Ok(_) => panic!("expected the resolver to reject an unknown table"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("unknown table"));
    }

    #[test]
    fn lazy_closure_compiles_once_and_fails_early() {
        let f = lazy_value_closure("row.n * 2").unwrap();
        let row = record(&[("n", CborValue::Integer(21.into()))]);
        assert_eq!(f(&row).unwrap(), CborValue::Integer(42.into()));
        assert!(
            lazy_value_closure("row.n *").is_err(),
            "syntax fails at build"
        );
    }

    #[test]
    fn lazy_expression_is_bounded() {
        let f = lazy_value_closure("loop {}").unwrap();
        let err = f(&record(&[])).unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }

    #[tokio::test]
    async fn augment_closure_narrows_base_per_row() {
        let f =
            augment_source_closure(resolver(), r#"self.add_condition_eq("id", row.key)"#.into());
        let row = record(&[("key", cbor_text("2"))]);
        let narrowed = f(&row, users_vista()).unwrap();
        let rows = narrowed.list_values().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows.contains_key("2"));
        // Second call reuses the compiled script.
        let row = record(&[("key", cbor_text("3"))]);
        let rows = f(&row, users_vista()).unwrap().list_values().await.unwrap();
        assert!(rows.contains_key("3"));
    }
}
