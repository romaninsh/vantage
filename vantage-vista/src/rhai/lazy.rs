//! `lazy:` column expressions: a script computing one value from a record.
//! [`Column::with_expression`](crate::Column::with_expression) is the public
//! way in.

use std::sync::{Arc, LazyLock};

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_rhai::rhai::{AST, Dynamic, Engine, Scope};
use vantage_rhai::{Env, Host, Limits, RhaiError};
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
///
/// `host` is the host to compile on; `None` is the fixed host with only the
/// date functions. A given host compiles the script self-contained, its
/// `import`s resolved now, so a missing module fails the build too.
pub(crate) fn lazy_value_closure(code: &str, host: Option<&Arc<Host>>) -> Result<LazyValueFn> {
    static HOST: LazyLock<Host> = LazyLock::new(|| {
        Host::builder(Limits::background())
            .vocab(vantage_rhai::Dates)
            .build()
    });
    let Some(host) = host else {
        let script = compile(&HOST, "rhai lazy expression", code)?;
        return Ok(Arc::new(move |row: &Record<CborValue>| {
            let result = script
                .eval(&Env::new().var("row", record_to_dynamic(row)))
                .map_err(|e| error!("Rhai lazy expression failed", detail = e.to_string()))?;
            to_value(result)
        }));
    };
    let engine = host.engine().clone();
    let ast = engine
        .compile_into_self_contained(&Scope::new(), code)
        .map_err(|e| {
            error!(
                "rhai script failed to compile",
                script = "rhai lazy expression",
                detail = RhaiError::from_eval(code, e).to_string()
            )
        })?;
    let src: Arc<str> = code.into();
    Ok(Arc::new(move |row: &Record<CborValue>| {
        let result = eval(&engine, &ast, row).map_err(|e| {
            error!(
                "Rhai lazy expression failed",
                detail = RhaiError::from_eval(&src, e).to_string()
            )
        })?;
        to_value(result)
    }))
}

fn eval(
    engine: &Engine,
    ast: &AST,
    row: &Record<CborValue>,
) -> std::result::Result<Dynamic, Box<vantage_rhai::rhai::EvalAltResult>> {
    let mut scope = Scope::new();
    scope.push_dynamic("row", record_to_dynamic(row));
    engine.eval_ast_with_scope::<Dynamic>(&mut scope, ast)
}

fn to_value(result: Dynamic) -> Result<CborValue> {
    dynamic_to_cbor(result).map_err(|e| {
        error!(
            "Rhai lazy expression returned an unusable value",
            detail = e.to_string()
        )
    })
}
