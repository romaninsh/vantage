//! Constructor functions for the SurrealDB Rhai DSL.

use crate::AnySurrealType;
use crate::Expr;
use crate::identifier::Identifier;
use crate::sum::Fx;
use rhai::Array;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::{RhaiExpr, RhaiIdent};

/// Extract Expression from a Dynamic that is either RhaiExpr or RhaiIdent.
pub fn unwrap_expr(val: rhai::Dynamic) -> Result<Expr, Box<rhai::EvalAltResult>> {
    if let Some(e) = val.clone().try_cast::<RhaiExpr>() {
        Ok(e.0)
    } else if let Some(i) = val.clone().try_cast::<RhaiIdent>() {
        Ok(i.0.expr())
    } else {
        Err(super::convert::rhai_err(format!(
            "expected Expr or Ident, got '{}'",
            val.type_name()
        )))
    }
}

// ── Ident constructors ─────────────────────────────────────────────────

pub fn make_rhai_ident(name: &str) -> RhaiIdent {
    RhaiIdent(Identifier::new(name))
}

pub fn ident_dot(id: Identifier, field: &str) -> RhaiIdent {
    // SurrealDB dot notation: table.field
    RhaiIdent(Identifier::new(format!(
        "{}.{}",
        id.expr().preview(),
        field
    )))
}

pub fn ident_as_alias(id: Identifier, alias: &str) -> Expr {
    Expression::new(
        "{} AS {}",
        vec![
            ExpressiveEnum::Nested(id.expr()),
            ExpressiveEnum::Nested(Identifier::new(alias).expr()),
        ],
    )
}

pub fn ident_index(id: Identifier, col: &str) -> RhaiIdent {
    let prefix = id.expr().preview();
    RhaiIdent(Identifier::new(format!("{}.{}", prefix, col)))
}

pub fn expr_as_alias(e: Expr, alias: &str) -> Expr {
    Expression::new(
        "{} AS {}",
        vec![
            ExpressiveEnum::Nested(e),
            ExpressiveEnum::Nested(Identifier::new(alias).expr()),
        ],
    )
}

// ── Expr constructors ──────────────────────────────────────────────────

pub fn make_expr0(template: &str) -> RhaiExpr {
    RhaiExpr(Expression::new(template, vec![]))
}

pub fn make_expr(template: &str, args: Array) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    let params: Result<Vec<ExpressiveEnum<AnySurrealType>>, _> = args
        .into_iter()
        .map(|a| {
            if let Some(e) = a.clone().try_cast::<RhaiExpr>() {
                Ok(ExpressiveEnum::Nested(e.0))
            } else if let Some(i) = a.clone().try_cast::<RhaiIdent>() {
                Ok(ExpressiveEnum::Nested(i.0.expr()))
            } else if let Some(v) = a.clone().try_cast::<i64>() {
                Ok(ExpressiveEnum::Scalar(AnySurrealType::from(v)))
            } else if let Some(v) = a.clone().try_cast::<f64>() {
                Ok(ExpressiveEnum::Scalar(AnySurrealType::from(v)))
            } else if let Some(v) = a.clone().try_cast::<bool>() {
                Ok(ExpressiveEnum::Scalar(AnySurrealType::from(v)))
            } else if let Some(v) = a.clone().try_cast::<rhai::ImmutableString>() {
                Ok(ExpressiveEnum::Scalar(AnySurrealType::from(v.to_string())))
            } else if let Some(v) = a.clone().try_cast::<String>() {
                Ok(ExpressiveEnum::Scalar(AnySurrealType::from(v)))
            } else {
                Err(super::convert::rhai_err(format!(
                    "expr: unsupported param type '{}'",
                    a.type_name()
                )))
            }
        })
        .collect();
    Ok(RhaiExpr(Expression::new(template, params?)))
}

// ── SurrealDB function call ────────────────────────────────────────────

pub fn make_fx(name: &str, args: Array) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    let exprs: Result<Vec<Expr>, _> = args
        .into_iter()
        .map(|a| {
            if let Some(e) = a.clone().try_cast::<RhaiExpr>() {
                Ok(e.0)
            } else if let Some(i) = a.clone().try_cast::<RhaiIdent>() {
                Ok(i.0.expr())
            } else {
                Err(super::convert::rhai_err(format!(
                    "fx: expected Expr or Ident, got '{}'",
                    a.type_name()
                )))
            }
        })
        .collect();
    Ok(RhaiExpr(Fx::new(name, exprs?).expr()))
}

// ── Aggregates ─────────────────────────────────────────────────────────

pub fn fn_count_expr() -> RhaiExpr {
    RhaiExpr(Expression::new("count()", vec![]))
}

pub fn fn_sum(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(
        Fx::new("math::sum", vec![unwrap_expr(arg)?]).expr(),
    ))
}

pub fn fn_min(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(
        Fx::new("math::min", vec![unwrap_expr(arg)?]).expr(),
    ))
}

pub fn fn_max(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(
        Fx::new("math::max", vec![unwrap_expr(arg)?]).expr(),
    ))
}

// ── Arithmetic ─────────────────────────────────────────────────────────

pub fn fn_mul(a: rhai::Dynamic, b: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(Expression::new(
        "({} * {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr(a)?),
            ExpressiveEnum::Nested(unwrap_expr(b)?),
        ],
    )))
}

pub fn fn_add(a: rhai::Dynamic, b: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(Expression::new(
        "({} + {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr(a)?),
            ExpressiveEnum::Nested(unwrap_expr(b)?),
        ],
    )))
}

pub fn fn_sub(a: rhai::Dynamic, b: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(Expression::new(
        "({} - {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr(a)?),
            ExpressiveEnum::Nested(unwrap_expr(b)?),
        ],
    )))
}

pub fn fn_div(a: rhai::Dynamic, b: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(Expression::new(
        "({} / {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr(a)?),
            ExpressiveEnum::Nested(unwrap_expr(b)?),
        ],
    )))
}

// ── Record ID constructor ──────────────────────────────────────────────

/// type::thing("table", "id") → creates a Record ID reference
pub fn fn_thing(table: &str, id: &str) -> RhaiExpr {
    RhaiExpr(Expression::new(
        format!("type::thing(\"{}\", \"{}\")", table, id),
        vec![],
    ))
}

// ── Parent reference ───────────────────────────────────────────────────

/// parent("field") → $parent.field
pub fn fn_parent(field: &str) -> RhaiExpr {
    RhaiExpr(Expression::new(
        "$parent.{}",
        vec![ExpressiveEnum::Nested(Identifier::new(field).expr())],
    ))
}

/// parent() → $parent
pub fn fn_parent_bare() -> RhaiIdent {
    RhaiIdent(Identifier::new("$parent"))
}

// ── SurrealDB namespaced functions ─────────────────────────────────────

pub fn fn_array_group(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(
        Fx::new("array::group", vec![unwrap_expr(arg)?]).expr(),
    ))
}

pub fn fn_time_now() -> RhaiExpr {
    RhaiExpr(Expression::new("time::now()", vec![]))
}

pub fn fn_string_len(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(
        Fx::new("string::len", vec![unwrap_expr(arg)?]).expr(),
    ))
}

pub fn fn_type_float(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(
        Fx::new("type::float", vec![unwrap_expr(arg)?]).expr(),
    ))
}

pub fn fn_type_int(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(
        Fx::new("type::int", vec![unwrap_expr(arg)?]).expr(),
    ))
}
