//! Rhai DSL test runner for SurrealDB.
//!
//! Walks `tests/rhai-tests/`, evaluates each `.rhai` file, and compares
//! output to pinned `.surql` files (or `.err` files for error cases).
//!
//! Run: cargo run --example rhai_test --features rhai
//! Fix: cargo run --example rhai_test --features rhai -- --fix

use std::fs;
use std::path::Path;
use std::process;

use vantage_surrealdb::rhai_engine::RhaiSelect;

vantage_surrealdb::register_surreal_engine!();

const TEST_DIR: &str = "tests/rhai-tests";

fn test_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(TEST_DIR)
}

fn main() {
    let fix = std::env::args().any(|a| a == "--fix");

    let test_dir_path = test_dir();
    if !test_dir_path.exists() {
        eprintln!("Test directory not found: {}", test_dir_path.display());
        process::exit(1);
    }

    let mut entries: Vec<_> = fs::read_dir(&test_dir_path)
        .expect("Failed to read test dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "rhai").unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let engine = __create_engine();

    let mut pass = 0u32;
    let mut fail = 0u32;

    for entry in &entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_str().unwrap();
        let code = fs::read_to_string(&path).expect("Failed to read .rhai");

        let surql_path = test_dir_path.join(format!("{}.surql", name));
        let err_path = test_dir_path.join(format!("{}.err", name));

        let has_surql = surql_path.exists();
        let has_err = err_path.exists();

        let result = engine
            .eval::<RhaiSelect>(&code)
            .map(|s| s.preview());

        // --fix mode: generate missing files
        if fix {
            match result {
                Ok(surql) => {
                    if !has_surql {
                        fs::write(&surql_path, surql.trim()).unwrap();
                        println!("  ✅ {} created .surql", name);
                        pass += 1;
                    } else {
                        println!("  ⏭️  {} .surql already exists", name);
                    }
                    if has_err {
                        fs::remove_file(&err_path).unwrap();
                    }
                }
                Err(e) => {
                    if !has_err {
                        let err_msg = e.to_string();
                        let first_line = err_msg.lines().next().unwrap_or("");
                        fs::write(&err_path, first_line).unwrap();
                        println!("  ✅ {} created .err", name);
                        pass += 1;
                    } else {
                        println!("  ⏭️  {} .err already exists", name);
                    }
                    if has_surql {
                        fs::remove_file(&surql_path).unwrap();
                    }
                }
            }
            continue;
        }

        // Normal test mode
        if has_surql {
            let expected = fs::read_to_string(&surql_path).unwrap();
            match result {
                Ok(actual) => {
                    if actual.trim() == expected.trim() {
                        println!("  ✅ {}", name);
                        pass += 1;
                    } else {
                        println!("  ❌ {} SurrealQL mismatch", name);
                        println!("     expected:\n{}", expected.trim());
                        println!("     actual:\n{}", actual.trim());
                        fail += 1;
                    }
                }
                Err(e) => {
                    println!("  ❌ {} unexpected error: {}", name, e);
                    fail += 1;
                }
            }
        } else if has_err {
            let expected = fs::read_to_string(&err_path).unwrap();
            match result {
                Err(e) => {
                    let actual = e.to_string();
                    if actual.contains(expected.trim()) {
                        println!("  ✅ {} (error)", name);
                        pass += 1;
                    } else {
                        println!("  ❌ {} error mismatch", name);
                        println!("     expected to contain: {}", expected.trim());
                        println!("     actual: {}", actual.lines().next().unwrap_or(""));
                        fail += 1;
                    }
                }
                Ok(surql) => {
                    println!(
                        "  ❌ {} expected error, got SurrealQL: {}",
                        name,
                        surql.trim()
                    );
                    fail += 1;
                }
            }
        }
        // No .surql or .err → skip silently
    }

    println!("\n  {} passed, {} failed", pass, fail);
    if fail > 0 {
        process::exit(1);
    }
}
