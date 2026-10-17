//! Evaluate a vista's `rhai:` source, `base:` transform or `expr:` column
//! script on the shared background host (`register_surreal_engine!`). Scripts
//! compile as statement blocks, so `let` works; the last expression is the
//! value.
// `register_surreal_engine!` brings several type aliases into scope; a vista
// source only needs `Sel`/`Ex`/`Id` + `__host`, so the unused remainder of the
// generated toolkit is allowed here.
#![allow(dead_code, unused_imports, unused_variables)]

use vantage_rhai::rhai;
use vantage_rhai::{Block, Env};

use crate::statements::SurrealSelect;

crate::register_surreal_engine!();

/// Run `code`, returning the `SurrealSelect` it builds. If `base` is given it is
/// available to the script as the `base` variable.
pub(crate) fn eval_to_select(
    code: &str,
    base: Option<SurrealSelect>,
) -> vantage_core::Result<SurrealSelect> {
    eval_to_select_args(code, base, &[])
}

/// [`eval_to_select`] with the host's `args` map in scope. Values are strings
/// (`""` means not set); one the script puts in the query binds as a
/// parameter like any other scalar.
pub(crate) fn eval_to_select_args(
    code: &str,
    base: Option<SurrealSelect>,
    args: &[(String, String)],
) -> vantage_core::Result<SurrealSelect> {
    let mut map = rhai::Map::new();
    for (k, v) in args {
        map.insert(k.as_str().into(), v.clone().into());
    }
    let mut env = Env::new().var("args", rhai::Dynamic::from_map(map));
    if let Some(base) = base {
        env = env.var("base", rhai::Dynamic::from(Sel { inner: base }));
    }
    __host()
        .compile(&Block::from(code))
        .and_then(|script| script.eval(&env))
        .map_err(|e| {
            vantage_core::error!(
                "Rhai vista source failed to evaluate",
                detail = e.to_string()
            )
        })?
        .try_cast::<Sel>()
        .map(|select| select.into_inner())
        .ok_or_else(|| vantage_core::error!("Rhai vista source must evaluate to a select"))
}

/// Run `code`, returning the expression it builds. A bare identifier
/// (`surreal::ident("x")`) is accepted as well as an expression.
pub(crate) fn eval_to_expr(code: &str) -> vantage_core::Result<crate::Expr> {
    use vantage_expressions::Expressive as _;
    let value = __host()
        .compile(&Block::from(code))
        .and_then(|script| script.eval(&Env::new()))
        .map_err(|e| {
            vantage_core::error!(
                "Rhai column expression failed to evaluate",
                detail = e.to_string()
            )
        })?;
    if value.is::<Ex>() {
        return Ok(value.cast::<Ex>().into_inner());
    }
    if value.is::<Id>() {
        return Ok(value.cast::<Id>().into_inner().expr());
    }
    Err(vantage_core::error!(
        "Rhai column expression must evaluate to an expression or identifier",
        got = value.type_name()
    ))
}
