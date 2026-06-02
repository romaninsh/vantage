//! Rhai select builder methods for SurrealDB.

use crate::identifier::Identifier;
use crate::statements::select::select_target::SelectTarget;
use vantage_expressions::Expressive;

use super::{RhaiExpr, RhaiIdent, RhaiSelect};

// ── Generic helpers ────────────────────────────────────────────────────

pub fn select_new() -> RhaiSelect {
    RhaiSelect::new()
}

pub fn select_from(mut s: RhaiSelect, name: &str) -> RhaiSelect {
    s.inner.add_from(SelectTarget::new(Identifier::new(name)));
    s
}

pub fn select_from_id(mut s: RhaiSelect, id: RhaiIdent) -> RhaiSelect {
    s.inner.add_from(SelectTarget::new(id.0.expr()));
    s
}

pub fn select_from_expr(mut s: RhaiSelect, expr: RhaiExpr) -> RhaiSelect {
    s.inner.add_from(SelectTarget::new(expr.0));
    s
}

pub fn select_field(mut s: RhaiSelect, name: &str) -> RhaiSelect {
    s.inner = s.inner.field(name);
    s
}

pub fn select_expression(mut s: RhaiSelect, expr: RhaiExpr) -> RhaiSelect {
    s.inner = s.inner.with_expression(expr.0, None);
    s
}

pub fn select_expression_id(mut s: RhaiSelect, id: RhaiIdent) -> RhaiSelect {
    s.inner = s.inner.with_expression(id.0.expr(), None);
    s
}

pub fn select_where(mut s: RhaiSelect, cond: RhaiExpr) -> RhaiSelect {
    s.inner = s.inner.with_where(cond.0);
    s
}

pub fn select_group_by(mut s: RhaiSelect, expr: RhaiExpr) -> RhaiSelect {
    s.inner = s.inner.with_group_by(expr.0);
    s
}

pub fn select_group_by_id(mut s: RhaiSelect, id: RhaiIdent) -> RhaiSelect {
    s.inner = s.inner.with_group_by(id.0.expr());
    s
}

pub fn select_order_by(
    mut s: RhaiSelect,
    expr: RhaiExpr,
    dir: &str,
) -> Result<RhaiSelect, Box<rhai::EvalAltResult>> {
    let order = super::convert::parse_order(dir)?;
    s.inner = s.inner.with_order_by(expr.0, order);
    Ok(s)
}

pub fn select_order_by_id(
    mut s: RhaiSelect,
    id: RhaiIdent,
    dir: &str,
) -> Result<RhaiSelect, Box<rhai::EvalAltResult>> {
    let order = super::convert::parse_order(dir)?;
    s.inner = s.inner.with_order_by(id.0.expr(), order);
    Ok(s)
}

pub fn select_distinct(mut s: RhaiSelect) -> RhaiSelect {
    s.inner = s.inner.with_distinct();
    s
}

/// SurrealDB: LIMIT n START s
pub fn select_limit(mut s: RhaiSelect, limit: i64, start: i64) -> RhaiSelect {
    s.inner = s.inner.with_limit(limit);
    if start > 0 {
        s.inner = s.inner.with_skip(start);
    }
    s
}

// ── SurrealDB-specific ─────────────────────────────────────────────────

/// SELECT ONLY — returns a single record
pub fn select_only(mut s: RhaiSelect) -> RhaiSelect {
    s.inner.from_only = true;
    s
}

/// SELECT VALUE — returns scalar values instead of objects
pub fn select_value(mut s: RhaiSelect) -> RhaiSelect {
    s.inner.single_value = true;
    s
}

// ── Graph traversal ────────────────────────────────────────────────────

/// Follow outgoing graph edge: ident->relation
pub fn graph_arrow(id: RhaiIdent, relation: &str) -> RhaiExpr {
    RhaiExpr(Identifier::new(format!(
        "{}->{}",
        id.0.expr().preview(),
        relation
    )).expr())
}

/// Follow incoming graph edge: ident<-relation
pub fn graph_back(id: RhaiIdent, relation: &str) -> RhaiExpr {
    RhaiExpr(Identifier::new(format!(
        "{}<-{}",
        id.0.expr().preview(),
        relation
    )).expr())
}

/// Follow outgoing edge to get target: ident->relation.field
pub fn graph_arrow_field(id: RhaiIdent, relation: &str, field: &str) -> RhaiExpr {
    RhaiExpr(Identifier::new(format!(
        "{}->{}.{}",
        id.0.expr().preview(),
        relation,
        field
    )).expr())
}
