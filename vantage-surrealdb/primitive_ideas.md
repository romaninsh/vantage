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

Flat multi-hop arity now reaches **7** (`fn_graph2`..`fn_graph7`), covering an anchor + 6 segments —
add further overloads only if a real path needs them. Still deferred: the `(b)` per-edge glyph
override (mixed direction is already handled by nesting, so this is only worth it for an
unexpressible case like bidirectional `<->`).

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

## Tier 3 — embedded-array closures — **IMPLEMENTED** (native Rhai closures)

The one place SurrealDB exceeds the SQL vocabulary entirely (`lines.map(|$l| {…})`,
`lines.fold(0, |$acc,$l| …)`). Resolved **not** with a `closure([params], body)` data-constructor but
by running SurrealDB's own closure syntax — Rhai's native `|l| …` — **symbolically**:

- **`.map(|l| …)` / `.fold(init, |acc, l| …)` / `.filter(|l| …)`** are registered on `Ex` (via a
  `Dynamic` receiver, so `ident("lines")` works too). Each binds the closure's parameters to
  placeholder `$name` expressions (`closure_param` → `Variable`) and calls the native closure with
  them (`FnPtr::call_within_context`). Because every operator/indexer on an `Ex` *builds* an
  expression rather than computing, the returned value **is** the SurrealQL body. Rust:
  `primitives::array_map`/`array_fold`/`array_filter` render only the `.method(|$p| body)` shell.
- **`#{ k: v }`** (native Rhai map) → object literal `{ k: v, … }`, and **`[…]`** (native Rhai array)
  → `[ … ]`, both handled by extending `to_expr` (Rhai's `Map` is a `BTreeMap`, so keys render
  **sorted** — deterministic). Rust: `primitives::object_literal`/`array_literal`. No separate
  `object()`/`array_lit()` constructors needed.
- **Arithmetic operators `* + - /`** are registered on `Ex` (only combos involving an `Ex`, so native
  numeric maths is untouched); each renders **parenthesized** — `({} * {})`. Rust:
  `operators::arith`.
- Field access in a body uses the existing string indexer: `l["product"]["name"]` → `$value.product.name`
  (Rhai doesn't route `.field` through a custom indexer, so the `["…"]` idiom stands).

**Two accepted consequences** (both cosmetic, both flow into the regenerated Q5 golden):
1. **The emitted `$name` is engine-chosen, not the script's** — Rhai locals can't carry a `$`, so we
   never see the user's `|l|`; the placeholders are `$value` (map/filter item), `$acc`/`$value`
   (fold). Output reads `|$value| …`, not `|$l| …`.
2. **Operator parens** — `$value.quantity * $value.price` renders `($value.quantity * $value.price)`
   and `$acc + …` renders `($acc + …)`. Semantically identical; the v4_q05 golden was regenerated to
   the parenthesized form and **executes byte-for-byte-equivalently against v4** (verified: same rows,
   same `computed_total`).

## Open decisions

1. **Q5 closures:** ✅ resolved — native Rhai `|l| …` closures run symbolically (see Tier 3 above),
   not a `closure(...)` data-constructor. `#{}`/`[]` lower natively; `* + - /` registered on `Ex`.
2. **`round` arity:** ✅ resolved — 2-arg `round(x, n)` → `math::fixed(x, n)` (SurrealDB's
   round-to-N-decimals builtin; `math::round` stays the 1-arg integer form). No scale-and-divide
   needed. `primitives::round_to` + a second registered `round` overload.
3. **`avg` vs `mean`:** Tier 1 uses `avg` (SQL parity) → `math::mean`. Add `mean` as a surreal-only
   alias? — still open (low priority).
4. **Formalize the vocabulary?** ✅ done — `docs4/src/surrealdb/primitives.md` (with a
   `surrealdb.md` landing page, wired into `SUMMARY.md`) documents the canonical names + SurrealQL
   lowerings as a Rhai-facing reference.

## Per-query stub → future-primitive map

- **Q1** ✅ **fully de-stubbed** (no `expr()`): `coalesce(first(graph(me, "reports_to", "employee")["name"]), "n/a")`,
  and `ident("department")["name"].alias("department")` for the dotted column path.
- **Q2** ✅ graph + recursion done: `recurse(graph("employee", "reports_to", me), 1, 5)["name"]`,
  `count(graph("reports_to", me))`.
- **Q3** ✅ **fully de-stubbed** (no `expr()`): `round(stddev(ident("salary")))`,
  `ident("department")["name"].alias("department")`; the `order_by` references the `payroll` output
  alias as `ident("payroll")`.
- **Q4** ✅ **fully de-stubbed** (no `expr()`): `count(graph(me, "placed", "order"))`,
  `sum(graph(me, "placed", "order")["total"])`, and the correlated subquery as
  `select().value().expression(max(ident("total"))).from("order").where(ident("client") == parent("id")).group_all().subquery()[0].alias("biggest_order")`.
- **Q5** ✅ **fully de-stubbed** (no `expr()`): `len(ident("lines"))`, and native closures
  `ident("lines").map(|l| #{ product: l["product"]["name"], subtotal: l["quantity"] * l["price"] })`
  + `ident("lines").fold(0, |acc, l| acc + l["quantity"] * l["price"])`. Emits engine-chosen
  `$value`/`$acc`; golden regenerated to the parenthesized form (executes equivalently against v4).
- **Q6** ✅ graph done: `count(graph("reviewed", me))`, `graph("reviewed", me)["rating"]`.
- **Q7** ✅ **fully de-stubbed** (no `expr()`): `time_group(ident("created_at"), "month")`.
- **Q8** ✅ **fully de-stubbed** (no `expr()`): `similarity(lower(ident("name")), "marti mcfligh")`,
  `words(ident("name"))`; the `order_by` references the `similarity` output alias as `ident("similarity")`.
- **Q9** ✅ **fully de-stubbed** (no `expr()`): `object_entries(ident("nutrition"))`,
  `sum(object_values(ident("nutrition")))`, and `ident("nutrition")["sugar"]` (in the projection +
  `WHERE`); the `order_by` references the `sugar` output alias as `ident("sugar")`.

### Column / field paths — **idiom settled** (`ident("t")["col"]`)
Dotted column paths use the string indexer: `ident("department")["name"]` → `department.name`
(`Identifier::needs_escaping` ignores `.`). Aliasing a name/path now projects correctly —
`ident(...)["col"].alias("x")` → `col_path AS x`: the Rhai `Id.alias` was changed to **lift the ident
into the expression layer** (`ident_as_alias` → `{ident} AS {alias}`) instead of silently renaming, so
it composes like `.alias()` on any `Ex`. Output-alias references in `order_by`/`group_by` (e.g.
`payroll`, `sugar`, `avg_rating`, `lifetime_spend`) now use `ident("…")` — they name a projection
alias, but `ident` renders them identically and reads truer than `expr("…")`.
- **Q10** ✅ **fully de-stubbed**: `from(select().expression(ident("tags").alias("tag")).field("price").from("product").split("tags").subquery())`;
  the `tags AS tag` projection and the `product_count` order-by both use `ident(...)`.

**The whole v4 suite (Q1–Q10) is now free of `expr()` constructor stubs** — every query renders
entirely from named primitives. (Q10 still calls `case_when()…​.expr()`, but that `.expr()` is the
`Case` terminator, not the raw-SurrealQL escape hatch.)
