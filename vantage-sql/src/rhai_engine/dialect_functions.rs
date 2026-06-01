//! Dialect-aware SQL functions for Rhai.
//!
//! These functions generate the correct SQL syntax for each database backend.

use std::fmt::{Debug, Display};
use rhai::Array;
use vantage_expressions::{Expressive, ExpressiveEnum, Expression};
use crate::primitives::identifier::Identifier;
use super::{RhaiExpr, RhaiIdent};

/// Extract Expression<V> from a Dynamic that is either RhaiExpr or RhaiIdent.
fn unwrap_expr<V: Clone + 'static>(val: rhai::Dynamic) -> Result<Expression<V>, Box<rhai::EvalAltResult>>
where Identifier: Expressive<V>,
{
    if let Some(e) = val.clone().try_cast::<RhaiExpr<V>>() {
        Ok(e.0)
    } else if let Some(i) = val.clone().try_cast::<RhaiIdent>() {
        Ok(Expressive::<V>::expr(&i.0))
    } else {
        Err(super::convert::rhai_err(format!(
            "expected Expr or Ident, got '{}'", val.type_name()
        )))
    }
}

// ── SQLite functions ───────────────────────────────────────────────────

pub fn fn_strftime_sqlite<V: Clone + Debug + Display + 'static>(
    format: &str,
    value: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where Identifier: Expressive<V>,
{
    let expr = unwrap_expr::<V>(value)?;
    Ok(RhaiExpr(Expression::new(
        &format!("STRFTIME('{}', {{}})", format),
        vec![ExpressiveEnum::Nested(expr)],
    )))
}

pub fn fn_group_concat_sqlite<V: Clone + Debug + Display + 'static>(
    value: rhai::Dynamic,
    distinct: bool,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where Identifier: Expressive<V>,
{
    let expr = unwrap_expr::<V>(value)?;
    let sql = if distinct {
        "GROUP_CONCAT(DISTINCT {})"
    } else {
        "GROUP_CONCAT({})"
    };
    Ok(RhaiExpr(Expression::new(
        sql,
        vec![ExpressiveEnum::Nested(expr)],
    )))
}

// ── PostgreSQL functions ──────────────────────────────────────────────

pub fn fn_strftime_postgres<V: Clone + Debug + Display + 'static>(
    format: &str,
    value: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where Identifier: Expressive<V>,
{
    let expr = unwrap_expr::<V>(value)?;
    // Convert strftime format to PostgreSQL TO_CHAR format
    let pg_format = format
        .replace("%Y", "YYYY")
        .replace("%m", "MM")
        .replace("%d", "DD")
        .replace("%H", "HH24")
        .replace("%M", "MI")
        .replace("%S", "SS");
    Ok(RhaiExpr(Expression::new(
        &format!("TO_CHAR({{}}, '{}')", pg_format),
        vec![ExpressiveEnum::Nested(expr)],
    )))
}

pub fn fn_group_concat_postgres<V: Clone + Debug + Display + 'static>(
    value: rhai::Dynamic,
    distinct: bool,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where Identifier: Expressive<V>,
{
    let expr = unwrap_expr::<V>(value)?;
    let sql = if distinct {
        "STRING_AGG(DISTINCT {}, ',')"
    } else {
        "STRING_AGG({}, ',')"
    };
    Ok(RhaiExpr(Expression::new(
        sql,
        vec![ExpressiveEnum::Nested(expr)],
    )))
}

// ── MySQL functions ──────────────────────────────────────────────────

pub fn fn_strftime_mysql<V: Clone + Debug + Display + 'static>(
    format: &str,
    value: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where Identifier: Expressive<V>,
{
    let expr = unwrap_expr::<V>(value)?;
    Ok(RhaiExpr(Expression::new(
        &format!("DATE_FORMAT({{}}, '{}')", format),
        vec![ExpressiveEnum::Nested(expr)],
    )))
}

pub fn fn_group_concat_mysql<V: Clone + Debug + Display + 'static>(
    value: rhai::Dynamic,
    distinct: bool,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where Identifier: Expressive<V>,
{
    let expr = unwrap_expr::<V>(value)?;
    let sql = if distinct {
        "GROUP_CONCAT(DISTINCT {})"
    } else {
        "GROUP_CONCAT({})"
    };
    Ok(RhaiExpr(Expression::new(
        sql,
        vec![ExpressiveEnum::Nested(expr)],
    )))
}

// ── Registration macros ──────────────────────────────────────────────

#[macro_export]
macro_rules! register_dialect_functions {
    ($engine:expr, value: $V:ty, dialect: sqlite) => {{
        $engine.register_fn("strftime", $crate::rhai_engine::dialect_functions::fn_strftime_sqlite::<$V>);
        $engine.register_fn("group_concat", $crate::rhai_engine::dialect_functions::fn_group_concat_sqlite::<$V>);
    }};
    ($engine:expr, value: $V:ty, dialect: postgres) => {{
        $engine.register_fn("strftime", $crate::rhai_engine::dialect_functions::fn_strftime_postgres::<$V>);
        $engine.register_fn("group_concat", $crate::rhai_engine::dialect_functions::fn_group_concat_postgres::<$V>);
    }};
    ($engine:expr, value: $V:ty, dialect: mysql) => {{
        $engine.register_fn("strftime", $crate::rhai_engine::dialect_functions::fn_strftime_mysql::<$V>);
        $engine.register_fn("group_concat", $crate::rhai_engine::dialect_functions::fn_group_concat_mysql::<$V>);
    }};
}
