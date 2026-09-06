//! Evaluate a Rhai script into a `SurrealSelect` for use as a vista source.
//!
//! Uses the same host the standalone Rhai tests use (`register_surreal_engine!`):
//! built once, background limits, the full vendor vocabulary. When `base` is
//! supplied it is pushed as the `base` variable (a `RhaiSelect` wrapping the
//! source table's select) so scripts can *transform* an existing query rather
//! than build one from scratch. `me` is always in scope.
//!
//! Scripts compile as statements (`Block`), the same shape `Engine::eval`
//! accepted before, so a `let` in an existing `rhai:` block keeps working; the
//! final expression is the value.
// `register_surreal_engine!` brings several type aliases into scope; a vista
// source only needs `Sel`/`Ex`/`Id` + `__host`, so the unused remainder of the
// generated toolkit is allowed here.
#![allow(dead_code, unused_imports, unused_variables)]

use vantage_rhai::rhai;
use vantage_rhai::{Block, Env};

use crate::rhai_engine::surreal_env;
use crate::statements::SurrealSelect;

crate::register_surreal_engine!();

/// Run `code`, returning the `SurrealSelect` it builds. If `base` is given it is
/// available to the script as the `base` variable.
pub(crate) fn eval_to_select(
    code: &str,
    base: Option<SurrealSelect>,
) -> vantage_core::Result<SurrealSelect> {
    let mut env = surreal_env(Env::new());
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

/// Run `code`, returning the expression it builds — the vocabulary is the
/// same as query-sourced scripts (`ident(...)`, operators, `expr("raw")`).
/// Accepts a bare identifier result (`ident("batch")["name"]` yields an
/// expression, `ident("x")` alone an identifier) so both shapes work.
pub(crate) fn eval_to_expr(code: &str) -> vantage_core::Result<crate::Expr> {
    use vantage_expressions::Expressive as _;
    let value = __host()
        .compile(&Block::from(code))
        .and_then(|script| script.eval(&surreal_env(Env::new())))
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vantage_rhai::{Expr, Lookup, Resolver};

    use super::*;

    #[test]
    fn me_is_in_scope_as_a_variable() {
        assert!(eval_to_expr("me").is_ok());
        assert!(eval_to_expr("me[\"name\"]").is_ok());
        let err = eval_to_expr("nope").unwrap_err();
        assert!(err.to_string().contains("nope"), "{err}");
    }

    #[test]
    fn me_coexists_with_a_host_resolver() {
        // The reason `me` moved: the old hook replaced the host's resolver hook
        // (rhai keeps only the last `on_var`). Both must work in one script.
        struct Ns;
        impl Resolver for Ns {
            fn resolve(&self, path: &str) -> Lookup {
                match path {
                    "app" => Lookup::Namespace,
                    "app.n" => Lookup::Leaf(rhai::Dynamic::from(2_i64)),
                    _ => Lookup::Unknown,
                }
            }
        }
        let env = surreal_env(Env::new().resolver(Arc::new(Ns)));

        let read = __host()
            .compile(&Expr::from("app.n"))
            .unwrap()
            .discover(&env)
            .unwrap();
        assert_eq!(read.eval(&env).unwrap().as_int().unwrap(), 2);
        assert!(
            read.read_set().contains("app.n"),
            "resolver reads are recorded"
        );

        let me = __host()
            .compile(&Expr::from("me"))
            .unwrap()
            .eval(&env)
            .unwrap();
        assert!(
            me.is::<Ex>(),
            "`me` is the anchor expression, got {}",
            me.type_name()
        );
    }

    #[test]
    fn query_source_is_bounded() {
        let err = eval_to_select("loop {}", None).unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }
}
