//! Constructor functions for the SurrealDB Rhai DSL.

use crate::AnySurrealType;
use crate::Expr;
use crate::identifier::Identifier;
use crate::primitives;
use crate::sum::Fx;
use rhai::{Array, FnPtr, NativeCallContext};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::operators::to_expr;
use super::types::RhaiCase;
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

// ── Parameters: param(name) → $name ─────────────────────────────────────

/// `param("parent")` → `$parent`. Covers any SurrealDB `$`-parameter
/// (`$parent`, `$this`, `$value`, …) or a `LET`-bound name — "parameter" is
/// SurrealDB's own term for `$`-prefixed names. (Rhai reserves `var`, so this
/// is named `param`.) The string `[...]` indexer adds the field tail:
/// `param("parent")["id"]` → `$parent.id`.
pub fn fn_param(name: &str) -> RhaiExpr {
    RhaiExpr(crate::variable::Variable::new(name).expr())
}

/// `parent("field")` → `$parent.field` (sugar over [`fn_param`]).
pub fn fn_parent(field: &str) -> RhaiExpr {
    RhaiExpr(primitives::field(
        crate::variable::Variable::new("parent"),
        field,
    ))
}

/// `parent()` → `$parent` (sugar over [`fn_param`]).
pub fn fn_parent_bare() -> RhaiExpr {
    fn_param("parent")
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

// ── Tier 1 shared-vocabulary primitives (same names as vantage-sql) ─────

/// `count(expr)` → `count(expr)` (arg overload of the zero-arg `count()`).
pub fn fn_count_of(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::count_of(unwrap_expr(arg)?)))
}

/// `count_distinct(expr)` → `count(array::distinct(expr))`.
pub fn fn_count_distinct(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::count_distinct(unwrap_expr(arg)?)))
}

/// `avg(expr)` → `math::mean(expr)`.
pub fn fn_avg(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::avg(unwrap_expr(arg)?)))
}

/// `round(expr)` → `math::round(expr)`.
pub fn fn_round(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::round(unwrap_expr(arg)?)))
}

/// `round(expr, places)` → `math::fixed(expr, places)` (round to N decimals).
pub fn fn_round_to(arg: rhai::Dynamic, places: i64) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::round_to(unwrap_expr(arg)?, places)))
}

// ── Tier 2 scalar/collection functions ──────────────────────────────────

/// `first(expr)` → `array::first(expr)`.
pub fn fn_first(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::first(unwrap_expr(arg)?)))
}

/// `len(expr)` → `array::len(expr)`.
pub fn fn_len(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::len(unwrap_expr(arg)?)))
}

/// `stddev(expr)` → `math::stddev(expr)`.
pub fn fn_stddev(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::stddev(unwrap_expr(arg)?)))
}

/// `median(expr)` → `math::median(expr)`.
pub fn fn_median(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::median(unwrap_expr(arg)?)))
}

/// `lower(expr)` → `string::lowercase(expr)`.
pub fn fn_lower(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::lower(unwrap_expr(arg)?)))
}

/// `words(expr)` → `string::words(expr)`.
pub fn fn_words(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::words(unwrap_expr(arg)?)))
}

/// `object_entries(expr)` → `object::entries(expr)`.
pub fn fn_object_entries(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::object_entries(unwrap_expr(arg)?)))
}

/// `object_values(expr)` → `object::values(expr)`.
pub fn fn_object_values(arg: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::object_values(unwrap_expr(arg)?)))
}

/// `time_group(expr, unit)` → `time::group(expr, $unit)` (unit bound as a parameter).
pub fn fn_time_group(arg: rhai::Dynamic, unit: &str) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::time_group(unwrap_expr(arg)?, unit)))
}

/// `similarity(expr, term)` → `string::similarity::jaro_winkler(expr, $term)` (term bound).
pub fn fn_similarity(arg: rhai::Dynamic, term: &str) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::similarity(unwrap_expr(arg)?, term)))
}

/// `coalesce(a, b)` → `a ?? b`.
pub fn fn_coalesce(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::coalesce(to_expr(a)?, to_expr(b)?)))
}

/// `nullif(a, b)` → `IF a = b THEN NONE ELSE a END`.
pub fn fn_nullif(a: rhai::Dynamic, b: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::nullif(to_expr(a)?, to_expr(b)?)))
}

/// `cast(expr, "int"|"float"|"string"|"decimal"|"datetime"|…)` → `type::<ty>(expr)`.
pub fn fn_cast(e: rhai::Dynamic, ty: &str) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::cast(to_expr(e)?, ty)))
}

/// `date_format(expr, fmt)` → `time::format(expr, "fmt")`.
pub fn fn_date_format(e: rhai::Dynamic, fmt: &str) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::date_format(to_expr(e)?, fmt)))
}

// ── Graph traversal: graph(me, "edge", "table", …) / recurse ────────────

/// Field access on an expression (the string `[...]` indexer): `{expr}.{col}`.
pub fn expr_index(e: Expr, col: &str) -> Expr {
    primitives::field(e, col)
}

/// Element access on an expression (the integer `[...]` indexer): `{expr}[n]`.
pub fn expr_index_at(e: Expr, n: i64) -> Expr {
    primitives::index_at(e, n)
}

fn dyn_segment(d: &rhai::Dynamic) -> Option<String> {
    if let Some(s) = d.clone().try_cast::<rhai::ImmutableString>() {
        Some(s.to_string())
    } else {
        d.clone().try_cast::<String>()
    }
}

fn dyn_is_anchor(d: &rhai::Dynamic) -> bool {
    d.is::<RhaiExpr>() || d.is::<RhaiIdent>()
}

/// `graph(me, "edge", "table", …)` builds a graph-traversal path. Exactly one
/// argument is the *anchor* — `me` (the current record) or a nested `graph(…)`;
/// every other argument is an edge/table name. The anchor's position sets the
/// direction: anchor on the **left** walks outward (`->edge->table`), anchor on
/// the **right** walks inward (`table<-edge<-…`). Nesting changes direction
/// per hop, so `graph("client", "placed", graph(me, "placed", "order"))`
/// renders `->placed->order<-placed<-client`.
pub fn graph_impl(args: Vec<rhai::Dynamic>) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    let n = args.len();
    let mut anchor_idx = None;
    for (i, a) in args.iter().enumerate() {
        if dyn_is_anchor(a) {
            if anchor_idx.is_some() {
                return Err(super::convert::rhai_err(
                    "graph: expected exactly one anchor (`me` or a sub-graph)".to_string(),
                ));
            }
            anchor_idx = Some(i);
        }
    }
    let ai = anchor_idx.ok_or_else(|| {
        super::convert::rhai_err("graph: missing anchor — pass `me` or a sub-graph".to_string())
    })?;

    let collect = |slice: &[rhai::Dynamic]| -> Result<Vec<String>, Box<rhai::EvalAltResult>> {
        slice
            .iter()
            .map(|d| {
                dyn_segment(d).ok_or_else(|| {
                    super::convert::rhai_err(format!(
                        "graph: edge/table must be a string, got '{}'",
                        d.type_name()
                    ))
                })
            })
            .collect()
    };

    if ai == 0 {
        let segs = collect(&args[1..])?;
        Ok(RhaiExpr(primitives::graph_out(
            unwrap_expr(args[0].clone())?,
            &segs,
        )))
    } else if ai == n - 1 {
        let mut segs = collect(&args[..n - 1])?;
        segs.reverse();
        Ok(RhaiExpr(primitives::graph_in(
            unwrap_expr(args[n - 1].clone())?,
            &segs,
        )))
    } else {
        Err(super::convert::rhai_err(
            "graph: `me` (the anchor) must be the first or last argument".to_string(),
        ))
    }
}

pub fn fn_graph2(a: rhai::Dynamic, b: rhai::Dynamic) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    graph_impl(vec![a, b])
}

pub fn fn_graph3(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
    c: rhai::Dynamic,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    graph_impl(vec![a, b, c])
}

pub fn fn_graph4(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
    c: rhai::Dynamic,
    d: rhai::Dynamic,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    graph_impl(vec![a, b, c, d])
}

pub fn fn_graph5(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
    c: rhai::Dynamic,
    d: rhai::Dynamic,
    e: rhai::Dynamic,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    graph_impl(vec![a, b, c, d, e])
}

pub fn fn_graph6(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
    c: rhai::Dynamic,
    d: rhai::Dynamic,
    e: rhai::Dynamic,
    f: rhai::Dynamic,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    graph_impl(vec![a, b, c, d, e, f])
}

pub fn fn_graph7(
    a: rhai::Dynamic,
    b: rhai::Dynamic,
    c: rhai::Dynamic,
    d: rhai::Dynamic,
    e: rhai::Dynamic,
    f: rhai::Dynamic,
    g: rhai::Dynamic,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    graph_impl(vec![a, b, c, d, e, f, g])
}

/// `recurse(path, min, max)` → `@.{min..max}(path)`.
pub fn fn_recurse(
    path: rhai::Dynamic,
    min: i64,
    max: i64,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    Ok(RhaiExpr(primitives::recurse(unwrap_expr(path)?, min, max)))
}

// ── Tier 3: embedded-array closures via native `|params| body` ──────────
//
// `.map`/`.fold`/`.filter` take a *native Rhai closure* and run it
// symbolically: each parameter is bound to a placeholder `$name` expression,
// so the body's operators/indexers build SurrealQL instead of computing. The
// returned value (an `Ex`, or a `#{…}` / `[…]` literal) is lowered by
// `to_expr`. The emitted `$name` is engine-chosen, not the script's parameter
// name (Rhai locals can't carry the `$`).

/// `expr.map(|item| body)` → `{expr}.map(|$value| {body})`.
pub fn fn_map(
    ctx: NativeCallContext,
    this: rhai::Dynamic,
    f: FnPtr,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    let item = RhaiExpr(primitives::closure_param("value"));
    let body: rhai::Dynamic = f.call_within_context(&ctx, (item,))?;
    Ok(RhaiExpr(primitives::array_map(
        unwrap_expr(this)?,
        &["value"],
        to_expr(body)?,
    )))
}

/// `expr.fold(init, |acc, item| body)` → `{expr}.fold({init}, |$acc, $value| {body})`.
pub fn fn_fold(
    ctx: NativeCallContext,
    this: rhai::Dynamic,
    init: rhai::Dynamic,
    f: FnPtr,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    let acc = RhaiExpr(primitives::closure_param("acc"));
    let item = RhaiExpr(primitives::closure_param("value"));
    let body: rhai::Dynamic = f.call_within_context(&ctx, (acc, item))?;
    Ok(RhaiExpr(primitives::array_fold(
        unwrap_expr(this)?,
        to_expr(init)?,
        &["acc", "value"],
        to_expr(body)?,
    )))
}

/// `expr.filter(|item| body)` → `{expr}.filter(|$value| {body})`.
pub fn fn_filter(
    ctx: NativeCallContext,
    this: rhai::Dynamic,
    f: FnPtr,
) -> Result<RhaiExpr, Box<rhai::EvalAltResult>> {
    let item = RhaiExpr(primitives::closure_param("value"));
    let body: rhai::Dynamic = f.call_within_context(&ctx, (item,))?;
    Ok(RhaiExpr(primitives::array_filter(
        unwrap_expr(this)?,
        &["value"],
        to_expr(body)?,
    )))
}

// ── case_when().when().else_().expr() → IF … THEN … ELSE … END ──────────

pub fn fn_case_new() -> RhaiCase {
    RhaiCase(primitives::Case::new())
}

pub fn fn_case_when(
    c: RhaiCase,
    cond: rhai::Dynamic,
    then: rhai::Dynamic,
) -> Result<RhaiCase, Box<rhai::EvalAltResult>> {
    Ok(RhaiCase(c.0.when(to_expr(cond)?, to_expr(then)?)))
}

pub fn fn_case_else(
    c: RhaiCase,
    value: rhai::Dynamic,
) -> Result<RhaiCase, Box<rhai::EvalAltResult>> {
    Ok(RhaiCase(c.0.else_(to_expr(value)?)))
}

pub fn fn_case_expr(c: RhaiCase) -> RhaiExpr {
    RhaiExpr(c.0.expr())
}
