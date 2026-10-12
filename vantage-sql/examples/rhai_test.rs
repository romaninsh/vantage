//! Rhai DSL golden runner.
//!
//! Evaluates each `tests/rhai-tests/*.rhai` file against all available SQL
//! backends and prints how each compares to its pinned `.sql` or `.err`
//! golden. The `rhai_tests` suite runs the same check under `cargo test`;
//! this runner is for reading the diffs and, with `--fix`, writing the
//! vendor goldens a fixture is missing.
//!
//! Run: cargo run --example rhai_test_sql --features "sqlite,postgres,mysql,rhai"

use std::fs;
use std::process;
use vantage_rhai::{Block, Env};

#[path = "../tests/common/rhai_golden.rs"]
mod golden;

// ── Vendor engines ─────────────────────────────────────────────────────

#[cfg(feature = "sqlite")]
mod sqlite_engine {
    use vantage_sql::condition::SqliteCondition;
    use vantage_sql::rhai_engine::RhaiSelect;
    use vantage_sql::sqlite::AnySqliteType;
    use vantage_sql::sqlite::statements::SqliteSelect;
    use vantage_sql::sqlite::statements::select::join::SqliteSelectJoin;

    vantage_sql::register_engine!(
        value: AnySqliteType,
        select: SqliteSelect,
        join: SqliteSelectJoin,
        cond: SqliteCondition,
    );

    pub type Select = RhaiSelect<AnySqliteType, SqliteSelect, SqliteSelectJoin, SqliteCondition>;

    pub fn host() -> &'static vantage_rhai::Host {
        __host()
    }
}

#[cfg(feature = "postgres")]
mod postgres_engine {
    use vantage_sql::condition::PostgresCondition;
    use vantage_sql::postgres::AnyPostgresType;
    use vantage_sql::postgres::statements::PostgresSelect;
    use vantage_sql::postgres::statements::select::join::PostgresSelectJoin;
    use vantage_sql::rhai_engine::RhaiSelect;

    vantage_sql::register_engine!(
        value: AnyPostgresType,
        select: PostgresSelect,
        join: PostgresSelectJoin,
        cond: PostgresCondition,
    );

    pub type Select =
        RhaiSelect<AnyPostgresType, PostgresSelect, PostgresSelectJoin, PostgresCondition>;

    pub fn host() -> &'static vantage_rhai::Host {
        __host()
    }
}

#[cfg(feature = "mysql")]
mod mysql_engine {
    use vantage_sql::condition::MysqlCondition;
    use vantage_sql::mysql::AnyMysqlType;
    use vantage_sql::mysql::statements::MysqlSelect;
    use vantage_sql::mysql::statements::select::join::MysqlSelectJoin;
    use vantage_sql::rhai_engine::RhaiSelect;

    vantage_sql::register_engine!(
        value: AnyMysqlType,
        select: MysqlSelect,
        join: MysqlSelectJoin,
        cond: MysqlCondition,
    );

    pub type Select = RhaiSelect<AnyMysqlType, MysqlSelect, MysqlSelectJoin, MysqlCondition>;

    pub fn host() -> &'static vantage_rhai::Host {
        __host()
    }
}

// ── Runner ─────────────────────────────────────────────────────────────

type Eval = fn(&str) -> Result<String, String>;

fn vendors() -> Vec<(&'static str, Eval)> {
    #[allow(unused_mut)]
    let mut vendors: Vec<(&'static str, Eval)> = Vec::new();
    #[cfg(feature = "sqlite")]
    vendors.push(("sqlite", |code| {
        sqlite_engine::host()
            .compile(&Block::from(code))
            .and_then(|s| s.eval_as::<sqlite_engine::Select>(&Env::new()))
            .map(|s| s.inner.preview())
            .map_err(|e| e.to_string())
    }));
    #[cfg(feature = "postgres")]
    vendors.push(("postgres", |code| {
        postgres_engine::host()
            .compile(&Block::from(code))
            .and_then(|s| s.eval_as::<postgres_engine::Select>(&Env::new()))
            .map(|s| s.inner.preview())
            .map_err(|e| e.to_string())
    }));
    #[cfg(feature = "mysql")]
    vendors.push(("mysql", |code| {
        mysql_engine::host()
            .compile(&Block::from(code))
            .and_then(|s| s.eval_as::<mysql_engine::Select>(&Env::new()))
            .map(|s| s.inner.preview())
            .map_err(|e| e.to_string())
    }));
    vendors
}

fn main() {
    let fix = std::env::args().any(|a| a == "--fix");
    let (mut pass, mut fail) = (0u32, 0u32);

    for (name, code) in golden::fixtures() {
        for (vendor, eval) in vendors() {
            let outcome = eval(&code);
            let ok = match golden::golden(vendor, &name) {
                Some(golden) => report(&name, vendor, golden::check(&golden, &outcome)),
                None if fix => write_golden(&name, vendor, &outcome),
                None => report(&name, vendor, Err("no .sql or .err golden".into())),
            };
            if ok {
                pass += 1;
            } else {
                fail += 1;
            }
        }
    }

    println!("\n  {pass} passed, {fail} failed");
    if fail > 0 {
        process::exit(1);
    }
}

fn report(name: &str, vendor: &str, result: Result<(), String>) -> bool {
    match &result {
        Ok(()) => println!("  ✅ {name} [{vendor}]"),
        Err(e) => println!("  ❌ {name} [{vendor}] {e}"),
    }
    result.is_ok()
}

/// Pin a fixture that has no golden to what it produces now: its formatted
/// SQL, or the first line of its error.
fn write_golden(name: &str, vendor: &str, outcome: &Result<String, String>) -> bool {
    let base = golden::dir().join(vendor);
    let (path, text) = match outcome {
        Ok(sql) => (base.join(format!("{name}.sql")), golden::format_sql(sql)),
        Err(e) => (
            base.join(format!("{name}.err")),
            e.lines().next().unwrap_or("").to_string(),
        ),
    };
    fs::write(&path, text).unwrap();
    println!("  ✅ {name} [{vendor}] created {}", path.display());
    true
}
