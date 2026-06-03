# SurrealDB primitive ideas (deferred)

Tier 1 (the shared vocabulary that already overlaps with vantage-sql) is **implemented** in
`src/primitives.rs` + registered in `src/rhai_engine/`. This file holds the **deferred** Tier 2 /
Tier 3 primitives and open decisions. They are SurrealDB-specific (no plain-SQL analogue) and will be
implemented one at a time. Until then the 10-query test-suite
(`tests/rhai-tests/v4_q01..v4_q10.rhai`) carries them as raw `expr("…")` stubs.

Naming principle (unchanged): one meaningful, single-purpose primitive per concept; reuse the
vantage-sql name where the concept exists; lower to SurrealQL. `Fx`/`expr` stay as escape hatches.

## Tier 2 — surreal-specific named primitives

### Stats / collection functions — **IMPLEMENTED** (mirror `math::`/`array::`/`object::`/`string::`)
| Primitive (Rhai + Rust) | SurrealQL lowering | De-stubbed |
|---|---|---|
| `stddev(expr)` | `math::stddev(expr)` | Q3 ✅ |
| `median(expr)` | `math::median(expr)` | — (parity) |
| `first(expr)` | `array::first(expr)` | Q1 ✅ |
| `len(expr)` | `array::len(expr)` | Q5 (closures pending) |
| `object_entries(expr)` | `object::entries(expr)` | Q9 ✅ |
| `object_values(expr)` | `object::values(expr)` | Q9 ✅ |
| `lower(expr)` | `string::lowercase(expr)` | Q8 ✅ |
| `words(expr)` | `string::words(expr)` | Q8 ✅ |
| `similarity(expr, term)` | `string::similarity::jaro_winkler(expr, 'term')` | Q8 ✅ |
| `time_group(expr, unit)` | `time::group(expr, 'unit')` | Q7 ✅ |

All in `primitives.rs` (named fns) + `rhai_engine/constructors.rs` (`fn_*` wrappers) + registered in
`mod.rs`. The plain ones are `Fx::new("…", …)` one-liners like `avg`/`round`. **`time_group` and
`similarity` inline their literal token single-quoted** (`'month'`, `'marti mcfligh'`) — scalar strings
otherwise render double-quoted (cf. `coalesce(…, "n/a")` → `?? "n/a"`), and the v4 goldens authored
those two config/search tokens single-quoted, so inlining is required for byte-exact output. `len` is
wired but its only stub (Q5) is gated behind the closures work.

### Graph idioms — **IMPLEMENTED** (`graph()` / `recurse()` / `me` / `[...]`)

Resolved by one positional primitive instead of a `out`/`in_`/`up`/`down` direction vocabulary
(every directional name either assumes a hierarchy or names the arrow rather than the meaning —
see the design discussion). Final design, in `src/primitives.rs` + `src/rhai_engine/`:

- **`graph(me, "edge", "table", …)`** — exactly one argument is the *anchor* (`me`, the
  current-record marker, or a nested `graph(…)`); the rest are edge/table names. The anchor's
  **position** sets direction: anchor on the **left** walks outward (`->edge->table`), anchor on the
  **right** walks inward (`table<-edge<-…`). Edge-only is just arity-1 (`graph(me, "reviewed")` →
  `->reviewed`; `graph("reviewed", me)` → `<-reviewed`). Rust: `graph_out`/`graph_in(anchor, &[seg])`.
- **Mixed direction by nesting** — each `graph()` appends one directed hop, so
  `graph("client", "placed", graph(me, "placed", "order"))` → `->placed->order<-placed<-client`.
  No glyph overrides; composition carries it. (Flat multi-arg = same-direction sugar.)
- **`me`** — bare constant (via `engine.on_var`) rendering to an empty path, so a leading hop starts
  from the current row. Rust: `primitives::me()`.
- **Field tail `["field"]`** — the `RhaiExpr` indexer (`register_indexer_get`), reusing the existing
  `ident["col"]` idiom. `graph(me, "reports_to", "employee")["name"]` → `…employee.name`. Rust:
  `primitives::field(expr, name)`.
- **`recurse(path, min, max)`** → `@.{min..max}(path)`, wrapping a `graph()` path. Q2:
  `recurse(graph("employee", "reports_to", me), 1, 5)["name"]`.

Used live in Q1, Q2, Q4, Q6 (render byte-identical to the v4 goldens, execute against `v4`).

- **Numeric index `(expr)[n]`** — the *integer* `RhaiExpr` indexer (`register_indexer_get` on `i64`),
  sibling of the string `["field"]` indexer. Rhai dispatches by argument type, so `["rating"]` →
  `.rating` and `[0]` → `[0]` coexist. Mirrors SurrealQL 1:1. Rust: `primitives::index_at(expr, n)`.
  Used live in Q4's `(SELECT … GROUP ALL)[0]`.

Still deferred here:
- **Flat multi-hop arity > 5** and the `(b)` per-edge glyph override — only if real use needs them.

### Select-builder clauses (surreal-only)
- **`group_all()`** → `GROUP ALL` — **IMPLEMENTED**. New `group_all: bool` on `SurrealSelect`,
  `with_group_all()` builder, rendered by `render_group()` (replaces `render_group_by`); mutually
  exclusive with `group_by` and wins when both are set. Rhai method `group_all`.
- **`subquery()`** → `(SELECT …)` — **IMPLEMENTED**. `primitives::subquery(expr)` wraps any
  expression in parens (the faithful analogue of SurrealQL's parentheses); Rhai exposes it as the
  `.subquery()` method bridging `RhaiSelect → RhaiExpr`. The result composes with the `[n]` indexer,
  `.alias()`, comparisons, and `from()`. This fully de-stubs **Q4** (see per-query map) and is the
  bridge Q10 will reuse for `from(select().split("tags").subquery())`.
- **`split(field)`** → `SPLIT field` — **IMPLEMENTED**. New `split: Vec<Expr>` on `SurrealSelect`,
  `with_split(field)` builder, `render_split()` slotted in the correct clause order
  (`… WHERE → SPLIT → GROUP → ORDER BY → LIMIT`). Rhai method `split` (`&str` and ident overloads).
  De-stubs **Q10**'s inner subquery via `select().split("tags").subquery()` as the FROM source.
- (`value()`/`only()` already exist; subquery-as-source already works via `from(<select>)`.)

### Parameters — **IMPLEMENTED** (`param()`)
- **`param(name)`** → `$name` — any SurrealDB `$`-parameter (`$parent`, `$this`, `$value`, …) or
  `LET`-bound name. "Parameter" is SurrealDB's own term for `$`-prefixed names, so the name is exact,
  not a workaround. **Named `param`, not `var`, because Rhai reserves `var` as a keyword** (`var(…)` is
  a parse error). Field tail via the string indexer: `param("parent")["id"]` → `$parent.id`. Backed by
  the `Variable` type in `src/variable.rs` (retargeted to surreal `Expr` + `Expressive`, module enabled
  — it was dead before: module commented out in `lib.rs`, `From` lowering to the wrong
  `serde_json::Value` generic). Proven end-to-end by the `v4_param` golden.
- **`parent()` / `parent("field")`** → `$parent` / `$parent.field` — sugar over `param`/`Variable`.
  This is what Q4 uses for its correlation (`= parent("id")`).
- Note `me` (graph anchor, renders empty) is **not** `$this` — distinct.
- Ready for Q5 closure params: `param("acc")`/`param("l")` → `$acc`/`$l`.

## Tier 3 — embedded-array closures (Q5 only)

The one place SurrealDB exceeds the SQL vocabulary entirely (`lines.map(|$l| {…})`,
`lines.fold(0, |$acc,$l| …)`). Proposed meaningful named primitives (still not a general AST):
- `closure([params], body)` → `|$p1, $p2| body` (params via `var`).
- `.map(closure)` / `.fold(init, closure)` / `.filter(closure)` methods on an `Expr`.
- `object([[k, v], …])` → `{ k: v, … }` literal; `array_lit([…])` → `[ … ]`.

Decision pending: implement these four, or keep Q5's per-line `breakdown` as a raw `expr()` and only
express `computed_total` via the shared `sum(mul(...))` over a subquery. Recommendation: implement —
each is a clear single-job primitive.

## Open decisions

1. **Q5 closures:** add `closure`/`map`/`fold`/`object`/`array_lit` (recommended) vs leave as `expr()`.
2. **`round` arity:** SQL `round(x, n)` vs SurrealQL `math::round(x)` (1-arg). Tier 1 implemented the
   1-arg form; a 2-arg overload would need scale-and-divide. Confirm desired behaviour.
3. **`avg` vs `mean`:** Tier 1 uses `avg` (SQL parity) → `math::mean`. Add `mean` as a surreal-only
   alias? 
4. **Formalize the vocabulary?** Consider a `docs4/src/…/surreal-primitives.md` mirroring
   `docs4/src/sql/primitives.md`, listing canonical names + SurrealQL lowerings, so the SQL↔Surreal
   overlap is documented rather than convention-only.

## Per-query stub → future-primitive map

- **Q1** ✅ **fully de-stubbed** (no `expr()`): `coalesce(first(graph(me, "reports_to", "employee")["name"]), "n/a")`,
  and `ident("department")["name"].alias("department")` for the dotted column path.
- **Q2** ✅ graph + recursion done: `recurse(graph("employee", "reports_to", me), 1, 5)["name"]`,
  `count(graph("reports_to", me))`.
- **Q3** ✅ `stddev` + column done: `round(stddev(ident("salary")))`,
  `ident("department")["name"].alias("department")`. Remaining: only `expr("payroll")` (an output-alias
  reference in `order_by`, not a column).
- **Q4** ✅ **fully de-stubbed** (no `expr()`): `count(graph(me, "placed", "order"))`,
  `sum(graph(me, "placed", "order")["total"])`, and the correlated subquery as
  `select().value().expression(max(ident("total"))).from("order").where(ident("client") == parent("id")).group_all().subquery()[0].alias("biggest_order")`.
- **Q5** all `expr(...)` → Tier 3 closures/`object`/`map`/`fold`/`len`.
- **Q6** ✅ graph done: `count(graph("reviewed", me))`, `graph("reviewed", me)["rating"]`.
- **Q7** ✅ **fully de-stubbed** (no `expr()`): `time_group(ident("created_at"), "month")`.
- **Q8** ✅ `similarity`/`lower`/`words` done: `similarity(lower(ident("name")), "marti mcfligh")`,
  `words(ident("name"))`. Remaining `expr()`: only the `similarity` order-by alias reference.
- **Q9** ✅ `object_entries`/`object_values` + column done: `object_entries(ident("nutrition"))`,
  `sum(object_values(ident("nutrition")))`, and `ident("nutrition")["sugar"]` (in the projection +
  `WHERE`). Remaining: only `expr("sugar")` (an output-alias reference in `order_by`).

### Column / field paths — **idiom settled** (`ident("t")["col"]`)
Dotted column paths use the string indexer: `ident("department")["name"]` → `department.name`
(`Identifier::needs_escaping` ignores `.`). Aliasing a name/path now projects correctly —
`ident(...)["col"].alias("x")` → `col_path AS x`: the Rhai `Id.alias` was changed to **lift the ident
into the expression layer** (`ident_as_alias` → `{ident} AS {alias}`) instead of silently renaming, so
it composes like `.alias()` on any `Ex`. Output-alias references in `order_by`/`group_by` (e.g.
`payroll`, `sugar`) are left as `expr("…")`/`ident("…")` — those name a projection alias, not a column.
- **Q10** ✅ SPLIT subquery done: `from(select().expression(expr("tags").alias("tag")).field("price").from("product").split("tags").subquery())`.
  Remaining `expr()`: only the bare `tags AS tag` projection, via the standard `expr("col").alias(...)`
  idiom (same as Q2/Q3) — not a structural stub.
