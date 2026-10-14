//! Entry points that evaluate a script slot into a [`Vista`]: reference
//! build-scripts and `modify:` scripts.
//!
//! Each host carries [`DataVocab`](super::DataVocab) with
//! [`Terminals::Describe`](super::Terminals::Describe) plus any backend
//! extensions. The script narrows a handle and must end on one; that handle
//! is resolved and returned. A statement that builds a handle and drops it
//! is refused when the script compiles (see [`reject_dropped_narrowing`]).

mod discard;

pub use discard::reject_dropped_narrowing;

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, error};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Compiled, Env, Host};
use vantage_types::Record;

use super::convert::record_to_dynamic;
use super::handle::Handle;
use crate::vista::Vista;

const MODIFY: &str = "rhai modify script";

/// Compile a script slot through the host's cache, naming the slot on error.
pub(crate) fn compile(host: &Host, what: &str, code: &str) -> Result<Compiled<Block>> {
    host.compile(&Block::from(code))
        .context(error!("rhai script failed to compile", script = what))
}

/// Compile a script whose value is a table handle, refusing one that drops
/// a handle it built.
fn compile_handle_script(host: &Host, what: &'static str, code: &str) -> Result<Compiled<Block>> {
    let script = compile(host, what, code)?;
    // A `Block` compiles to one AST; a script without one can't be checked,
    // so it is refused rather than run unchecked.
    let ast = script.ast().ok_or_else(|| {
        error!(
            "rhai script compiled to no single AST to check for dropped handles",
            script = what
        )
    })?;
    reject_dropped_narrowing(ast, what, true)?;
    Ok(script)
}

/// Evaluate a reference build-script and return the Vista it describes.
///
/// `env` is the caller's base environment; the parent `row` is pushed onto
/// it. The script must end on a table handle, e.g.
/// `table("order").where("client", row.id)`.
pub fn eval_ref_script(
    host: &Host,
    code: &str,
    env: Env,
    row: &Record<CborValue>,
) -> Result<Vista> {
    const WHAT: &str = "rhai reference build-script";
    let script = compile_handle_script(host, WHAT, code)?;
    let result = script
        .eval(&env.var("row", record_to_dynamic(row)))
        .context(error!("rhai script failed", script = WHAT))?;
    finish(WHAT, result)
}

/// Evaluate a `modify:` script against a built Vista, exposed as `self`.
///
/// Each verb returns a new handle and leaves `self` as it was, so the script
/// chains its changes and ends on the handle it built; `self` alone keeps the
/// Vista as given:
///
/// ```rhai
/// self.with_condition(surreal::ident("is_paying_client") == true).sort("name", "desc")
/// ```
///
/// The script's value is its last statement, and both of these are
/// incorrect-usage errors:
///
/// - an earlier statement that builds a handle and drops it, such as
///   `self.sort("name");` followed by more statements, refused when the
///   script compiles: chain the calls (`self.a().b()`) or assign the handle;
/// - a last value that is not a table handle, such as a `let`, or an `if`
///   with no `else` whose condition is false (end it with `else { self }`).
pub fn eval_modify_script(host: &Host, code: &str, vista: Vista) -> Result<Vista> {
    let script = compile_handle_script(host, MODIFY, code)?;
    let result = script
        .eval(&Env::new().var("self", Dynamic::from(Handle::over(vista))))
        .context(error!("rhai script failed", script = MODIFY))?;
    finish(MODIFY, result)
}

/// The handle the script ended on, resolved.
fn finish(what: &'static str, result: Dynamic) -> Result<Vista> {
    result
        .try_cast::<Handle>()
        .ok_or_else(|| {
            error!("The script must end on a table handle", script = what).mark_incorrect_usage()
        })?
        .resolve(None)
}
