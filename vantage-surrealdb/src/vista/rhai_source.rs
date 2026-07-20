//! Evaluate a Rhai script into a `SurrealSelect` for use as a vista source.
//!
//! Uses the same engine the standalone Rhai tests use (`register_surreal_engine!`).
//! When `base` is supplied it is seeded into scope as `base` (a `RhaiSelect`
//! wrapping the source table's select) so scripts can *transform* an existing
//! query rather than build one from scratch.
//!
//! `register_surreal_engine!` expands to a full engine toolkit; a vista source
//! only needs `__create_engine` + `eval`, so the unused generated items are
//! allowed here.
// `register_surreal_engine!` brings several type aliases and helper closures
// into scope; a vista source only needs `Sel` + `__create_engine`, so the
// unused remainder of the generated toolkit is allowed here.
#![allow(dead_code, unused_imports, unused_variables)]

use crate::statements::SurrealSelect;

crate::register_surreal_engine!();

/// Run `code`, returning the `SurrealSelect` it builds. If `base` is given it is
/// available to the script as the `base` variable.
pub(crate) fn eval_to_select(
    code: &str,
    base: Option<SurrealSelect>,
) -> vantage_core::Result<SurrealSelect> {
    let engine = __create_engine();
    let evaluated = match base {
        Some(base) => {
            let mut scope = rhai::Scope::new();
            scope.push("base", Sel { inner: base });
            engine.eval_with_scope::<Sel>(&mut scope, code)
        }
        None => engine.eval::<Sel>(code),
    };
    evaluated.map(|select| select.into_inner()).map_err(|e| {
        vantage_core::error!(
            "Rhai vista source failed to evaluate",
            detail = e.to_string()
        )
    })
}

/// Run `code`, returning the expression it builds — the vocabulary is the
/// same as query-sourced scripts (`ident(...)`, operators, `expr("raw")`).
/// Accepts a bare identifier result (`ident("batch")["name"]` yields an
/// expression, `ident("x")` alone an identifier) so both shapes work.
pub(crate) fn eval_to_expr(code: &str) -> vantage_core::Result<crate::Expr> {
    use vantage_expressions::Expressive as _;
    let engine = __create_engine();
    let value = engine.eval::<rhai::Dynamic>(code).map_err(|e| {
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
