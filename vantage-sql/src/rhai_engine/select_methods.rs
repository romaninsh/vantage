//! Rhai select builder methods.
//!
//! Generic helper functions + the `register_select!` macro.

use crate::primitives::identifier::Identifier;
use crate::primitives::select::{JoinBuilder, SelectBuilder};
use vantage_expressions::{Expression, Expressive, Selectable};

use super::{RhaiExpr, RhaiIdent, RhaiSelect};

// ── Generic helpers ────────────────────────────────────────────────────

pub fn select_new<V, S: Default, J, C>() -> RhaiSelect<V, S, J, C> {
    RhaiSelect::new(S::default())
}

pub fn select_from_str<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    name: &str,
) -> RhaiSelect<V, S, J, C>
where
    S: Selectable<V, C>,
    V: From<String>,
{
    s.inner.add_source(name, None);
    s
}

pub fn select_from_str_as<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    name: &str,
    alias: &str,
) -> RhaiSelect<V, S, J, C>
where
    S: Selectable<V, C>,
    V: From<String>,
{
    s.inner.add_source(name, Some(alias.to_string()));
    s
}

pub fn select_from_id<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    id: RhaiIdent,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: Selectable<V, C>,
    Identifier: Expressive<V>,
{
    // id.0.expr() already includes the alias (e.g. "users" AS "u"),
    // so we pass None for the alias to avoid double-rendering.
    s.inner.add_source(id.0.expr(), None);
    s
}

pub fn select_field<V, S, J, C>(mut s: RhaiSelect<V, S, J, C>, name: &str) -> RhaiSelect<V, S, J, C>
where
    S: Selectable<V, C>,
{
    s.inner.add_field(name);
    s
}

/// Drop every field collected so far. In transform mode the seeded `base`
/// carries all of the base vista's columns; aggregations call this first so the
/// resulting `GROUP BY` is valid on strict backends (PostgreSQL, MySQL), which
/// reject bare non-grouped columns that SQLite would silently tolerate.
pub fn select_clear_fields<V, S, J, C>(mut s: RhaiSelect<V, S, J, C>) -> RhaiSelect<V, S, J, C>
where
    S: Selectable<V, C>,
{
    s.inner.clear_fields();
    s
}

pub fn select_expression<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    expr: RhaiExpr<V>,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: Selectable<V, C>,
{
    s.inner.add_expression(expr.0);
    s
}

pub fn select_expression_id<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    id: RhaiIdent,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: Selectable<V, C>,
    Identifier: Expressive<V>,
{
    s.inner.add_expression(id.0.expr());
    s
}

pub fn select_where<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    cond: RhaiExpr<V>,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: Selectable<V, C>,
    Expression<V>: Into<C>,
{
    s.inner.add_where_condition(cond.0);
    s
}

pub fn select_group_by<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    expr: RhaiExpr<V>,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: Selectable<V, C>,
{
    s.inner.add_group_by(expr.0);
    s
}

pub fn select_group_by_id<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    id: RhaiIdent,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: Selectable<V, C>,
    Identifier: Expressive<V>,
{
    s.inner.add_group_by(id.0.expr());
    s
}

pub fn select_order_by<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    expr: RhaiExpr<V>,
    dir: &str,
) -> Result<RhaiSelect<V, S, J, C>, Box<rhai::EvalAltResult>>
where
    V: Clone,
    S: Selectable<V, C>,
    Expression<V>: Into<C>,
{
    let order = super::convert::parse_order(dir)?;
    s.inner.add_order_by(expr.0, order);
    Ok(s)
}

pub fn select_order_by_id<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    id: RhaiIdent,
    dir: &str,
) -> Result<RhaiSelect<V, S, J, C>, Box<rhai::EvalAltResult>>
where
    V: Clone,
    S: Selectable<V, C>,
    Expression<V>: Into<C>,
    Identifier: Expressive<V>,
{
    let order = super::convert::parse_order(dir)?;
    s.inner.add_order_by(id.0.expr(), order);
    Ok(s)
}

pub fn select_having<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    cond: RhaiExpr<V>,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: SelectBuilder<V, Join = J>,
{
    s.inner.push_having(cond.0);
    s
}

pub fn select_distinct<V, S, J, C>(mut s: RhaiSelect<V, S, J, C>) -> RhaiSelect<V, S, J, C>
where
    S: Selectable<V, C>,
{
    s.inner.set_distinct(true);
    s
}

pub fn select_limit<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    limit: i64,
    skip: i64,
) -> RhaiSelect<V, S, J, C>
where
    S: Selectable<V, C>,
{
    s.inner.set_limit(Some(limit), Some(skip));
    s
}

pub fn select_inner_join<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    table: &str,
    alias: &str,
    on: RhaiExpr<V>,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: SelectBuilder<V, Join = J>,
    J: JoinBuilder<V>,
{
    s.inner.push_join(J::make_inner(table, alias, on.0));
    s
}

pub fn select_left_join<V, S, J, C>(
    mut s: RhaiSelect<V, S, J, C>,
    table: &str,
    alias: &str,
    on: RhaiExpr<V>,
) -> RhaiSelect<V, S, J, C>
where
    V: Clone,
    S: SelectBuilder<V, Join = J>,
    J: JoinBuilder<V>,
{
    s.inner.push_join(J::make_left(table, alias, on.0));
    s
}

// ── Macro ──────────────────────────────────────────────────────────────

#[macro_export]
macro_rules! register_select {
    ($engine:expr, value: $V:ty, select: $Select:ty, join: $Join:ty, cond: $Cond:ty) => {{
        $engine.register_fn(
            "select",
            $crate::rhai_engine::select_methods::select_new::<$V, $Select, $Join, $Cond>,
        );

        $engine.register_fn(
            "from",
            $crate::rhai_engine::select_methods::select_from_str::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "from",
            $crate::rhai_engine::select_methods::select_from_id::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "from_as",
            $crate::rhai_engine::select_methods::select_from_str_as::<$V, $Select, $Join, $Cond>,
        );

        $engine.register_fn(
            "field",
            $crate::rhai_engine::select_methods::select_field::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "clear_fields",
            $crate::rhai_engine::select_methods::select_clear_fields::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "expression",
            $crate::rhai_engine::select_methods::select_expression::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "expression",
            $crate::rhai_engine::select_methods::select_expression_id::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "where",
            $crate::rhai_engine::select_methods::select_where::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "group_by",
            $crate::rhai_engine::select_methods::select_group_by::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "group_by",
            $crate::rhai_engine::select_methods::select_group_by_id::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "order_by",
            $crate::rhai_engine::select_methods::select_order_by::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "order_by",
            $crate::rhai_engine::select_methods::select_order_by_id::<$V, $Select, $Join, $Cond>,
        );

        $engine.register_fn(
            "having",
            $crate::rhai_engine::select_methods::select_having::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "distinct",
            $crate::rhai_engine::select_methods::select_distinct::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "limit",
            $crate::rhai_engine::select_methods::select_limit::<$V, $Select, $Join, $Cond>,
        );

        $engine.register_fn(
            "inner_join",
            $crate::rhai_engine::select_methods::select_inner_join::<$V, $Select, $Join, $Cond>,
        );
        $engine.register_fn(
            "left_join",
            $crate::rhai_engine::select_methods::select_left_join::<$V, $Select, $Join, $Cond>,
        );
    }};
}
