# SurrealDB Primitives

Primitives are the named building blocks for SurrealQL expressions. Each one does a single job and
lowers to the SurrealQL it's named for — you call `surreal::coalesce(a, b)` and get `a ?? b`, not a
string you had to escape yourself.

The guiding rule is **one meaningful name per concept, reusing the `vantage-sql` name wherever the
concept already exists.** A script that says `count()`, `avg()`, `round()`, `coalesce()`,
`case_when()`, `date_format()` reads the same against SQLite, Postgres, or SurrealDB; only the
lowering differs (`avg` → `AVG` on SQL, `math::mean` on SurrealDB). Where SurrealDB has no SQL
analogue — graph traversals, embedded-array closures, `SPLIT` — the primitive carries SurrealDB's own
term.

The surface is the Rhai engine built by `register_surreal_engine!`. The examples below are Rhai; the
`// →` comment shows what each renders to.

```admonish note title="The surreal:: namespace"
Every free function here, and the `me` anchor, lives in the `surreal::` namespace:
`surreal::count()`, `surreal::ident("name")`, `surreal::me`. The SQL builders use `sql::` for the
same names, so a script names its backend once per call. Methods (`.alias`, `.when`, `.map`,
`.from`, …), the `[…]` indexer and the operators are called on their receiver without a prefix.
The tables below leave the prefix out.
```

```admonish note title="Escape hatches"
`surreal::expr("…")` and `surreal::fx("name", […])` drop to raw SurrealQL when no primitive fits —
`surreal::expr("tags")`, `surreal::fx("crypto::md5", [surreal::ident("email")])`. Prefer a named
primitive; reach for these only at the edges.
```

## The shared vocabulary

Same names as `vantage-sql`, lowered to SurrealQL:

| Primitive | SurrealQL | Notes |
|---|---|---|
| `count()` | `count()` | zero-arg row count |
| `count(expr)` | `count(expr)` | count truthy / array values |
| `count_distinct(expr)` | `count(array::distinct(expr))` | |
| `sum(expr)` | `math::sum(expr)` | |
| `avg(expr)` | `math::mean(expr)` | SQL name, surreal lowering |
| `min(expr)` / `max(expr)` | `math::min` / `math::max` | |
| `round(expr)` | `math::round(expr)` | nearest integer |
| `round(expr, n)` | `math::fixed(expr, n)` | round to `n` decimals — `math::round` has no places arg |
| `coalesce(a, b)` | `a ?? b` | null-coalescing |
| `nullif(a, b)` | `IF a = b THEN NONE ELSE a END` | |
| `cast(expr, "int")` | `type::int(expr)` | also `float`/`string`/`decimal`/`datetime`/`number`/`bool` |
| `date_format(expr, fmt)` | `time::format(expr, "fmt")` | |

```rhai
surreal::coalesce(surreal::ident("nickname"), "anonymous")   // → nickname ?? "anonymous"
surreal::round(surreal::avg(surreal::ident("price")), 2)     // → math::fixed(math::mean(price), 2)
surreal::cast(surreal::ident("qty"), "float")                // → type::float(qty)
```

### `case_when()` — multi-branch conditional

Builds a SurrealQL `IF … THEN … ELSE … END`. Note SurrealQL uses a single trailing `END`, not one
per branch:

```rhai
surreal::case_when()
    .when(surreal::avg(surreal::ident("price")) >= 250, "premium")
    .when(surreal::avg(surreal::ident("price")) >= 150, "mid")
    .else_("value")
    .expr()
// → IF math::mean(price) >= 250 THEN "premium" ELSE IF math::mean(price) >= 150 THEN "mid" ELSE "value" END
```

## Surreal-specific functions

Stats, collection, string, and time helpers — each a single-purpose name over a `math::`/`array::`/
`object::`/`string::`/`time::` function:

| Primitive | SurrealQL |
|---|---|
| `first(expr)` | `array::first(expr)` |
| `len(expr)` | `array::len(expr)` |
| `stddev(expr)` | `math::stddev(expr)` |
| `median(expr)` | `math::median(expr)` |
| `lower(expr)` | `string::lowercase(expr)` |
| `words(expr)` | `string::words(expr)` |
| `object_entries(expr)` | `object::entries(expr)` |
| `object_values(expr)` | `object::values(expr)` |
| `similarity(expr, term)` | `string::similarity::jaro_winkler(expr, "term")` |
| `time_group(expr, unit)` | `time::group(expr, "unit")` |

`similarity` and `time_group` bind their second argument (the search term, the bucket unit) as a
parameter, like any other string operand.

## Field and element access

Dotted paths and indexing use the `[…]` indexer rather than a separate primitive:

```rhai
surreal::ident("department")["name"]      // → department.name
surreal::ident("nutrition")["sugar"]      // → nutrition.sugar
some_subquery[0]                          // → (…)[0]   (integer index)
```

`surreal::ident("t")["col"].alias("x")` projects a path: `col_path AS x`.

## Graph traversal

SurrealDB replaces joins with graph paths (`->edge->table`, `<-edge<-table`). One positional
primitive expresses both directions — exactly one argument is the **anchor** (`surreal::me`, the
current record, or a nested `surreal::graph(…)`), and its **position** sets direction:

```rhai
surreal::graph(surreal::me, "placed", "order")      // → ->placed->order   (anchor left = outward)
surreal::graph("reports_to", surreal::me)           // → <-reports_to      (anchor right = inward)
surreal::graph(surreal::me, "reports_to", "employee")["name"]   // → ->reports_to->employee.name
```

Mixed direction comes from nesting — each `graph()` appends one directed hop:

```rhai
surreal::graph("client", "placed", surreal::graph(surreal::me, "placed", "order"))
// → ->placed->order<-placed<-client
```

`recurse(path, min, max)` wraps a path in ranged recursion:

```rhai
surreal::recurse(surreal::graph("employee", "reports_to", surreal::me), 1, 5)["name"]
// → @.{1..5}(<-reports_to<-employee).name
```

```admonish note title="Why position, not direction names?"
An `out`/`in`/`up`/`down` vocabulary either assumes a hierarchy or names the arrow rather than the
meaning. Anchoring by position keeps the call reading like the path it produces, and composition
(nesting) carries mixed direction without per-edge glyph overrides.
```

## Select clauses

Surreal-only clauses on the select builder:

```rhai
surreal::select().value().expression(surreal::max(surreal::ident("total"))).from("order").group_all()
// → SELECT VALUE math::max(total) FROM order GROUP ALL

surreal::select().field("price").from("product").split("tags")
// → SELECT price FROM product SPLIT tags
```

`.subquery()` parenthesizes a select so it composes as a scalar expression — it bridges a select into
the expression layer, where it pairs with the `[n]` indexer, `.alias()`, comparisons, and `from()`:

```rhai
surreal::select().value().expression(surreal::max(surreal::ident("total")))
    .from("order").where(surreal::ident("client") == surreal::parent("id")).group_all()
    .subquery()[0].alias("biggest_order")
// → (SELECT VALUE math::max(total) FROM order WHERE client = $parent.id GROUP ALL)[0] AS biggest_order
```

## Parameters

`param(name)` is any SurrealDB `$`-parameter (`$auth`, `$this`, `$parent`, a `LET`-bound name).
"Parameter" is SurrealDB's own term for `$`-prefixed names, so the primitive is named for it — and
**not** `var`, which Rhai reserves as a keyword.

```rhai
surreal::param("auth")["id"]        // → $auth.id
surreal::parent("id")               // → $parent.id   (sugar for the correlated-subquery case)
```

## Embedded-array closures

SurrealDB's `array.map`/`fold`/`filter` take an inline closure — the one place it exceeds the SQL
vocabulary. These are written as **native Rhai closures**, not a closure-as-data constructor: each
parameter is bound to a placeholder expression and the closure runs *symbolically*, so every operator
and indexer in the body builds SurrealQL instead of computing a value. A `#{…}` map lowers to an
object literal, `[…]` to an array literal, and `* + - /` render parenthesized. `map`, `fold` and
`filter` are methods, so they take no namespace.

```rhai
surreal::ident("lines").map(|l| #{
    product:  l["product"]["name"],
    subtotal: l["quantity"] * l["price"]
})
// → lines.map(|$value| { product: $value.product.name, subtotal: ($value.quantity * $value.price) })

surreal::ident("lines").fold(0, |acc, l| acc + l["quantity"] * l["price"])
// → lines.fold(0, |$acc, $value| ($acc + ($value.quantity * $value.price)))
```

```admonish warning title="The closure parameter name is engine-chosen"
A Rhai local can't carry a `$`, so the engine never sees your `|l|` — it binds its own placeholders
(`$value` for the item, `$acc` for a fold accumulator). The emitted SurrealQL therefore reads
`|$value| …`, not `|$l| …`. This is cosmetic: SurrealQL doesn't care about the parameter name, and the
result executes identically.
```

## Checklist

- Reach for a **named primitive** first; it handles the lowering, quoting, and escaping.
- Call free functions through **`surreal::`**; call methods, indexers and operators on their
  receiver.
- Use the **`vantage-sql` name** when the concept overlaps (`count`, `avg`, `round`, `coalesce`,
  `case_when`, `date_format`) — the same name works under `sql::`.
- Build **field paths** with the `["col"]` indexer, **graph paths** with `surreal::graph()` +
  `surreal::me`, and **mixed direction** by nesting rather than glyphs.
- Write **array closures** as native `|l| …` Rhai closures; expect engine-chosen `$value`/`$acc`
  parameter names in the output.
- Drop to **`surreal::expr("…")` / `surreal::fx(…)`** only when no primitive fits.
