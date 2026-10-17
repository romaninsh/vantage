//! Named SurrealQL expression primitives: single-purpose builders that reuse
//! the vantage-sql name where the concept exists (`count`, `avg`, `coalesce`,
//! `case_when`, …) and lower to SurrealQL. Prefer them over `Fx` and
//! `surreal_expr!`.

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

/// `round(expr)` → `math::round(expr)`. SurrealDB `math::round` is 1-arg
/// (rounds to the nearest integer).
pub fn round(expr: impl Expressive<AnySurrealType>) -> Expr {
    Fx::new("math::round", vec![expr.expr()]).expr()
}

/// `round(expr, places)` → `math::fixed(expr, places)`. SurrealDB's `math::round`
/// has no decimal-places arg; `math::fixed` is the builtin that rounds to N
/// decimals (e.g. `math::fixed(3.146, 2)` → `3.15`). `places` is inlined as a
/// bare integer literal.
pub fn round_to(expr: impl Expressive<AnySurrealType>, places: i64) -> Expr {
    Expression::new(
        format!("math::fixed({{}}, {places})"),
        vec![ExpressiveEnum::Nested(expr.expr())],
    )
}

/// `coalesce(a, b)` → `(a ?? b)`. Parenthesised: `??` binds looser than
/// `=`, so a bare `a ?? b = 0` would read as `a ?? (b = 0)`.
pub fn coalesce(a: impl Expressive<AnySurrealType>, b: impl Expressive<AnySurrealType>) -> Expr {
    Expression::new(
        "({} ?? {})",
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
// Every segment, edge or node table, takes the same arrow, so a path is an
// anchor plus arrow-prefixed segments. Nesting changes direction.

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

// ── SurrealDB scalar and collection functions ───────────────────────────

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

/// `time_group(expr, unit)` → `time::group(expr, $unit)`. `unit` is the bucket
/// token (`year`/`month`/`day`/…), bound as a parameter so it cannot break out of
/// the SurrealQL string literal.
pub fn time_group(expr: impl Expressive<AnySurrealType>, unit: &str) -> Expr {
    Expression::new(
        "time::group({}, {})",
        vec![
            ExpressiveEnum::Nested(expr.expr()),
            ExpressiveEnum::Scalar(AnySurrealType::from(unit.to_string())),
        ],
    )
}

/// `similarity(expr, term)` → `string::similarity::jaro_winkler(expr, $term)`.
/// `term` is the literal search string, bound as a parameter so an embedded quote
/// cannot escape the literal and inject SurrealQL.
pub fn similarity(expr: impl Expressive<AnySurrealType>, term: &str) -> Expr {
    Expression::new(
        "string::similarity::jaro_winkler({}, {})",
        vec![
            ExpressiveEnum::Nested(expr.expr()),
            ExpressiveEnum::Scalar(AnySurrealType::from(term.to_string())),
        ],
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

// ── Embedded-array closures ─────────────────────────────────────────────
//
// The Rhai engine runs a native closure symbolically over `$name`
// placeholders; these helpers render only the `.method(|$params| body)` shell
// and the `{…}` / `[…]` literals a body produces.

/// A closure parameter placeholder: `closure_param("value")` → `$value`. Bound
/// to the closure's argument so the body renders against it.
pub fn closure_param(name: &str) -> Expr {
    crate::variable::Variable::new(name).expr()
}

/// Render a closure parameter header: `["acc", "value"]` → `|$acc, $value|`.
fn closure_header(params: &[&str]) -> String {
    let names = params
        .iter()
        .map(|p| format!("${p}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("|{names}|")
}

/// `expr.map(|$p| body)` → `{expr}.map(|$p| {body})`.
pub fn array_map(
    this: impl Expressive<AnySurrealType>,
    params: &[&str],
    body: impl Expressive<AnySurrealType>,
) -> Expr {
    let header = closure_header(params);
    Expression::new(
        format!("{{}}.map({header} {{}})"),
        vec![
            ExpressiveEnum::Nested(this.expr()),
            ExpressiveEnum::Nested(body.expr()),
        ],
    )
}

/// `expr.fold(init, |$acc, $p| body)` → `{expr}.fold({init}, |$acc, $p| {body})`.
pub fn array_fold(
    this: impl Expressive<AnySurrealType>,
    init: impl Expressive<AnySurrealType>,
    params: &[&str],
    body: impl Expressive<AnySurrealType>,
) -> Expr {
    let header = closure_header(params);
    Expression::new(
        format!("{{}}.fold({{}}, {header} {{}})"),
        vec![
            ExpressiveEnum::Nested(this.expr()),
            ExpressiveEnum::Nested(init.expr()),
            ExpressiveEnum::Nested(body.expr()),
        ],
    )
}

/// `expr.filter(|$p| body)` → `{expr}.filter(|$p| {body})`.
pub fn array_filter(
    this: impl Expressive<AnySurrealType>,
    params: &[&str],
    body: impl Expressive<AnySurrealType>,
) -> Expr {
    let header = closure_header(params);
    Expression::new(
        format!("{{}}.filter({header} {{}})"),
        vec![
            ExpressiveEnum::Nested(this.expr()),
            ExpressiveEnum::Nested(body.expr()),
        ],
    )
}

/// Object literal `{ k1: v1, k2: v2 }` — the lowering of a Rhai `#{…}` map.
/// Keys are inlined verbatim (they arrive sorted from Rhai's map); values are
/// nested expressions.
pub fn object_literal(entries: Vec<(String, Expr)>) -> Expr {
    let mut template = String::from("{ ");
    let mut params = Vec::with_capacity(entries.len());
    for (i, (key, value)) in entries.into_iter().enumerate() {
        if i > 0 {
            template.push_str(", ");
        }
        template.push_str(&key);
        template.push_str(": {}");
        params.push(ExpressiveEnum::Nested(value));
    }
    template.push_str(" }");
    Expression::new(template, params)
}

/// Array literal `[a, b, c]` — the lowering of a Rhai `[…]` array.
pub fn array_literal(items: Vec<Expr>) -> Expr {
    let placeholders = vec!["{}"; items.len()].join(", ");
    Expression::new(
        format!("[{placeholders}]"),
        items.into_iter().map(ExpressiveEnum::Nested).collect(),
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
