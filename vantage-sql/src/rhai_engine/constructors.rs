//! Generic helper functions and the `register_constructors!` macro.
//!
//! All helpers are generic over the value type V, monomorphised by the
//! macro at the call site.

use crate::primitives::concat::Concat;
use crate::primitives::fx::Fx;
use crate::primitives::identifier::{Identifier, ident as make_ident};
use std::fmt::{Debug, Display};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};
use vantage_rhai::rhai;
use vantage_rhai::rhai::Array;

use super::{RhaiExpr, RhaiIdent};

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

pub fn make_expr0<V: Clone>(template: &str) -> RhaiExpr<V> {
    RhaiExpr(Expression::new(template, vec![]))
}

/// An expression or identifier as it is; a string, int, float or bool as a
/// bound value, never SQL text. `None` for any other type.
fn to_param<V>(a: &rhai::Dynamic) -> Option<ExpressiveEnum<V>>
where
    V: Clone + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
{
    if let Some(e) = a.clone().try_cast::<RhaiExpr<V>>() {
        Some(ExpressiveEnum::Nested(e.0))
    } else if let Some(i) = a.clone().try_cast::<RhaiIdent>() {
        Some(ExpressiveEnum::Nested(Expressive::<V>::expr(&i.0)))
    } else if let Some(v) = a.clone().try_cast::<i64>() {
        Some(ExpressiveEnum::Scalar(V::from(v)))
    } else if let Some(v) = a.clone().try_cast::<f64>() {
        Some(ExpressiveEnum::Scalar(V::from(v)))
    } else if let Some(v) = a.clone().try_cast::<bool>() {
        Some(ExpressiveEnum::Scalar(V::from(v)))
    } else {
        a.clone()
            .try_cast::<rhai::ImmutableString>()
            .map(|v| ExpressiveEnum::Scalar(V::from(v.to_string())))
    }
}

pub fn make_expr<V>(template: &str, args: Array) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
{
    let params: Result<Vec<ExpressiveEnum<V>>, _> = args
        .iter()
        .map(|a| {
            to_param(a).ok_or_else(|| {
                super::convert::rhai_err(format!(
                    "expr: unsupported param type '{}'",
                    a.type_name()
                ))
            })
        })
        .collect();
    Ok(RhaiExpr(Expression::new(template, params?)))
}

/// One `sql::concat` / `sql::concat_ws` argument as an expression; `what`
/// names it in the error. A number or bool is bound as its text: Postgres
/// has no `integer || bigint`, and MySQL would read `true` as `1`.
fn concat_arg<V>(
    a: &rhai::Dynamic,
    func: &str,
    what: &str,
) -> Result<Expression<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
{
    let as_text = if let Some(v) = a.clone().try_cast::<i64>() {
        Some(v.to_string())
    } else if let Some(v) = a.clone().try_cast::<f64>() {
        Some(v.to_string())
    } else {
        a.clone().try_cast::<bool>().map(|v| v.to_string())
    };
    let a = &as_text.map_or_else(|| a.clone(), rhai::Dynamic::from);
    match to_param(a) {
        Some(ExpressiveEnum::Nested(e)) => Ok(e),
        Some(scalar) => Ok(Expression::new("{}", vec![scalar])),
        None => Err(super::convert::rhai_err(format!(
            "sql::{func}: {what} is a {}; expected sql::ident, an expression, a string, number or bool",
            super::convert::friendly_type_name(a)
        ))),
    }
}

fn concat_parts<V>(
    parts: &Array,
    func: &str,
) -> Result<Vec<Expression<V>>, Box<rhai::EvalAltResult>>
where
    V: Clone + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
{
    if parts.is_empty() {
        return Err(super::convert::rhai_err(format!(
            "sql::{func} needs at least one part"
        )));
    }
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| concat_arg(p, func, &format!("the part at position {}", i + 1)))
        .collect()
}

pub fn fn_concat<V>(parts: Array) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + Debug + Display + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
    Concat<V>: Expressive<V>,
{
    Ok(RhaiExpr(
        Concat::new(concat_parts(&parts, "concat")?).expr(),
    ))
}

pub fn fn_concat_ws<V>(
    sep: rhai::Dynamic,
    parts: Array,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + Debug + Display + From<i64> + From<f64> + From<bool> + From<String> + 'static,
    Identifier: Expressive<V>,
    Concat<V>: Expressive<V>,
{
    let sep = concat_arg(&sep, "concat_ws", "the separator")?;
    Ok(RhaiExpr(
        Concat::new(concat_parts(&parts, "concat_ws")?)
            .ws(sep)
            .expr(),
    ))
}

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
        format!("CAST({{}} AS {sql_type})"),
        vec![ExpressiveEnum::Nested(unwrap_expr::<V>(e)?)],
    )))
}

/// `decimals` is written into the SQL, not bound: a bound i64 is a `bigint`
/// on Postgres, which has no `round(numeric, bigint)`.
pub fn fn_round<V>(
    value: rhai::Dynamic,
    decimals: i64,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + Debug + Display + 'static,
    Identifier: Expressive<V>,
{
    Ok(RhaiExpr(
        Fx::new(
            "ROUND",
            vec![
                unwrap_expr::<V>(value)?,
                Expression::new(decimals.to_string(), vec![]),
            ],
        )
        .expr(),
    ))
}

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

/// `sql::json(expr, path)`: the JSON value at `path` (a key or a list of
/// keys) of `expr`, as text; `raw` keeps it JSON.
pub fn fn_json<V>(
    expr: rhai::Dynamic,
    path: rhai::Dynamic,
    raw: bool,
) -> Result<RhaiExpr<V>, Box<rhai::EvalAltResult>>
where
    V: Clone + Debug + Display + 'static,
    Identifier: Expressive<V>,
    crate::primitives::json_extract::JsonExtract<V>: Expressive<V>,
{
    use crate::primitives::json_extract::JsonExtract;
    let keys: Vec<String> = if let Some(key) = path.clone().try_cast::<rhai::ImmutableString>() {
        vec![key.to_string()]
    } else if let Some(keys) = path.clone().try_cast::<Array>() {
        keys.into_iter()
            .map(|k| {
                k.try_cast::<rhai::ImmutableString>()
                    .map(|k| k.to_string())
                    .ok_or_else(|| super::convert::rhai_err("sql::json path keys are strings"))
            })
            .collect::<Result<_, _>>()?
    } else {
        return Err(super::convert::rhai_err(format!(
            "sql::json path is a key or a list of keys, got '{}'",
            super::convert::friendly_type_name(&path)
        )));
    };
    if keys.is_empty() {
        return Err(super::convert::rhai_err("sql::json needs a path"));
    }
    // The keys are written into the SQL as literals: only plain names, so
    // no quote, backslash or `{}` can reach the statement.
    if let Some(bad) = keys.iter().find(|k| {
        k.is_empty()
            || !k
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }) {
        return Err(super::convert::rhai_err(format!(
            "sql::json path keys are letters, digits, `_` and `-`; got {bad:?}"
        )));
    }
    let extract = JsonExtract::new(unwrap_expr::<V>(expr)?, keys);
    let extract = if raw { extract.as_json() } else { extract };
    Ok(RhaiExpr(extract.expr()))
}

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

#[macro_export]
macro_rules! register_constructors {
    ($engine:expr, $ns:expr, value: $V:ty) => {{
        // Uses type aliases Ex, Id, Expr from register_engine!

        $ns.func("ident", $crate::rhai_engine::constructors::make_rhai_ident)
            .func("expr", $crate::rhai_engine::constructors::make_expr0::<$V>)
            .func("expr", $crate::rhai_engine::constructors::make_expr::<$V>)
            .func("fx", $crate::rhai_engine::constructors::make_fx::<$V>)
            .func("concat", $crate::rhai_engine::constructors::fn_concat::<$V>)
            .func(
                "concat_ws",
                $crate::rhai_engine::constructors::fn_concat_ws::<$V>,
            );

        $ns.func("sum", $crate::rhai_engine::constructors::fn_sum::<$V>)
            .func("count", $crate::rhai_engine::constructors::fn_count::<$V>)
            .func(
                "count_distinct",
                $crate::rhai_engine::constructors::fn_count_distinct::<$V>,
            )
            .func("avg", $crate::rhai_engine::constructors::fn_avg::<$V>)
            .func("min", $crate::rhai_engine::constructors::fn_min::<$V>)
            .func("max", $crate::rhai_engine::constructors::fn_max::<$V>);

        $ns.func(
            "coalesce",
            $crate::rhai_engine::constructors::fn_coalesce::<$V>,
        )
        .func("nullif", $crate::rhai_engine::constructors::fn_nullif::<$V>)
        .func("cast", $crate::rhai_engine::constructors::fn_cast::<$V>)
        .func("round", $crate::rhai_engine::constructors::fn_round::<$V>);

        $ns.func("mul", $crate::rhai_engine::constructors::fn_mul::<$V>)
            .func("add", $crate::rhai_engine::constructors::fn_add::<$V>)
            .func("sub", $crate::rhai_engine::constructors::fn_sub::<$V>)
            .func("div", $crate::rhai_engine::constructors::fn_div::<$V>);

        $ns.func(
            "date_format",
            $crate::rhai_engine::constructors::fn_date_format::<$V>,
        )
        .func(
            "group_concat",
            $crate::rhai_engine::constructors::fn_group_concat::<$V>,
        )
        .func("json", |e: rhai::Dynamic, path: rhai::Dynamic| {
            $crate::rhai_engine::constructors::fn_json::<$V>(e, path, false)
        })
        .func("json_raw", |e: rhai::Dynamic, path: rhai::Dynamic| {
            $crate::rhai_engine::constructors::fn_json::<$V>(e, path, true)
        });

        $engine.register_fn("dot_of", |id: &mut Id, prefix: &str| -> Id {
            $crate::rhai_engine::RhaiIdent($crate::rhai_engine::constructors::ident_dot_of(
                id.0.clone(),
                prefix,
            ))
        });

        $engine.register_fn("alias", |id: &mut Id, alias: &str| -> Id {
            $crate::rhai_engine::RhaiIdent(id.0.clone().with_alias(alias))
        });

        $engine.register_fn("alias", |e: &mut Ex, alias: &str| -> Ex {
            $crate::rhai_engine::RhaiExpr($crate::rhai_engine::constructors::expr_as_alias::<$V>(
                e.0.clone(),
                alias,
            ))
        });

        $engine.register_indexer_get(|id: &mut Id, col: &str| -> Id {
            $crate::rhai_engine::RhaiIdent($crate::rhai_engine::constructors::ident_index(
                id.0.clone(),
                col,
            ))
        });

        $engine.register_fn("clone", |e: Ex| -> Ex { e });
        $engine.register_fn("clone", |id: Id| -> Id { id });
    }};
}
