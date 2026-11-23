//! Evaluate a Rhai script into a `SqliteSelect` for use as a vista source.
//!
//! The engine is the same one the standalone Rhai tests use, registered here
//! for the SQLite dialect. When `base` is supplied it is seeded into scope as
//! `base` (a `RhaiSelect` wrapping the source table's select) so scripts can
//! *transform* an existing query rather than build one from scratch.
//!
//! `register_engine!` expands to a full engine toolkit (conversion helpers,
//! type aliases); a vista source only needs `__host` + `Sel`, so the unused
//! generated items are allowed here.
#![allow(dead_code)]

// NOTE: do not `use vantage_core::Result` here — `register_engine!` expands to
// std `Result<_, _>` (two type args) unqualified, and importing vantage-core's
// one-arg `Result` alias into this scope would shadow it and break the macro.
use crate::condition::SqliteCondition;
use crate::sqlite::AnySqliteType;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::statements::select::join::SqliteSelectJoin;

// `register_engine!` imports `rhai` into this scope.
crate::register_engine!(
    value: AnySqliteType,
    select: SqliteSelect,
    join: SqliteSelectJoin,
    cond: SqliteCondition,
);

/// Run `code`, returning the `SqliteSelect` it builds. If `base` is given it is
/// available to the script as the `base` variable.
/// Public one-shot evaluation: run a query-builder script and return the
/// `SqliteSelect` it built. Used by hosts (vantage-ui) that need a standalone
/// query outside any vista — e.g. a dashboard dropdown's `query:` options
/// block. Embedded scalar values become bound parameters as always.
pub fn eval_select(code: &str) -> vantage_core::Result<SqliteSelect> {
    eval_to_select(code, None)
}

/// [`eval_to_select`] with an observation-supplied `args` map in scope.
/// Values are plain strings ("" = not set by convention); anything the
/// script embeds in the query binds as a parameter like every other scalar.
pub(crate) fn eval_to_select_args(
    code: &str,
    base: Option<SqliteSelect>,
    args: &[(String, String)],
) -> vantage_core::Result<SqliteSelect> {
    let mut map = rhai::Map::new();
    for (k, v) in args {
        map.insert(k.as_str().into(), v.clone().into());
    }
    let mut env = vantage_rhai::Env::new().var("args", rhai::Dynamic::from_map(map));
    if let Some(base) = base {
        env = env.var("base", rhai::Dynamic::from(Sel::new(base)));
    }
    // Statements, not a lone expression: a `let` in an existing `rhai:` block
    // keeps working, and the final expression is the select.
    __host()
        .compile(&vantage_rhai::Block::from(code))
        .and_then(|script| script.eval_as::<Sel>(&env))
        .map(|select| select.into_inner())
        .map_err(|e| {
            vantage_core::error!(
                "Rhai vista source failed to evaluate",
                detail = e.to_string()
            )
        })
}

/// Register the SQLite query vocabulary onto an existing engine: the free
/// functions under `sql::` and the methods and operators on the SQL types.
pub fn register_sqlite_onto(engine: &mut rhai::Engine) {
    __register_engine_onto(engine);
}

pub(crate) fn eval_to_select(
    code: &str,
    base: Option<SqliteSelect>,
) -> vantage_core::Result<SqliteSelect> {
    eval_to_select_args(code, base, &[])
}

/// The host `expr:` scripts run on: the SQL vocabulary plus `table.ref(…)`
/// relation expressions over this dialect's tables.
fn __expr_host() -> &'static vantage_rhai::Host {
    static HOST: std::sync::LazyLock<vantage_rhai::Host> = std::sync::LazyLock::new(|| {
        vantage_rhai::Host::builder(vantage_rhai::Limits::background())
            .vocab(SqlVocab)
            .vocab_fn(crate::rhai_engine::relation::register_relations::<crate::sqlite::SqliteDB>)
            .build()
    });
    &HOST
}

/// Evaluate a column's `expr:` script into the expression the column
/// projects. With `table`, the script has it as `table`, so it can build a
/// relation expression; `env` carries the caller's other variables
/// (`session`, `app`).
pub fn eval_column_expr(
    code: &str,
    table: Option<vantage_table::table::Table<crate::sqlite::SqliteDB, vantage_types::EmptyEntity>>,
    env: vantage_rhai::Env,
) -> vantage_core::Result<vantage_expressions::Expression<AnySqliteType>> {
    let env = match table {
        Some(table) => env.var(
            "table",
            rhai::Dynamic::from(crate::rhai_engine::relation::RhaiTable::new(table)),
        ),
        None => env,
    };
    __expr_host()
        .compile(&vantage_rhai::Block::from(code))
        .and_then(|script| script.eval_as::<Ex>(&env))
        .map(|expr| expr.into_inner())
        .map_err(|e| vantage_core::error!(format!("expr: {e}"), detail = e.to_string()))
}
