//! Named SurrealQL expression primitives.
//!
//! Meaningful, single-purpose building blocks that mirror the vantage-sql
//! primitive vocabulary (`count`, `sum`, `avg`, `coalesce`, `round`,
//! `case_when`, …) and lower to SurrealQL. Each carries the same name as its
//! SQL counterpart where the concept exists, so db-agnostic Rhai scripts keep
//! one vocabulary across backends. Prefer these over the generic `Fx` /
//! `surreal_expr!` escape hatches.

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::identifier::Identifier;
use crate::sum::Fx;
use crate::{AnySurrealType, Expr};

/// `count(expr)` — count truthy / array values. SurrealDB: `count(expr)`.
pub fn count_of(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("count", vec![expr.expr()]).expr()
}

/// `count_distinct(expr)` → `count(array::distinct(expr))`.
pub fn count_distinct(expr: impl Expressive<AnySurrealType>) -> Expr {
    let distinct = Fx::new("array::distinct", vec![expr.expr()]).expr();
    Fx::new("count", vec![distinct]).expr()
}

/// `avg(expr)` → `math::mean(expr)`.
pub fn avg(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("math::mean", vec![expr.expr()]).expr()
}

/// `round(expr)` → `math::round(expr)`. SurrealDB `math::round` is 1-arg.
pub fn round(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("math::round", vec![expr.expr()]).expr()
}

/// `coalesce(a, b)` → `a ?? b` (SurrealDB null-coalescing operator).
pub fn coalesce(a: impl Expressive<AnySurrealType>, b: impl Expressive<AnySurrealType>) -> Expr {
    Expression::new(
        "{} ?? {}",
        vec![
            ExpressiveEnum::Nested(a.expr()),
            ExpressiveEnum::Nested(b.expr()),
        ],
    )
}

/// `nullif(a, b)` → `IF a = b THEN NONE ELSE a END`.
pub fn nullif(a: impl Expressive<AnySurrealType>, b: impl Expressive<AnySurrealType>) -> Expr {
    Expression::new(
        "IF {} = {} THEN NONE ELSE {} END",
        vec![
            ExpressiveEnum::Nested(a.expr()),
            ExpressiveEnum::Nested(b.expr()),
            ExpressiveEnum::Nested(a.expr()),
        ],
    )
}

/// `cast(expr, ty)` → `type::<ty>(expr)` where `ty` is one of
/// `int | float | string | decimal | datetime | number | bool`.
pub fn cast(expr: impl Expressive<AnySurrealType>, ty: &str) -> Expr {
    Fx::new(format!("type::{ty}"), vec![expr.expr()]).expr()
}

/// `date_format(expr, fmt)` → `time::format(expr, "fmt")`.
pub fn date_format(expr: impl Expressive<AnySurrealType>, fmt: &str) -> Expr {
    Expression::new(
        "time::format({}, {})",
        vec![
            ExpressiveEnum::Nested(expr.expr()),
            ExpressiveEnum::Scalar(AnySurrealType::from(fmt.to_string())),
        ],
    )
}

// ── Graph traversal ─────────────────────────────────────────────────────
//
// SurrealQL builds a graph path by prefixing each step with an arrow:
// `->placed->order`, `<-reports_to<-employee`. Every segment — edge or node
// table alike — gets the same arrow, so a traversal is just an anchor plus a
// list of arrow-prefixed segments. The anchor is your standpoint: `me()`
// renders empty, so a leading hop starts from the current record; a nested
// `graph_*` result lets paths change direction (`->a->b<-c<-d`) by composition.

/// Current-record marker for a `graph` traversal. Renders to an empty path so
/// a leading hop (`->placed->order`) starts from the current row.
pub fn me() -> Expr {
    Expression::new("", vec![])
}

/// Outgoing traversal: appends `->segment` to `anchor` for each segment, in
/// order. `graph_out(me(), &["placed", "order"])` → `->placed->order`.
pub fn graph_out(anchor: impl Expressive<AnySurrealType>, segments: &[String]) -> Expr {
    graph_walk(anchor.expr(), "->", segments)
}

/// Incoming traversal: appends `<-segment` to `anchor` for each segment, in
/// path order. `graph_in(me(), &["reports_to", "employee"])`
/// → `<-reports_to<-employee`.
pub fn graph_in(anchor: impl Expressive<AnySurrealType>, segments: &[String]) -> Expr {
    graph_walk(anchor.expr(), "<-", segments)
}

fn graph_walk(anchor: Expr, arrow: &str, segments: &[String]) -> Expr {
    let template = format!("{{}}{arrow}{{}}");
    segments.iter().fold(anchor, |path, seg| {
        Expression::new(
            template.clone(),
            vec![
                ExpressiveEnum::Nested(path),
                ExpressiveEnum::Nested(Identifier::new(seg.as_str()).expr()),
            ],
        )
    })
}

/// Field access after a traversal or expression: `{expr}.{name}`. The Rhai
/// engine exposes this as the `[...]` indexer (`graph(…)["name"]`).
pub fn field(expr: impl Expressive<AnySurrealType>, name: &str) -> Expr {
    Expression::new(
        "{}.{}",
        vec![
            ExpressiveEnum::Nested(expr.expr()),
            ExpressiveEnum::Nested(Identifier::new(name).expr()),
        ],
    )
}

/// Numeric element access after a path or subquery: `{expr}[n]`. The Rhai
/// engine exposes this as the integer `[...]` indexer (`subquery[0]`), the
/// sibling of the string `["field"]` indexer (`field`). The index is inlined
/// into the template, so it always renders as a bare integer literal.
pub fn index_at(expr: impl Expressive<AnySurrealType>, n: i64) -> Expr {
    Expression::new(
        format!("{{}}[{n}]"),
        vec![ExpressiveEnum::Nested(expr.expr())],
    )
}

/// Parenthesize an expression so a `SELECT …` can be used as a scalar
/// subquery: `(SELECT …)`. The faithful analogue of SurrealQL's parentheses —
/// the result composes with the `[n]` indexer, `.alias()`, comparisons, and
/// `from()` exactly like any other expression. The Rhai engine exposes this as
/// the `.subquery()` method on the select builder.
pub fn subquery(inner: impl Expressive<AnySurrealType>) -> Expr {
    Expression::new("({})", vec![ExpressiveEnum::Nested(inner.expr())])
}

// ── Tier 2: surreal-specific scalar/collection functions ────────────────────
// Each mirrors a `math::`/`array::`/`object::`/`string::`/`time::` function under
// a single-purpose name. The plain ones are `Fx` one-liners (like `avg`/`round`);
// `time_group`/`similarity` inline a fixed config/search token single-quoted.

/// `first(expr)` → `array::first(expr)`.
pub fn first(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("array::first", vec![expr.expr()]).expr()
}

/// `len(expr)` → `array::len(expr)`.
pub fn len(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("array::len", vec![expr.expr()]).expr()
}

/// `stddev(expr)` → `math::stddev(expr)`.
pub fn stddev(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("math::stddev", vec![expr.expr()]).expr()
}

/// `median(expr)` → `math::median(expr)`.
pub fn median(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("math::median", vec![expr.expr()]).expr()
}

/// `lower(expr)` → `string::lowercase(expr)`.
pub fn lower(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("string::lowercase", vec![expr.expr()]).expr()
}

/// `words(expr)` → `string::words(expr)`.
pub fn words(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("string::words", vec![expr.expr()]).expr()
}

/// `object_entries(expr)` → `object::entries(expr)`.
pub fn object_entries(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("object::entries", vec![expr.expr()]).expr()
}

/// `object_values(expr)` → `object::values(expr)`.
pub fn object_values(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("object::values", vec![expr.expr()]).expr()
}

/// `time_group(expr, unit)` → `time::group(expr, 'unit')`. `unit` is a fixed
/// bucket token (`'year'`/`'month'`/`'day'`/…), inlined single-quoted to match
/// SurrealQL's literal form.
pub fn time_group(expr: impl Expressive<AnySurrealType>, unit: &str) -> Expr {
    Expression::new(
        format!("time::group({{}}, '{unit}')"),
        vec![ExpressiveEnum::Nested(expr.expr())],
    )
}

/// `similarity(expr, term)` → `string::similarity::jaro_winkler(expr, 'term')`.
/// `term` is the literal search string, inlined single-quoted.
pub fn similarity(expr: impl Expressive<AnySurrealType>, term: &str) -> Expr {
    Expression::new(
        format!("string::similarity::jaro_winkler({{}}, '{term}')"),
        vec![ExpressiveEnum::Nested(expr.expr())],
    )
}

/// Ranged graph recursion: `@.{min..max}(path)`. `path` is a traversal built
/// with `graph_out`/`graph_in`; wrap the result with `field` for the trailing
/// projection (`@.{1..5}(<-reports_to<-employee).name`).
pub fn recurse(path: impl Expressive<AnySurrealType>, min: i64, max: i64) -> Expr {
    Expression::new(
        format!("@.{{{min}..{max}}}({{}})"),
        vec![ExpressiveEnum::Nested(path.expr())],
    )
}

/// SurrealQL conditional, the SurrealDB rendering of the shared `case_when`
/// primitive. Renders as `IF c1 THEN v1 ELSE IF c2 THEN v2 ELSE e END`
/// (SurrealQL uses a single trailing `END`, not one per branch).
#[derive(Debug, Clone, Default)]
pub struct Case {
    branches: Vec<(Expr, Expr)>,
    otherwise: Option<Expr>,
}

impl Case {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn when(
        mut self,
        cond: impl Expressive<AnySurrealType>,
        then: impl Expressive<AnySurrealType>,
    ) -> Self {
        self.branches.push((cond.expr(), then.expr()));
        self
    }

    pub fn else_(mut self, value: impl Expressive<AnySurrealType>) -> Self {
        self.otherwise = Some(value.expr());
        self
    }
}

impl Expressive<AnySurrealType> for Case {
    fn expr(&self) -> Expr {
        let mut template = String::new();
        let mut params: Vec<ExpressiveEnum<AnySurrealType>> = Vec::new();
        for (i, (cond, then)) in self.branches.iter().enumerate() {
            template.push_str(if i == 0 {
                "IF {} THEN {}"
            } else {
                " ELSE IF {} THEN {}"
            });
            params.push(ExpressiveEnum::Nested(cond.clone()));
            params.push(ExpressiveEnum::Nested(then.clone()));
        }
        if let Some(other) = &self.otherwise {
            template.push_str(" ELSE {}");
            params.push(ExpressiveEnum::Nested(other.clone()));
        }
        template.push_str(" END");
        Expression::new(template, params)
    }
}

impl From<Case> for Expr {
    fn from(c: Case) -> Self {
        c.expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identifier::Identifier;
    use crate::surreal_expr;

    #[test]
    fn aggregates_lower_to_surreal() {
        assert_eq!(
            count_of(surreal_expr!("->placed->order")).preview(),
            "count(->placed->order)"
        );
        assert_eq!(
            avg(Identifier::new("salary")).preview(),
            "math::mean(salary)"
        );
        assert_eq!(
            round(avg(Identifier::new("total"))).preview(),
            "math::round(math::mean(total))"
        );
        assert_eq!(
            count_distinct(Identifier::new("id")).preview(),
            "count(array::distinct(id))"
        );
    }

    #[test]
    fn coalesce_and_nullif() {
        assert_eq!(
            coalesce(surreal_expr!("array::first(x)"), "n/a".to_string()).preview(),
            r#"array::first(x) ?? "n/a""#
        );
        assert_eq!(
            nullif(Identifier::new("qty"), 0i64).preview(),
            "IF qty = 0 THEN NONE ELSE qty END"
        );
    }

    #[test]
    fn cast_and_date_format() {
        assert_eq!(cast(Identifier::new("x"), "int").preview(), "type::int(x)");
        assert_eq!(
            date_format(Identifier::new("created_at"), "%Y-%m").preview(),
            r#"time::format(created_at, "%Y-%m")"#
        );
    }

    fn segs(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn graph_traversal_lowers_to_arrow_paths() {
        // leading outgoing / incoming from the current record
        assert_eq!(
            graph_out(me(), &segs(&["reports_to", "employee"])).preview(),
            "->reports_to->employee"
        );
        assert_eq!(
            graph_in(me(), &segs(&["reports_to", "employee"])).preview(),
            "<-reports_to<-employee"
        );
        // edge-only (single segment)
        assert_eq!(
            graph_out(me(), &segs(&["reviewed"])).preview(),
            "->reviewed"
        );
        // field tail
        assert_eq!(
            field(graph_out(me(), &segs(&["reports_to", "employee"])), "name").preview(),
            "->reports_to->employee.name"
        );
    }

    #[test]
    fn nesting_yields_mixed_direction() {
        // "clients who placed the same order as me":
        // anchor on the right of the outer call → the appended hop reverses.
        let inner = graph_out(me(), &segs(&["placed", "order"]));
        assert_eq!(
            graph_in(inner, &segs(&["placed", "client"])).preview(),
            "->placed->order<-placed<-client"
        );
    }

    #[test]
    fn numeric_index_appends_brackets() {
        // element access on a subquery / fanned-out path
        assert_eq!(
            index_at(surreal_expr!("(SELECT VALUE x FROM y GROUP ALL)"), 0).preview(),
            "(SELECT VALUE x FROM y GROUP ALL)[0]"
        );
        assert_eq!(
            index_at(graph_out(me(), &segs(&["placed", "order"])), 0).preview(),
            "->placed->order[0]"
        );
    }

    #[test]
    fn recursion_wraps_a_path() {
        let path = graph_in(me(), &segs(&["reports_to", "employee"]));
        assert_eq!(
            field(recurse(path, 1, 5), "name").preview(),
            "@.{1..5}(<-reports_to<-employee).name"
        );
    }

    #[test]
    fn case_renders_if_then_else() {
        let c = Case::new()
            .when(surreal_expr!("price >= 250"), "premium".to_string())
            .when(surreal_expr!("price >= 150"), "mid".to_string())
            .else_("value".to_string());
        assert_eq!(
            c.preview(),
            r#"IF price >= 250 THEN "premium" ELSE IF price >= 150 THEN "mid" ELSE "value" END"#
        );
    }

    #[test]
    fn tier2_fns_lower_to_surreal() {
        let f = Identifier::new("salary");
        assert_eq!(first(Identifier::new("x")).preview(), "array::first(x)");
        assert_eq!(len(Identifier::new("lines")).preview(), "array::len(lines)");
        assert_eq!(stddev(f.clone()).preview(), "math::stddev(salary)");
        assert_eq!(median(f).preview(), "math::median(salary)");
        assert_eq!(lower(Identifier::new("name")).preview(), "string::lowercase(name)");
        assert_eq!(words(Identifier::new("name")).preview(), "string::words(name)");
        assert_eq!(
            object_entries(Identifier::new("nutrition")).preview(),
            "object::entries(nutrition)"
        );
        assert_eq!(
            object_values(Identifier::new("nutrition")).preview(),
            "object::values(nutrition)"
        );
    }

    #[test]
    fn tier2_literal_tokens_are_single_quoted() {
        // time unit and search term are inlined single-quoted to match SurrealQL.
        assert_eq!(
            time_group(Identifier::new("created_at"), "month").preview(),
            "time::group(created_at, 'month')"
        );
        assert_eq!(
            similarity(lower(Identifier::new("name")), "marti mcfligh").preview(),
            "string::similarity::jaro_winkler(string::lowercase(name), 'marti mcfligh')"
        );
    }
}
