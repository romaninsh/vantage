//! Rhai DSL golden runner for SurrealDB.
//!
//! Evaluates each `tests/rhai-tests/*.rhai` file and prints how it compares
//! to its pinned `.surql` or `.err` golden. The `rhai_golden` suite runs the
//! same check under `cargo test`; this runner is for reading the diffs and,
//! with `--fix`, writing the goldens a fixture is missing.
//!
//! Run: cargo run --example rhai_test_surreal --features rhai
//! Fix: cargo run --example rhai_test_surreal --features rhai -- --fix

use std::fs;
use std::process;

#[path = "../tests/support/rhai_golden.rs"]
mod golden;

fn main() {
    let fix = std::env::args().any(|a| a == "--fix");
    let (mut pass, mut fail) = (0u32, 0u32);

    for (name, code) in golden::fixtures() {
        let outcome = golden::eval(&code);
        let result = match golden::check(&name, &outcome) {
            Some(result) => result,
            None if fix => {
                write_golden(&name, &outcome);
                Ok(())
            }
            None => Err("no .surql or .err golden".to_string()),
        };
        match result {
            Ok(()) => {
                println!("  ✅ {name}");
                pass += 1;
            }
            Err(e) => {
                println!("  ❌ {name} {e}");
                fail += 1;
            }
        }
    }

    println!("\n  {pass} passed, {fail} failed");
    if fail > 0 {
        process::exit(1);
    }
}

/// Pin a fixture that has no golden to what it produces now: its SurrealQL,
/// or the first line of its error.
fn write_golden(name: &str, outcome: &Result<String, String>) {
    let (path, text) = match outcome {
        Ok(surql) => (
            golden::dir().join(format!("{name}.surql")),
            surql.trim().to_string(),
        ),
        Err(e) => (
            golden::dir().join(format!("{name}.err")),
            e.lines().next().unwrap_or("").to_string(),
        ),
    };
    fs::write(&path, text).unwrap();
    println!("  created {}", path.display());
}
