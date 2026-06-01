//! Generic Rhai DSL test runner.
//!
//! Walks `tests/rhai-tests/`, evaluates each `.rhai` file against all
//! available SQL backends, and compares output to pinned `.sql` files
//! (or expected `.err` files for error cases).
//!
//! Run: cargo run --example rhai_test --features "sqlite,postgres,mysql,rhai"

use std::fs;
use std::path::Path;
use std::process;

fn format_sql(sql: &str) -> String {
    let options = sqlformat::FormatOptions {
        indent: sqlformat::Indent::Spaces(2),
        uppercase: Some(true),
        lines_between_queries: 1,
        ..Default::default()
    };
    sqlformat::format(sql, &sqlformat::QueryParams::None, &options)
}

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

    pub fn create() -> rhai::Engine {
        __create_engine()
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

    pub fn create() -> rhai::Engine {
        __create_engine()
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

    pub fn create() -> rhai::Engine {
        __create_engine()
    }
}

// ── Test runner ────────────────────────────────────────────────────────

const TEST_DIR: &str = "tests/rhai-tests";

fn test_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(TEST_DIR)
}

fn main() {
    let fix = std::env::args().any(|a| a == "--fix");

    let test_dir = test_dir();
    if !test_dir.exists() {
        eprintln!("Test directory not found: {}", test_dir.display());
        process::exit(1);
    }

    let mut entries: Vec<_> = fs::read_dir(test_dir)
        .expect("Failed to read test dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "rhai").unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut pass = 0u32;
    let mut fail = 0u32;

    for entry in &entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_str().unwrap();
        let code = fs::read_to_string(&path).expect("Failed to read .rhai");

        #[cfg(feature = "sqlite")]
        run_test(&mut pass, &mut fail, "sqlite", name, &code, fix, |e| {
            sqlite_engine::create()
                .eval::<sqlite_engine::Select>(e)
                .map(|s| s.inner.preview())
        });

        #[cfg(feature = "postgres")]
        run_test(&mut pass, &mut fail, "postgres", name, &code, fix, |e| {
            postgres_engine::create()
                .eval::<postgres_engine::Select>(e)
                .map(|s| s.inner.preview())
        });

        #[cfg(feature = "mysql")]
        run_test(&mut pass, &mut fail, "mysql", name, &code, fix, |e| {
            mysql_engine::create()
                .eval::<mysql_engine::Select>(e)
                .map(|s| s.inner.preview())
        });
    }

    println!("\n  {} passed, {} failed", pass, fail);
    if fail > 0 {
        process::exit(1);
    }
}

fn run_test<F>(
    pass: &mut u32,
    fail: &mut u32,
    vendor: &str,
    name: &str,
    code: &str,
    fix: bool,
    eval: F,
) where
    F: Fn(&str) -> Result<String, Box<rhai::EvalAltResult>>,
{
    let base = test_dir();
    let common_sql = base.join(format!("common/{}.sql", name));
    let common_err = base.join(format!("common/{}.err", name));
    let vendor_sql = base.join(format!("{}/{}.sql", vendor, name));
    let vendor_err = base.join(format!("{}/{}.err", vendor, name));

    let has_common_sql = common_sql.exists();
    let has_common_err = common_err.exists();
    let has_vendor_sql = vendor_sql.exists();
    let has_vendor_err = vendor_err.exists();

    let sql_path = if has_common_sql {
        Some(common_sql)
    } else if has_vendor_sql {
        Some(vendor_sql.clone())
    } else {
        None
    };

    let err_path = if has_common_err {
        Some(common_err)
    } else if has_vendor_err {
        Some(vendor_err.clone())
    } else {
        None
    };

    // --fix mode: generate missing files
    if fix {
        if has_common_sql || has_common_err {
            println!("  ⚠️  {} [{}] cannot fix: common file exists", name, vendor);
            *fail += 1;
            return;
        }

        match eval(code) {
            Ok(sql) => {
                if !has_vendor_sql {
                    let formatted = format_sql(&sql);
                    fs::write(&vendor_sql, formatted.trim()).unwrap();
                    println!("  ✅ {} [{}] created .sql", name, vendor);
                    *pass += 1;
                } else {
                    println!("  ⏭️  {} [{}] .sql already exists", name, vendor);
                }
                // Remove .err if it exists
                if has_vendor_err {
                    fs::remove_file(&vendor_err).unwrap();
                }
            }
            Err(e) => {
                if !has_vendor_err {
                    let err_msg = e.to_string();
                    let first_line = err_msg.lines().next().unwrap_or("");
                    fs::write(&vendor_err, first_line).unwrap();
                    println!("  ✅ {} [{}] created .err", name, vendor);
                    *pass += 1;
                } else {
                    println!("  ⏭️  {} [{}] .err already exists", name, vendor);
                }
                // Remove .sql if it exists
                if has_vendor_sql {
                    fs::remove_file(&vendor_sql).unwrap();
                }
            }
        }
        return;
    }

    // Normal test mode
    if let Some(sql_path) = sql_path {
        let expected = fs::read_to_string(&sql_path).unwrap();
        match eval(code) {
            Ok(sql) => {
                let actual_fmt = format_sql(&sql);
                let expected_fmt = format_sql(&expected);
                if actual_fmt.trim() == expected_fmt.trim() {
                    println!("  ✅ {} [{}]", name, vendor);
                    *pass += 1;
                } else {
                    println!("  ❌ {} [{}] SQL mismatch", name, vendor);
                    println!("     expected:\n{}", expected_fmt.trim());
                    println!("     actual:\n{}", actual_fmt.trim());
                    *fail += 1;
                }
            }
            Err(e) => {
                println!("  ❌ {} [{}] unexpected error: {}", name, vendor, e);
                *fail += 1;
            }
        }
    } else if let Some(err_path) = err_path {
        let expected = fs::read_to_string(&err_path).unwrap();
        match eval(code) {
            Err(e) => {
                let actual = e.to_string();
                if contains_normalized(&actual, &expected) {
                    println!("  ✅ {} [{}] (error)", name, vendor);
                    *pass += 1;
                } else {
                    println!("  ❌ {} [{}] error mismatch", name, vendor);
                    println!("     expected to contain: {}", expected.trim());
                    println!("     actual: {}", actual.lines().next().unwrap_or(""));
                    *fail += 1;
                }
            }
            Ok(sql) => {
                println!(
                    "  ❌ {} [{}] expected error, got SQL: {}",
                    name,
                    vendor,
                    sql.trim()
                );
                *fail += 1;
            }
        }
    }
    // No .sql or .err → skip silently
}

/// Check if `haystack` contains `needle` after normalizing whitespace.
fn contains_normalized(haystack: &str, needle: &str) -> bool {
    let h: String = haystack.split_whitespace().collect::<Vec<_>>().join(" ");
    let n: String = needle.split_whitespace().collect::<Vec<_>>().join(" ");
    h.contains(&n)
}
