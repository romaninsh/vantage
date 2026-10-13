//! `lazy:` column expressions: a script computing one value from a record.
//! [`Column::with_expression`](crate::Column::with_expression) is the public
//! way in.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_rhai::Env;
use vantage_types::Record;

use super::convert::{dynamic_to_cbor, record_to_dynamic};
use super::eval::compile;
use crate::column::LazyValueFn;

/// Compile `code` once into a [`LazyValueFn`], so a script that does not
/// parse fails the build, not the first row. The record as built so far is
/// the `row` map; the script's final expression is the column's value:
///
/// ```rhai
/// row.contents.split("\n").len() - 1
/// ```
pub(crate) fn lazy_value_closure(code: &str) -> Result<LazyValueFn> {
    let script = compile(
        vantage_rhai::background_host(),
        "rhai lazy expression",
        code,
    )?;
    Ok(Arc::new(move |row: &Record<CborValue>| {
        let result = script
            .eval(&Env::new().var("row", record_to_dynamic(row)))
            .map_err(|e| error!("Rhai lazy expression failed", detail = e.to_string()))?;
        dynamic_to_cbor(result).map_err(|e| {
            error!(
                "Rhai lazy expression returned an unusable value",
                detail = e.to_string()
            )
        })
    }))
}
