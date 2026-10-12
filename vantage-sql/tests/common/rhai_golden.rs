//! The `tests/rhai-tests/` golden fixtures: each `<name>.rhai` script is
//! pinned to the SQL it renders (`<vendor>/<name>.sql` or `common/<name>.sql`)
//! or to the error it raises (`common/<name>.err` or `<vendor>/<name>.err`,
//! matched as a substring). Shared by the `rhai_tests` suite, which fails on a
//! stale golden, and the `rhai_test_sql` example, which can also write them.

// The example and the suite each use part of the module.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// What a fixture is pinned to.
pub enum Golden {
    Sql(PathBuf),
    Err(PathBuf),
}

pub fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/rhai-tests")
}

/// Every `.rhai` fixture as `(name, code)`, sorted by name.
pub fn fixtures() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = fs::read_dir(dir())
        .expect("tests/rhai-tests is readable")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|x| x == "rhai"))
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let code = fs::read_to_string(&path).expect("fixture is readable");
            (name, code)
        })
        .collect();
    found.sort();
    found
}

/// The golden for `name` on `vendor`; a common file wins over a vendor one.
pub fn golden(vendor: &str, name: &str) -> Option<Golden> {
    let base = dir();
    let path = |folder: &str, ext: &str| base.join(format!("{folder}/{name}.{ext}"));
    [
        Golden::Sql(path("common", "sql")),
        Golden::Sql(path(vendor, "sql")),
        Golden::Err(path("common", "err")),
        Golden::Err(path(vendor, "err")),
    ]
    .into_iter()
    .find(|g| match g {
        Golden::Sql(p) | Golden::Err(p) => p.exists(),
    })
}

pub fn format_sql(sql: &str) -> String {
    let options = sqlformat::FormatOptions {
        indent: sqlformat::Indent::Spaces(2),
        uppercase: Some(true),
        lines_between_queries: 1,
        ..Default::default()
    };
    sqlformat::format(sql, &sqlformat::QueryParams::None, &options)
        .trim()
        .to_string()
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Compare one script's outcome (rendered SQL or error text) with its
/// golden; `Err` describes the mismatch.
pub fn check(golden: &Golden, outcome: &Result<String, String>) -> Result<(), String> {
    match (golden, outcome) {
        (Golden::Sql(path), Ok(sql)) => {
            let expected = format_sql(&fs::read_to_string(path).unwrap());
            let actual = format_sql(sql);
            if expected == actual {
                Ok(())
            } else {
                Err(format!(
                    "SQL mismatch\nexpected:\n{expected}\nactual:\n{actual}"
                ))
            }
        }
        (Golden::Sql(_), Err(e)) => Err(format!("unexpected error: {e}")),
        (Golden::Err(path), Err(e)) => {
            let expected = fs::read_to_string(path).unwrap();
            if normalized(e).contains(&normalized(&expected)) {
                Ok(())
            } else {
                Err(format!(
                    "error mismatch\nexpected to contain: {}\nactual: {e}",
                    expected.trim()
                ))
            }
        }
        (Golden::Err(_), Ok(sql)) => Err(format!("expected an error, got SQL: {sql}")),
    }
}

/// Run every fixture on `vendor` and return one line per mismatch, plus one
/// per fixture without a golden.
pub fn mismatches(vendor: &str, eval: impl Fn(&str) -> Result<String, String>) -> Vec<String> {
    fixtures()
        .into_iter()
        .filter_map(|(name, code)| {
            let Some(golden) = golden(vendor, &name) else {
                return Some(format!("{name} [{vendor}]: no .sql or .err golden"));
            };
            check(&golden, &eval(&code))
                .err()
                .map(|e| format!("{name} [{vendor}]: {e}"))
        })
        .collect()
}
