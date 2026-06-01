//! Generic helper functions and the `register_constructors!` macro.
//!
//! All helpers are generic over the value type V, monomorphised by the
//! macro at the call site.

use crate::primitives::fx::Fx;
use crate::primitives::identifier::{Identifier, ident as make_ident};
use rhai::Array;
use std::fmt::{Debug, Display};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::{RhaiExpr, RhaiIdent};

/// Extract Expression<V> from a Dynamic that is either RhaiExpr or RhaiIdent.
fn unwrap_expr<V: Clone + 'static>(
    val: rhai::Dynamic,
) -> Result<Expression<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    if let Some(e) = val.clone().try_cast::<RhaiExpr<V>>() {
        Ok(e.0)
    } else if let Some(i) = val.clone().try_cast::<RhaiIdent>() {
        Ok(Expressive::<V>::expr(&i.0))
    } else {
        Err(super::convert::rhai_err(format!(
            "expected Expr or Ident, got '{}'",
            val.type_name()
        )))
    }
}

// ── Ident constructors ─────────────────────────────────────────────────

pub fn make_rhai_ident(name: &str) -> RhaiIdent {
    RhaiIdent(make_ident(name))
}

pub fn ident_dot_of(id: Identifier, prefix: &str) -> Identifier {
    id.dot_of(prefix)
}

pub fn ident_as_alias<V: Clone>(id: Identifier, alias: &str) -> Expression<V>
where
    Identifier: Expressive<V>,
{
    Expression::new(
        "{} AS {}",
        vec![
            ExpressiveEnum::Nested(Expressive::<V>::expr(&id)),
            ExpressiveEnum::Nested(Expressive::<V>::expr(&Identifier::new(alias))),
        ],
    )
}

pub fn ident_index(id: Identifier, col: &str) -> Identifier {
    let prefix = id.alias().unwrap_or(&id.name()).to_string();
    make_ident(col).dot_of(prefix)
}

pub fn expr_as_alias<V: Clone>(e: Expression<V>, alias: &str) -> Expression<V>
where
    Identifier: Expressive<V>,
{
    Expression::new(
        "{} AS {}",
        vec![
            ExpressiveEnum::Nested(e),
            ExpressiveEnum::Nested(Expressive::<V>::expr(&Identifier::new(alias))),
        ],
    )
}

// ── Expr constructors ──────────────────────────────────────────────────

pub fn make_expr0<V: Clone>(template: &str) -> RhaiExpr<V> {
    RhaiExpr(Expression::new(template, vec![]))
}

pub fn make_expr<V>(template: &str, args: Array) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
{
    let params: Result<Vec<ExpressiveEnum<V>>, _> = args
        .into_iter()
        .map(|a| {
            if let Some(e) = a.clone().try_cast::<RhaiExpr<V>>() {
                Ok(ExpressiveEnum::Nested(e.0))
            } else if let Some(i) = a.clone().try_cast::<RhaiIdent>() {
                Ok(ExpressiveEnum::Nested(Expressive::<V>::expr(&i.0)))
            } else if let Some(v) = a.clone().try_cast::<i64>() {
                Ok(ExpressiveEnum::Scalar(V::from(v)))
            } else if let Some(v) = a.clone().try_cast::<f64>() {
                Ok(ExpressiveEnum::Scalar(V::from(v)))
            } else if let Some(v) = a.clone().try_cast::<bool>() {
                Ok(ExpressiveEnum::Scalar(V::from(v)))
            } else if let Some(v) = a.clone().try_cast::<rhai::ImmutableString>() {
                Ok(ExpressiveEnum::Scalar(V::from(v.to_string())))
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

// ── SQL function call ──────────────────────────────────────────────────

pub fn make_fx<V: Clone + Debug + Display + 'static>(
    name: &str,
    args: Array,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    let exprs: Result<Vec<Expression<V>>, _> = args
        .into_iter()
        .map(|a| {
            if let Some(e) = a.clone().try_cast::<RhaiExpr<V>>() {
                Ok(e.0)
            } else if let Some(i) = a.clone().try_cast::<RhaiIdent>() {
                Ok(Expressive::<V>::expr(&i.0))
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

pub fn fn_sum<V: Clone + Debug + Display + 'static>(
    arg: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new("SUM", vec![unwrap_expr::<V>(arg)?]).expr(),
    ))
}

pub fn fn_count<V: Clone + Debug + Display + 'static>(
    arg: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new("COUNT", vec![unwrap_expr::<V>(arg)?]).expr(),
    ))
}

pub fn fn_count_distinct<V: Clone + Debug + Display + 'static>(
    arg: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(Expression::new(
        "COUNT(DISTINCT {})",
        vec![ExpressiveEnum::Nested(unwrap_expr::<V>(arg)?)],
    )))
}

pub fn fn_avg<V: Clone + Debug + Display + 'static>(
    arg: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new("AVG", vec![unwrap_expr::<V>(arg)?]).expr(),
    ))
}

pub fn fn_min<V: Clone + Debug + Display + 'static>(
    arg: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new("MIN", vec![unwrap_expr::<V>(arg)?]).expr(),
    ))
}

pub fn fn_max<V: Clone + Debug + Display + 'static>(
    arg: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new("MAX", vec![unwrap_expr::<V>(arg)?]).expr(),
    ))
}

// ── SQL functions ──────────────────────────────────────────────────────

pub fn fn_coalesce<V: Clone + Debug + Display + 'static>(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new("COALESCE", vec![unwrap_expr::<V>(a)?, unwrap_expr::<V>(b)?]).expr(),
    ))
}

pub fn fn_nullif<V: Clone + Debug + Display + 'static>(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new("NULLIF", vec![unwrap_expr::<V>(a)?, unwrap_expr::<V>(b)?]).expr(),
    ))
}

pub fn fn_cast<V: Clone + Debug + Display + 'static>(
    e: rhai::Dynamic,
    sql_type: &str,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(Expression::new(
        &format!("CAST({{}} AS {sql_type})"),
        vec![ExpressiveEnum::Nested(unwrap_expr::<V>(e)?)],
    )))
}

pub fn fn_round<V>(
    value: rhai::Dynamic,
    decimals: i64,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + Debug + Display + From<i64> + 'static,
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new(
            "ROUND",
            vec![
                unwrap_expr::<V>(value)?,
                Expression::new("{}", vec![ExpressiveEnum::Scalar(V::from(decimals))]),
            ],
        )
        .expr(),
    ))
}

// ── Date/Time functions ──────────────────────────────────────────────

pub fn fn_date_format<V>(
    expr: rhai::Dynamic,
    format: &str,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + Debug + Display + 'static,
    Identifier: Expressive<V>,
    crate::primitives::date_format::DateFormat<V>: Expressive<V>,
{
    use crate::primitives::date_format::DateFormat;
    let inner = unwrap_expr::<V>(expr)?;
    Ok(RhaiExpr(DateFormat::new(inner, format).expr()))
}

pub fn fn_group_concat<V>(
    expr: rhai::Dynamic,
    distinct: bool,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + Debug + Display + 'static,
    Identifier: Expressive<V>,
    crate::primitives::group_concat::GroupConcat<V>: Expressive<V>,
{
    use crate::primitives::group_concat::GroupConcat;
    let inner = unwrap_expr::<V>(expr)?;
    let mut gc = GroupConcat::new(inner);
    if distinct {
        gc = gc.distinct();
    }
    Ok(RhaiExpr(gc.expr()))
}

// ── Arithmetic ─────────────────────────────────────────────────────────

pub fn fn_mul<V: Clone + Debug + Display + 'static>(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(Expression::new(
        "({} * {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr::<V>(a)?),
            ExpressiveEnum::Nested(unwrap_expr::<V>(b)?),
        ],
    )))
}

pub fn fn_add<V: Clone + Debug + Display + 'static>(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(Expression::new(
        "({} + {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr::<V>(a)?),
            ExpressiveEnum::Nested(unwrap_expr::<V>(b)?),
        ],
    )))
}

pub fn fn_sub<V: Clone + Debug + Display + 'static>(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(Expression::new(
        "({} - {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr::<V>(a)?),
            ExpressiveEnum::Nested(unwrap_expr::<V>(b)?),
        ],
    )))
}

pub fn fn_div<V: Clone + Debug + Display + 'static>(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(Expression::new(
        "({} / {})",
        vec![
            ExpressiveEnum::Nested(unwrap_expr::<V>(a)?),
            ExpressiveEnum::Nested(unwrap_expr::<V>(b)?),
        ],
    )))
}

// ── Macro ──────────────────────────────────────────────────────────────

#[macro_export]
macro_rules! register_constructors {
    ($engine:expr, value: $V:ty) => {{
        // Uses type aliases Ex, Id, Expr from register_engine!

        // ── Constructors ────────────────────────────────────────────
        $engine.register_fn("ident", $crate::rhai_engine::constructors::make_rhai_ident);
        $engine.register_fn("table", $crate::rhai_engine::constructors::make_rhai_ident);
        $engine.register_fn("expr", $crate::rhai_engine::constructors::make_expr0::<$V>);
        $engine.register_fn("expr", $crate::rhai_engine::constructors::make_expr::<$V>);
        $engine.register_fn("fx", $crate::rhai_engine::constructors::make_fx::<$V>);

        // ── Aggregates ──────────────────────────────────────────────
        $engine.register_fn("sum", $crate::rhai_engine::constructors::fn_sum::<$V>);
        $engine.register_fn("count", $crate::rhai_engine::constructors::fn_count::<$V>);
        $engine.register_fn("count_distinct", $crate::rhai_engine::constructors::fn_count_distinct::<$V>);
        $engine.register_fn("avg", $crate::rhai_engine::constructors::fn_avg::<$V>);
        $engine.register_fn("min", $crate::rhai_engine::constructors::fn_min::<$V>);
        $engine.register_fn("max", $crate::rhai_engine::constructors::fn_max::<$V>);

        // ── SQL functions ───────────────────────────────────────────
        $engine.register_fn("coalesce", $crate::rhai_engine::constructors::fn_coalesce::<$V>);
        $engine.register_fn("nullif", $crate::rhai_engine::constructors::fn_nullif::<$V>);
        $engine.register_fn("cast", $crate::rhai_engine::constructors::fn_cast::<$V>);
        $engine.register_fn("round", $crate::rhai_engine::constructors::fn_round::<$V>);

        // ── Arithmetic ──────────────────────────────────────────────
        $engine.register_fn("mul", $crate::rhai_engine::constructors::fn_mul::<$V>);
        $engine.register_fn("add", $crate::rhai_engine::constructors::fn_add::<$V>);
        $engine.register_fn("sub", $crate::rhai_engine::constructors::fn_sub::<$V>);
        $engine.register_fn("div", $crate::rhai_engine::constructors::fn_div::<$V>);

        // ── Date/Time functions ────────────────────────────────────────
        $engine.register_fn("date_format", $crate::rhai_engine::constructors::fn_date_format::<$V>);
        $engine.register_fn("group_concat", $crate::rhai_engine::constructors::fn_group_concat::<$V>);

        // ── Ident methods ───────────────────────────────────────────
        $engine.register_fn("dot_of", |id: &mut Id, prefix: &str| -> Id {
            $crate::rhai_engine::RhaiIdent($crate::rhai_engine::constructors::ident_dot_of(id.0.clone(), prefix))
        });

        $engine.register_fn("alias", |id: &mut Id, alias: &str| -> Id {
            $crate::rhai_engine::RhaiIdent(id.0.clone().with_alias(alias))
        });

        // ── Expr methods ────────────────────────────────────────────
        $engine.register_fn("alias", |e: &mut Ex, alias: &str| -> Ex {
            $crate::rhai_engine::RhaiExpr($crate::rhai_engine::constructors::expr_as_alias::<$V>(e.0.clone(), alias))
        });

        // ── Indexer: t["col"] ───────────────────────────────────────
        $engine.register_indexer_get(|id: &mut Id, col: &str| -> Id {
            $crate::rhai_engine::RhaiIdent($crate::rhai_engine::constructors::ident_index(id.0.clone(), col))
        });

        // ── Clone ───────────────────────────────────────────────────
        $engine.register_fn("clone", |e: Ex| -> Ex { e });
        $engine.register_fn("clone", |id: Id| -> Id { id });
    }};
}
