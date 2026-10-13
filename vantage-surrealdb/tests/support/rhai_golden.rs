//! The `tests/rhai-tests/` golden fixtures: each `<name>.rhai` script is
//! pinned to the SurrealQL it renders (`<name>.surql`, compared trimmed) or
//! to the error it raises (`<name>.err`, matched as a substring). Shared by
//! the `rhai_golden` suite, which fails on a stale golden, and the
//! `rhai_test_surreal` example, which can also write them.

// `register_surreal_engine!` also brings type aliases this module doesn't
// use, and the example and the suite each use part of the module.
#![allow(dead_code, unused_imports)]

use std::fs;
use std::path::{Path, PathBuf};

use vantage_rhai::{Block, Env};
use vantage_surrealdb::rhai_engine::RhaiSelect;

vantage_surrealdb::register_surreal_engine!();

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

/// Evaluate a fixture: the SurrealQL its select renders, or its error.
pub fn eval(code: &str) -> Result<String, String> {
    __host()
        .compile(&Block::from(code))
        .and_then(|script| script.eval_as::<RhaiSelect>(&Env::new()))
        .map(|select| select.preview())
        .map_err(|e| e.to_string())
}

/// Compare one fixture's outcome with its golden. `None` means the fixture
/// has no golden; `Some(Err)` describes the mismatch.
pub fn check(name: &str, outcome: &Result<String, String>) -> Option<Result<(), String>> {
    let surql = dir().join(format!("{name}.surql"));
    let err = dir().join(format!("{name}.err"));
    let result = if surql.exists() {
        let expected = fs::read_to_string(&surql).unwrap();
        match outcome {
            Ok(actual) if actual.trim() == expected.trim() => Ok(()),
            Ok(actual) => Err(format!(
                "SurrealQL mismatch\nexpected:\n{}\nactual:\n{}",
                expected.trim(),
                actual.trim()
            )),
            Err(e) => Err(format!("unexpected error: {e}")),
        }
    } else if err.exists() {
        let expected = fs::read_to_string(&err).unwrap();
        match outcome {
            Err(e) if e.contains(expected.trim()) => Ok(()),
            Err(e) => Err(format!(
                "error mismatch\nexpected to contain: {}\nactual: {e}",
                expected.trim()
            )),
            Ok(surql) => Err(format!("expected an error, got SurrealQL: {surql}")),
        }
    } else {
        return None;
    };
    Some(result)
}

/// Run every fixture and return one line per mismatch, plus one per fixture
/// without a golden.
pub fn mismatches() -> Vec<String> {
    fixtures()
        .into_iter()
        .filter_map(|(name, code)| match check(&name, &eval(&code)) {
            None => Some(format!("{name}: no .surql or .err golden")),
            Some(result) => result.err().map(|e| format!("{name}: {e}")),
        })
        .collect()
}
