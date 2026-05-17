# CLI walkthrough

A tour of the `bakery_model3` Vista-driven CLI, starting from the simplest
query and working up to multi-stage chains across relations.

Every example here is a real run against the seeded SQLite copy of the
bakery data. Build once, then follow along:

```bash
cargo run --example cli-vista -p bakery_model3 -- sqlite bakery
```

The argument grammar is shared with the underlying
`vantage_cli_util::vista_cli` runner; see that crate's docs for the full
reference. This page is the beginner path.

## 1. List a model

```
sqlite products
```

```
──────────────────────────────────────────────────────────────────────────────────────────
 ID               NAME                            CALORIES   PRICE   IS_DELETED   STICKER
══════════════════════════════════════════════════════════════════════════════════════════
 flux_cupcake     Flux Capacitor Cupcake          300        120     false        cat
 delorean_donut   DeLorean Doughnut               250        135     false        dog
 time_tart        Time Traveler Tart              200        220     false        —
 sea_pie          Enchantment Under the Sea Pie   350        299     false        pig
 hover_cookies    Hoverboard Cookies              150        199     false        chicken
──────────────────────────────────────────────────────────────────────────────────────────
5 records
```

Plural model names list. Records come back in driver-native order — for
SQLite that's seed order.

## 2. Single record by id

```
sqlite product id=flux_cupcake
```

```
id: flux_cupcake
--------
name: Flux Capacitor Cupcake
calories: 300
price: 120
is_deleted: false
sticker: cat

Relations:
  :bakery  → one
```

Two things flipped this into single-record card mode: the singular model
name (`product`, not `products`), and the `id=` sugar — which resolves to
whatever the table calls its id column.

## 3. Filter with `field=value`

```
sqlite clients is_paying_client=true
```

```
────────────────────────────────────────────────────────────────────────────────────────────────────────
 ID      NAME          EMAIL             CONTACT_DETAILS   IS_PAYING_CLIENT   BAKERY_ID     ORDER_COUNT
════════════════════════════════════════════════════════════════════════════════════════════════════════
 marty   Marty McFly   marty@gmail.com   555-1955          true               hill_valley   1
 doc     Doc Brown     doc@brown.com     555-1885          true               hill_valley   2
────────────────────────────────────────────────────────────────────────────────────────────────────────
2 records
```

Biff drops out — he isn't a paying client. The string `"true"` is
auto-detected as a boolean before reaching the driver.

## 4. Narrow by index — `[N]`

```
sqlite 'clients[0]'
```

```
id: marty
--------
name: Marty McFly
email: marty@gmail.com
contact_details: 555-1955
is_paying_client: true
bakery_id: hill_valley
order_count: 1

Relations:
  :bakery  → one
  :orders  ↠ many
```

`[0]` picks the first row of the (filtered) list and switches the render
into card mode, ready for traversal.

## 5. Pick columns — `=col1,col2`

```
sqlite clients =name,email
```

```
─────────────────────────────────────
 NAME          EMAIL
═════════════════════════════════════
 Marty McFly   marty@gmail.com
 Doc Brown     doc@brown.com
 Biff Tannen   biff-3293@hotmail.com
─────────────────────────────────────
3 records
```

The default column set is the metadata-declared one; `=…` overrides it
for the next render.

## 6. Follow a relation — `:rel`

```
sqlite client id=marty :bakery
```

```
id: hill_valley
--------
name: Hill Valley Bakery
profit_margin: 15

Relations:
  :clients  ↠ many
  :products  ↠ many
```

Marty → his bakery. The relation is declared `HasOne`, so the render stays
in card mode. `HasMany` relations stay in list mode.

```
sqlite 'bakery[0]' :clients
```

```
────────────────────────────────────────────────────────────────────────────────────────────────
 ID      NAME          EMAIL                   CONTACT_DETAILS   IS_PAYING_CLIENT   BAKERY_ID
════════════════════════════════════════════════════════════════════════════════════════════════
 marty   Marty McFly   marty@gmail.com         555-1955          true               hill_valley
 doc     Doc Brown     doc@brown.com           555-1885          true               hill_valley
 biff    Biff Tannen   biff-3293@hotmail.com   555-1955          false              hill_valley
────────────────────────────────────────────────────────────────────────────────────────────────
3 records
```

## 7. Sort ascending — `[+col]`

```
sqlite products '[+price]'
```

```
──────────────────────────────────────────────────────────────────────────────────────────
 ID               NAME                            CALORIES   PRICE   IS_DELETED   STICKER
══════════════════════════════════════════════════════════════════════════════════════════
 flux_cupcake     Flux Capacitor Cupcake          300        120     false        cat
 delorean_donut   DeLorean Doughnut               250        135     false        dog
 hover_cookies    Hoverboard Cookies              150        199     false        chicken
 time_tart        Time Traveler Tart              200        220     false        —
 sea_pie          Enchantment Under the Sea Pie   350        299     false        pig
──────────────────────────────────────────────────────────────────────────────────────────
5 records
```

Compare with section 1 — the same five rows, now ordered `120 → 299`.

## 8. Sort descending — `[-col]`

```
sqlite products '[-calories]'
```

```
──────────────────────────────────────────────────────────────────────────────────────────
 ID               NAME                            CALORIES   PRICE   IS_DELETED   STICKER
══════════════════════════════════════════════════════════════════════════════════════════
 sea_pie          Enchantment Under the Sea Pie   350        299     false        pig
 flux_cupcake     Flux Capacitor Cupcake          300        120     false        cat
 delorean_donut   DeLorean Doughnut               250        135     false        dog
 time_tart        Time Traveler Tart              200        220     false        —
 hover_cookies    Hoverboard Cookies              150        199     false        chicken
──────────────────────────────────────────────────────────────────────────────────────────
5 records
```

`350 → 150`.

## 9. Sort + pick a row — `[+col:N]`

```
sqlite 'products[-price:0]'
```

```
id: sea_pie
--------
name: Enchantment Under the Sea Pie
calories: 350
price: 299
is_deleted: false
sticker: pig

Relations:
  :bakery  → one
```

Reads as "sort descending by price, take row 0" — the most expensive
product, in card mode.

## 10. Search — `?keyword`

```
sqlite clients '?marty'
```

```
────────────────────────────────────────────────────────────────────────────────────────────────────────
 ID      NAME          EMAIL             CONTACT_DETAILS   IS_PAYING_CLIENT   BAKERY_ID     ORDER_COUNT
════════════════════════════════════════════════════════════════════════════════════════════════════════
 marty   Marty McFly   marty@gmail.com   555-1955          true               hill_valley   1
────────────────────────────────────────────────────────────────────────────────────────────────────────
1 record
```

The driver fans the keyword out across columns flagged searchable. The
match can be in any of them — here both `id` and `name` and `email` would
hit, but the row is reported once.

```
sqlite clients '?gmail'
```

```
────────────────────────────────────────────────────────────────────────────────────────────────────────
 ID      NAME          EMAIL             CONTACT_DETAILS   IS_PAYING_CLIENT   BAKERY_ID     ORDER_COUNT
════════════════════════════════════════════════════════════════════════════════════════════════════════
 marty   Marty McFly   marty@gmail.com   555-1955          true               hill_valley   1
────────────────────────────────────────────────────────────────────────────────────────────────────────
1 record
```

Only Marty's email contains `gmail`.

## 11. Combining tokens

Tokens are processed left-to-right; each one mutates the in-flight Vista.

```
sqlite clients is_paying_client=true '[+name]' =name,email
```

```
───────────────────────────────
 NAME          EMAIL
═══════════════════════════════
 Doc Brown     doc@brown.com
 Marty McFly   marty@gmail.com
───────────────────────────────
2 records
```

Filter first (Biff out), sort ascending by name (Doc < Marty), trim
columns last.

## 12. Output formats

`--format=` selects the renderer. Position-agnostic — anywhere on the
line.

### JSON

```
--format=json sqlite client id=marty
```

```
{"marty":{"id":"marty","name":"Marty McFly","email":"marty@gmail.com","contact_details":"555-1955","is_paying_client":true,"bakery_id":"hill_valley","order_count":1}}
```

### NDJSON

```
--format=ndjson sqlite products '[+price]'
```

```
{"_id":"flux_cupcake","id":"flux_cupcake","name":"Flux Capacitor Cupcake","calories":300,"price":120,"is_deleted":false,"sticker":"cat"}
{"_id":"delorean_donut","id":"delorean_donut","name":"DeLorean Doughnut","calories":250,"price":135,"is_deleted":false,"sticker":"dog"}
{"_id":"hover_cookies","id":"hover_cookies","name":"Hoverboard Cookies","calories":150,"price":199,"is_deleted":false,"sticker":"chicken"}
{"_id":"time_tart","id":"time_tart","name":"Time Traveler Tart","calories":200,"price":220,"is_deleted":false,"sticker":null}
{"_id":"sea_pie","id":"sea_pie","name":"Enchantment Under the Sea Pie","calories":350,"price":299,"is_deleted":false,"sticker":"pig"}
```

One record per line — pipes straight into `jq`.

### CBOR-diag

```
--format=cbor-diag sqlite client id=biff
```

```
"biff": {"id": "biff", "name": "Biff Tannen", "email": "biff-3293@hotmail.com", "contact_details": "555-1955", "is_paying_client": false, "bakery_id": "hill_valley", "order_count": 0}
```

RFC 8949 §8 diagnostic notation. Lossless — `false` is a bool, `0` is an
integer, strings are quoted. This is the format used for cross-driver
golden tests, where every backend has to match byte-for-byte.

## 13. Chains across relations

Sort and search apply to whatever Vista is currently in-flight — after a
`:relation` traversal, that's the child Vista.

```
sqlite 'bakery[0]' :clients '[+name]'
```

```
────────────────────────────────────────────────────────────────────────────────────────────────
 ID      NAME          EMAIL                   CONTACT_DETAILS   IS_PAYING_CLIENT   BAKERY_ID
════════════════════════════════════════════════════════════════════════════════════════════════
 biff    Biff Tannen   biff-3293@hotmail.com   555-1955          false              hill_valley
 doc     Doc Brown     doc@brown.com           555-1885          true               hill_valley
 marty   Marty McFly   marty@gmail.com         555-1955          true               hill_valley
─────────────────────────────────────────────────────────────────────────────────────────────────
3 records
```

The bakery's clients, sorted alphabetically.

```
sqlite 'bakery[0]' :clients '?555' =name,contact_details
```

```
───────────────────────────────
 NAME          CONTACT_DETAILS
═══════════════════════════════
 Marty McFly   555-1955
 Doc Brown     555-1885
 Biff Tannen   555-1955
───────────────────────────────
3 records
```

All three contact strings contain `555`, so the search keeps them all —
but only `name` and `contact_details` render.

## 14. Sort-then-narrow, then traverse

```
--format=json sqlite 'clients[+name:0]' :bakery
```

```
{"hill_valley":{"id":"hill_valley","name":"Hill Valley Bakery","profit_margin":15}}
```

"Pick the alphabetically-first client, then walk to her bakery, then
render as JSON." Tokens compose freely; the runner threads the in-flight
Vista through each step.

## 15. Putting it all together

```
--format=cbor-diag sqlite 'bakery[0]' :products '[-price]'
```

```
{"sea_pie": {"id": "sea_pie", "name": "Enchantment Under the Sea Pie", "calories": 350, "price": 299, "is_deleted": false, "sticker": "pig"}, "time_tart": {"id": "time_tart", "name": "Time Traveler Tart", "calories": 200, "price": 220, "is_deleted": false, "sticker": null}, "hover_cookies": {"id": "hover_cookies", "name": "Hoverboard Cookies", "calories": 150, "price": 199, "is_deleted": false, "sticker": "chicken"}, "delorean_donut": {"id": "delorean_donut", "name": "DeLorean Doughnut", "calories": 250, "price": 135, "is_deleted": false, "sticker": "dog"}, "flux_cupcake": {"id": "flux_cupcake", "name": "Flux Capacitor Cupcake", "calories": 300, "price": 120, "is_deleted": false, "sticker": "cat"}}
```

Bakery's products, sorted descending by price (`299 → 120`), serialised
in lossless CBOR diagnostic notation.

One thing worth noting in this run: the column override doesn't carry
through a `:relation` step — traversal lands on a fresh Vista with its
own default columns. Add a new `=…` token after the traversal if you
want one on the child.

## Where to go next

- Same grammar drives every backend (`csv`, `sqlite`, `postgres`,
  `mongo`, `surreal`). Backends advertise which operators they support
  via capability flags; ones they don't surface as honest `Unsupported`
  errors rather than silently dropping the request.
- `--format=cbor-diag` is the right pick when piping output into another
  tool that needs type-faithful records; `--format=json` is right when
  you're piping into `jq`.
- The token grammar has more vocabulary the parser already accepts —
  operator conditions (`:lt=`, `:gt=`, `:like=`, `:in=`, `:null`),
  range slices (`[N:M]`), and aggregates (`@sum:field`, `@count`).
  These parse cleanly today; the runner notes which underlying call
  they map to as those land on the universal Vista surface.
