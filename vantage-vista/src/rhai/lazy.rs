//! `lazy:` column expressions: a script computing one value from a record.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_rhai::{Block, Compiled, Env, Host};
use vantage_types::Record;

use super::convert::{dynamic_to_cbor, record_to_dynamic};
use super::eval::compile;

/// A lazy-expression value closure: given the record as built so far, compute
/// one column value. This is the CBOR-carrier form of
/// `vantage_table::Table::with_lazy_expression`'s callback; driver factories
/// adapt it to their native value type when lowering a spec's `lazy:` script.
pub type LazyValueFn = Arc<dyn Fn(&Record<CborValue>) -> Result<CborValue> + Send + Sync>;

/// Evaluate a lazy-expression script for one record. The record as built so
/// far is the `row` map; the script's final expression is the column's value:
///
/// ```rhai
/// row.contents.split("\n").len() - 1
/// ```
pub fn eval_lazy_expression(host: &Host, code: &str, row: &Record<CborValue>) -> Result<CborValue> {
    let script = compile(host, "rhai lazy expression", code)?;
    eval_lazy_compiled(&script, row)
}

fn eval_lazy_compiled(script: &Compiled<Block>, row: &Record<CborValue>) -> Result<CborValue> {
    let result = script
        .eval(&Env::new().var("row", record_to_dynamic(row)))
        .map_err(|e| error!(format!("rhai lazy expression failed: {e}")))?;
    dynamic_to_cbor(result).map_err(|e| error!(format!("rhai lazy expression result: {e}")))
}

/// Build a reusable [`LazyValueFn`] from a script. Compiles once, here, so a
/// script that does not parse fails the table build, not the first row.
pub fn lazy_value_closure(code: &str) -> Result<LazyValueFn> {
    let script = compile(
        vantage_rhai::background_host(),
        "rhai lazy expression",
        code,
    )?;
    Ok(Arc::new(move |row: &Record<CborValue>| {
        eval_lazy_compiled(&script, row)
    }))
}
