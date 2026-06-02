//! Evaluate a Rhai script into a `SqliteSelect` for use as a vista source.
//!
//! The engine is the same one the standalone Rhai tests use, registered here
//! for the SQLite dialect. When `base` is supplied it is seeded into scope as
//! `base` (a `RhaiSelect` wrapping the source table's select) so scripts can
//! *transform* an existing query rather than build one from scratch.
//!
//! `register_engine!` expands to a full engine toolkit (conversion helpers,
//! type aliases); a vista source only needs `__create_engine` + `eval`, so the
//! unused generated items are allowed here.
#![allow(dead_code)]

// NOTE: do not `use vantage_core::Result` here — `register_engine!` expands to
// std `Result<_, _>` (two type args) unqualified, and importing vantage-core's
// one-arg `Result` alias into this scope would shadow it and break the macro.
use crate::condition::SqliteCondition;
use crate::sqlite::AnySqliteType;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::statements::select::join::SqliteSelectJoin;

crate::register_engine!(
    value: AnySqliteType,
    select: SqliteSelect,
    join: SqliteSelectJoin,
    cond: SqliteCondition,
);

/// Run `code`, returning the `SqliteSelect` it builds. If `base` is given it is
/// available to the script as the `base` variable.
pub(crate) fn eval_to_select(
    code: &str,
    base: Option<SqliteSelect>,
) -> vantage_core::Result<SqliteSelect> {
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
