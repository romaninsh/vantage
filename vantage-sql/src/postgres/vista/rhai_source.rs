//! Evaluate a Rhai script into a `PostgresSelect` for use as a vista source.
//!
//! The engine is the same one the standalone Rhai tests use, registered here
//! for the PostgreSQL dialect. When `base` is supplied it is seeded into scope
//! as `base` (a `RhaiSelect` wrapping the source table's select) so scripts can
//! *transform* an existing query rather than build one from scratch.
//!
//! `register_engine!` expands to a full engine toolkit (conversion helpers,
//! type aliases); a vista source only needs `__create_engine` + `eval`, so the
//! unused generated items are allowed here.
#![allow(dead_code)]

// NOTE: do not `use vantage_core::Result` here — `register_engine!` expands to
// std `Result<_, _>` (two type args) unqualified, and importing vantage-core's
// one-arg `Result` alias into this scope would shadow it and break the macro.
use crate::condition::PostgresCondition;
use crate::postgres::AnyPostgresType;
use crate::postgres::statements::PostgresSelect;
use crate::postgres::statements::select::join::PostgresSelectJoin;

crate::register_engine!(
    value: AnyPostgresType,
    select: PostgresSelect,
    join: PostgresSelectJoin,
    cond: PostgresCondition,
);

/// Run `code`, returning the `PostgresSelect` it builds. If `base` is given it
/// is available to the script as the `base` variable.
/// Public one-shot evaluation: run a query-builder script and return the
/// `PostgresSelect` it built. Used by hosts (vantage-ui) that need a standalone
/// query outside any vista — e.g. a dashboard dropdown's `query:` options
/// block. Embedded scalar values become bound parameters as always.
pub fn eval_select(code: &str) -> vantage_core::Result<PostgresSelect> {
    eval_to_select(code, None)
}

/// [`eval_to_select`] with an observation-supplied `args` map in scope.
/// Values are plain strings ("" = not set by convention); anything the
/// script embeds in the query binds as a parameter like every other scalar.
pub(crate) fn eval_to_select_args(
    code: &str,
    base: Option<PostgresSelect>,
    args: &[(String, String)],
) -> vantage_core::Result<PostgresSelect> {
    let engine = __create_engine();
    let mut scope = rhai::Scope::new();
    let mut map = rhai::Map::new();
    for (k, v) in args {
        map.insert(k.as_str().into(), v.clone().into());
    }
    scope.push_constant("args", map);
    if let Some(base) = base {
        scope.push("base", Sel::new(base));
    }
    engine
        .eval_with_scope::<Sel>(&mut scope, code)
        .map(|select| select.into_inner())
        .map_err(|e| {
            vantage_core::error!(
                "Rhai vista source failed to evaluate",
                detail = e.to_string()
            )
        })
}

pub(crate) fn eval_to_select(
    code: &str,
    base: Option<PostgresSelect>,
) -> vantage_core::Result<PostgresSelect> {
    let engine = __create_engine();
    let evaluated = match base {
        Some(base) => {
            let mut scope = rhai::Scope::new();
            scope.push("base", Sel::new(base));
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
