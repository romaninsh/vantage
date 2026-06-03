//! Evaluate a Rhai script into a `MysqlSelect` for use as a vista source.
//!
//! The engine is the same one the standalone Rhai tests use, registered here
//! for the MySQL dialect. When `base` is supplied it is seeded into scope as
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
use crate::condition::MysqlCondition;
use crate::mysql::AnyMysqlType;
use crate::mysql::statements::MysqlSelect;
use crate::mysql::statements::select::join::MysqlSelectJoin;

crate::register_engine!(
    value: AnyMysqlType,
    select: MysqlSelect,
    join: MysqlSelectJoin,
    cond: MysqlCondition,
);

/// Run `code`, returning the `MysqlSelect` it builds. If `base` is given it is
/// available to the script as the `base` variable.
pub(crate) fn eval_to_select(
    code: &str,
    base: Option<MysqlSelect>,
) -> vantage_core::Result<MysqlSelect> {
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
