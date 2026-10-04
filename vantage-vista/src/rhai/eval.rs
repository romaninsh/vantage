//! Entry points that evaluate a script slot into a [`Vista`]: reference
//! build-scripts, `modify:` scripts, and augmentation sources.
//!
//! Each host carries [`DataVocab`] with [`Terminals::Describe`](super::Terminals::Describe) plus any
//! backend extensions. The script narrows a handle; the handle it ends on is
//! resolved and returned.

use std::sync::{Arc, OnceLock};

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, VantageError, error};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Compiled, Env, Host, Limits};
use vantage_types::Record;

use super::convert::record_to_dynamic;
use super::handle::Handle;
use super::vocab::{DataVocab, TargetResolver};
use crate::vista::Vista;

/// Compile a script slot through the host's cache, naming the slot on error.
pub(crate) fn compile(host: &Host, what: &str, code: &str) -> Result<Compiled<Block>> {
    host.compile(&Block::from(code))
        .context(error!("rhai script failed to compile", script = what))
}

/// Evaluate a reference build-script and return the Vista it describes.
///
/// `env` is the backend's base environment
/// ([`TableShell::rhai_env`](crate::TableShell::rhai_env)); the parent `row`
/// is pushed onto it. The script must end on a table handle, e.g.
/// `table("order").where("client", row.id)`.
pub fn eval_ref_script(
    host: &Host,
    code: &str,
    env: Env,
    row: &Record<CborValue>,
) -> Result<Vista> {
    const WHAT: &str = "rhai reference build-script";
    let script = compile(host, WHAT, code)?;
    let result = script
        .eval(&env.var("row", record_to_dynamic(row)))
        .context(error!("rhai script failed", script = WHAT))?;
    result
        .try_cast::<Handle>()
        .ok_or_else(|| error!("Script did not return a table handle", script = WHAT))?
        .resolve(None)
}

/// Evaluate a `modify:` script against a built Vista, exposed as `self`.
///
/// When the script ends on a table handle (`self.where("vip", true)`), that
/// handle is resolved and returned. Otherwise the latest handle a backend
/// extension verb made from `self` is used, else `self` as given:
///
/// ```rhai
/// self.with_condition(ident("is_paying_client") == true);
/// ```
pub fn eval_modify_script(host: &Host, code: &str, vista: Vista) -> Result<Vista> {
    const WHAT: &str = "rhai modify script";
    let script = compile(host, WHAT, code)?;
    let env = vista.source.rhai_env(Env::new());
    let base = Handle::over(vista);
    let result = script
        .eval(&env.var("self", Dynamic::from(base.clone())))
        .context(error!("rhai script failed", script = WHAT))?;
    finish(base, result)
}

/// A diorama augmentation source: given a master `row` and a fresh `base`
/// detail Vista, return `base` narrowed for that row.
pub type AugmentSourceFn = Arc<dyn Fn(&Record<CborValue>, Vista) -> Result<Vista> + Send + Sync>;

/// Evaluate an augmentation source script: `self` is the `base` Vista, `row`
/// the master row, as in `self.where("key", row.key)`. The result follows
/// [`eval_modify_script`].
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
    let base = Handle::over(base);
    let result = script
        .eval(
            &env.var("self", Dynamic::from(base.clone()))
                .var("row", record_to_dynamic(row)),
        )
        .context(error!(
            "rhai script failed",
            script = "rhai augment source script"
        ))?;
    finish(base, result)
}

/// The script's handle resolved; else the latest handle an extension verb
/// made from `self`; else the base it was given.
fn finish(base: Handle, result: Dynamic) -> Result<Vista> {
    match result.try_cast::<Handle>() {
        Some(handle) => handle.resolve(None),
        None => match base.latest_extended() {
            Some(handle) => handle.resolve(None),
            None => base.take_base(),
        },
    }
}

/// Build a reusable [`AugmentSourceFn`] from a script and a `resolver` for
/// `table(name)`. Backend extensions come from the `base` Vista's shell, so the
/// host is built on the first call and reused: one augmentation always narrows
/// the same detail table.
pub fn augment_source_closure(resolver: TargetResolver, code: String) -> AugmentSourceFn {
    // The compile error is shared by every later call, so it is kept in an
    // `Arc` and attached as each call's source.
    let compiled: OnceLock<std::result::Result<Compiled<Block>, Arc<VantageError>>> =
        OnceLock::new();
    Arc::new(
        move |row: &Record<CborValue>, base: Vista| -> Result<Vista> {
            let script = compiled.get_or_init(|| {
                let host = Host::builder(Limits::background())
                    .vocab_fn(|engine| base.source.register_rhai_extensions(engine))
                    .vocab(DataVocab::describe(Some(resolver.clone())))
                    .build();
                compile(&host, "rhai augment source script", &code).map_err(Arc::new)
            });
            match script {
                Ok(script) => eval_augment_compiled(script, base, row),
                Err(e) => Err::<Vista, _>(e.clone())
                    .context(error!("Augment source script can't be used")),
            }
        },
    )
}
