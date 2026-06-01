//! Window functions and CASE expressions for Rhai.
//!
//! Window: `window()`, `.partition_by()`, `.order_by()`, `.rows()`, `.range()`, `.apply()`
//! Case: `case_when()`, `.when()`, `.else_()`, `.expr()`

use std::fmt::{Debug, Display};
use vantage_expressions::Expressive;
use crate::primitives::select::window::Window;
use crate::primitives::case::Case;
use crate::primitives::identifier::Identifier;

use super::{RhaiExpr, RhaiIdent, RhaiWindow, RhaiCase};

// ── Window helpers ─────────────────────────────────────────────────────

pub fn window_new<V: Debug + Display + Clone>() -> RhaiWindow<V> {
    RhaiWindow(Window::new())
}

pub fn window_partition_by<V>(mut w: RhaiWindow<V>, expr: RhaiExpr<V>) -> RhaiWindow<V>
where V: Debug + Display + Clone,
{
    w.0 = w.0.partition_by(expr.0);
    w
}

pub fn window_partition_by_id<V>(mut w: RhaiWindow<V>, id: RhaiIdent) -> RhaiWindow<V>
where V: Debug + Display + Clone, Identifier: Expressive<V>,
{
    w.0 = w.0.partition_by(id.0.expr());
    w
}

pub fn window_order_by<V>(mut w: RhaiWindow<V>, expr: RhaiExpr<V>, dir: &str)
    -> Result<RhaiWindow<V>, Box<rhai::EvalAltResult>>
where V: Debug + Display + Clone,
{
    let order = super::convert::parse_order(dir)?;
    w.0 = w.0.order_by(expr.0, order);
    Ok(w)
}

pub fn window_order_by_id<V>(mut w: RhaiWindow<V>, id: RhaiIdent, dir: &str)
    -> Result<RhaiWindow<V>, Box<rhai::EvalAltResult>>
where V: Debug + Display + Clone, Identifier: Expressive<V>,
{
    let order = super::convert::parse_order(dir)?;
    w.0 = w.0.order_by(id.0.expr(), order);
    Ok(w)
}

pub fn window_rows<V>(mut w: RhaiWindow<V>, from: &str, to: &str) -> RhaiWindow<V>
where V: Debug + Display + Clone,
{
    w.0 = w.0.rows(from, to);
    w
}

pub fn window_range<V>(mut w: RhaiWindow<V>, from: &str, to: &str) -> RhaiWindow<V>
where V: Debug + Display + Clone,
{
    w.0 = w.0.range(from, to);
    w
}

pub fn window_apply<V>(w: RhaiWindow<V>, func: RhaiExpr<V>) -> RhaiExpr<V>
where V: Debug + Display + Clone,
{
    RhaiExpr(w.0.apply(func.0))
}

pub fn window_apply_id<V>(w: RhaiWindow<V>, id: RhaiIdent) -> RhaiExpr<V>
where V: Debug + Display + Clone, Identifier: Expressive<V>,
{
    RhaiExpr(w.0.apply(id.0.expr()))
}

// ── Case helpers ───────────────────────────────────────────────────────

pub fn case_new<V: Debug + Display + Clone>() -> RhaiCase<V> {
    RhaiCase(Case::new())
}

pub fn case_when<V>(mut c: RhaiCase<V>, cond: RhaiExpr<V>, then: RhaiExpr<V>) -> RhaiCase<V>
where V: Debug + Display + Clone,
{
    c.0 = c.0.when(cond.0, then.0);
    c
}

pub fn case_when_id<V>(mut c: RhaiCase<V>, cond: RhaiIdent, then: RhaiExpr<V>) -> RhaiCase<V>
where V: Debug + Display + Clone, Identifier: Expressive<V>,
{
    c.0 = c.0.when(cond.0.expr(), then.0);
    c
}

pub fn case_else<V>(mut c: RhaiCase<V>, value: RhaiExpr<V>) -> RhaiCase<V>
where V: Debug + Display + Clone,
{
    c.0 = c.0.else_(value.0);
    c
}

pub fn case_else_id<V>(mut c: RhaiCase<V>, id: RhaiIdent) -> RhaiCase<V>
where V: Debug + Display + Clone, Identifier: Expressive<V>,
{
    c.0 = c.0.else_(id.0.expr());
    c
}

pub fn case_expr<V>(c: RhaiCase<V>) -> RhaiExpr<V>
where V: Debug + Display + Clone,
{
    RhaiExpr(c.0.expr())
}

// ── Macro ──────────────────────────────────────────────────────────────

#[macro_export]
macro_rules! register_window_case {
    ($engine:expr, value: $V:ty) => {{
        // Window constructors and methods
        $engine.register_fn("window", $crate::rhai_engine::window_case::window_new::<$V>);

        $engine.register_fn("partition_by", $crate::rhai_engine::window_case::window_partition_by::<$V>);
        $engine.register_fn("partition_by", $crate::rhai_engine::window_case::window_partition_by_id::<$V>);

        $engine.register_fn("order_by", $crate::rhai_engine::window_case::window_order_by::<$V>);
        $engine.register_fn("order_by", $crate::rhai_engine::window_case::window_order_by_id::<$V>);

        $engine.register_fn("rows", $crate::rhai_engine::window_case::window_rows::<$V>);
        $engine.register_fn("range", $crate::rhai_engine::window_case::window_range::<$V>);

        $engine.register_fn("apply", $crate::rhai_engine::window_case::window_apply::<$V>);
        $engine.register_fn("apply", $crate::rhai_engine::window_case::window_apply_id::<$V>);

        // Case constructors and methods
        $engine.register_fn("case_when", $crate::rhai_engine::window_case::case_new::<$V>);

        $engine.register_fn("when", $crate::rhai_engine::window_case::case_when::<$V>);
        $engine.register_fn("when", $crate::rhai_engine::window_case::case_when_id::<$V>);

        $engine.register_fn("else_", $crate::rhai_engine::window_case::case_else::<$V>);
        $engine.register_fn("else_", $crate::rhai_engine::window_case::case_else_id::<$V>);

        $engine.register_fn("expr", $crate::rhai_engine::window_case::case_expr::<$V>);

        // Clone for window/case
        $engine.register_fn("clone", |w: $crate::rhai_engine::RhaiWindow<$V>| -> $crate::rhai_engine::RhaiWindow<$V> { w });
        $engine.register_fn("clone", |c: $crate::rhai_engine::RhaiCase<$V>| -> $crate::rhai_engine::RhaiCase<$V> { c });
    }};
}
