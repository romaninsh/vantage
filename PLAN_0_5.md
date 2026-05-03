# PLAN_0_5.md — 0.5 design scratch pad

Not urgent. This file captures an in-progress brainstorm for three large
architectural features we want in 0.5, plus the cross-cutting concerns they
surface. The intent is generic UI components that can drive `AnyTable`
directly — which is what makes most of these features worth doing together
rather than one at a time.

Everything here is open for revision. Sections end with **Open questions**
that we need to come back to before implementation.

---

## 1. Column visibility + column defaults

### What we want

Columns already carry flags (`ColumnFlag` in `vantage-table/src/column/flags.rs`).
Add a `Hidden` flag and a `default: Option<T>` on `Column<T>`. Two driving use
cases:

- **UI hints.** A generic table editor rendering an `AnyTable` should know
  which columns to show and which to skip. `category_id` is useful for
  queries but noise in the editor.
- **Set-aware inserts.** Given `let vip = users.with_condition(vip_flag.eq(true))`,
  inserting a new user through `vip` should auto-set `vip_flag = true`. The
  caller already narrowed the set — the insert should preserve that
  narrowing. Same idea covers `bakery.ref("products")` where products
  inserted through that ref should inherit `bakery_id`.

### Sketch

- New column flag `Hidden` — purely a UI hint.
- New field `default: Option<T>` on `Column<T>` — applied on insert if
  the entity doesn't provide the field.
- `with_condition` tries to inspect the condition and, where it sees a
  flat `col.eq(literal)`, marks `col` as hidden + sets its default on
  *the returned Table only*. Parent table unaffected.

### The hard parts

**Backend-agnostic inspection.** We can walk SQL `Expression`s but
`MongoCondition` is a BSON document — its "eq" form is
`{ field: { "$eq": v } }` with no guarantee of a typed leaf. If
auto-detection can't be made uniform, the escape hatch is an explicit API:

```rust
let vip = users
    .with_condition(vip_flag.eq(true))
    .with_default("vip_flag", true);
```

Naming open — `with_default`, `with_hidden_default`, `with_narrowing`, etc.
The distinction is important because the semantics differ:

- Auto-detection binds condition → default implicitly.
- Explicit API separates "this column is filtered to value V" from "this
  column defaults to V on insert" — they're the *same value* but
  conceptually two things.

**`with_one` / `with_many` is worse.** `bakery.ref("products")` narrows by
`products.bakery_id = bakery.id`, where `bakery.id` is an expression (IN
subquery in practice), not a literal. We can't set a default because we
don't know the value at condition-building time — only at insert time.

Options for references:
- **Validate instead of default.** Let the insert go through, then read it
  back (`get`); if it's not in the narrowed set, rollback. Requires
  transactions (tracked separately).
- **Lazily resolve on insert.** If the condition came from a same-backend
  reference where the parent id is known (e.g. `bakery.with_id(1).ref("products")`),
  we can capture the parent's id and pre-fill `bakery_id`. If the parent is
  another set (`vip_clients.ref("orders")`), we can't — only validation works.
- **Punt for now.** Ship defaults for simple literal conditions, document
  that reference narrowing doesn't auto-fill, use transaction-rollback as
  the safety net.

**Column visibility isn't one thing.** UI rendering needs nuance:
- *Hidden forever.* Internal id columns, audit stamps — never show.
- *Hidden by default, expandable.* Advanced/technical fields the user can
  reveal.
- *Hidden in this subset only.* The narrowing case — `vip_flag` is hidden
  inside `vip`, but visible in the unfiltered `users`.
- *Hidden from list view, visible in detail view.* Body text columns that
  bloat a list.
- *Hidden from input, shown in output.* Computed fields, timestamps.
- *Conditional on role/tenant.* Out of scope for column metadata — that's
  hook territory.

Our column flag model should express at least the first three. The rest
may be UI-side concerns we don't encode on the column.

**Defaults as values vs functions.** `default: Option<T>` covers literals.
Natural next step: `default: DefaultFn<T>` where `DefaultFn` can hold
`T`, `Fn() -> T`, or `AsyncFn() -> T`. Async opens "fetch from config
service" style defaults but introduces an await in a previously sync path.
Leaning: start with sync (`T | Fn() -> T`), add async only when a use case
demands it.

**Optional fields with defaults.** If the entity has `Option<String>`
and the column has a default, what happens when the user inserts with
`None`?
- (a) "`None` is explicit absence" → respect `None`, store NULL.
- (b) "`None` means 'use default'" → substitute the default.
- (c) Error / warn.

We haven't decided. Rust's conventions lean toward (a) — `None` is a
value, not a missing field. But set-narrowing semantics argue for (b) —
the user said `.with_condition(vip.eq(true))`, a `None` slip shouldn't
bypass that intent.

### Open questions

- Auto-detect from conditions (complex, backend-specific) or explicit API
  (clear, more verbose)?
- Name for the explicit API.
- What's the full list of visibility modes we want to encode on the column?
- Literal default vs function default vs both — and sync vs async for the
  function case.
- `Option<T>` field + default — which of (a)(b)(c) wins?
- Conflict resolution: user explicitly sets `vip_flag = false` while
  inserting into the `vip` set — override, respect, or error?
- How far do we push reference-based narrowing? (plain id → literal works;
  subquery narrowing needs transaction rollback)

---

## 2. Column (de)serialisation mapping

### What we want

Entity field name is stable (consumers do `record["email"]` or
`user.email`). Storage column name and value shape might differ per
backend, per column, per user. Today we have one path: serde. That works
until you want per-persistence-per-column customisation.

Motivating examples:

- Dates in SQLite stored as ISO strings today. User might want unix
  timestamps. Currently no escape hatch without a custom type wrapper.
- Enum drift: DB has `"ARCHIVED"` but the Rust enum dropped that variant.
  Failing the whole fetch is rarely what you want.
- Same entity stored in Mongo (BSON ObjectId) and SQL (VARCHAR 36): `id`
  field in Rust, but `_id` in Mongo vs `id` in SQL.
- "Store this field into a *different* table" — `user.preferences` is a
  `HashMap<String, String>` but each key is its own row in a
  `user_preferences` table.

### Sketch

Column gets optional encode/decode closures:

```rust
Column::<Date>::new("birthday")
    .with_physical_name("birthday_unix")
    .with_encoder(|date: &Date| date.timestamp())
    .with_decoder(|v: i64| Date::from_timestamp(v));
```

Read path: pull under physical name → run decoder → place under entity
name. Write path: pull entity name → run encoder → write under physical
name. No closure = straight passthrough (current behaviour).

### The hard parts

**Study serde's prior art first.** serde has `#[serde(with = "...")]`,
`serialize_with`, `deserialize_with`, `#[serde(from = "...")]`, custom
serializers. The split "decoder for read, encoder for write" mirrors
`SerializeWith`/`DeserializeWith`. Worth looking at their signatures,
fallible variants, and how they compose with defaults/Option before
we reinvent.

**Validation and fallbacks on decode.** If the DB has an enum value our
Rust type doesn't know about, options are:
- Error the whole `list()` (current behaviour — one bad row poisons all).
- Skip the row silently (dangerous).
- Substitute a fallback value (best for UI-driven tools that need to render
  *something*).
- Wrap the field in `Result<T, DecodeError>` at the entity level (too
  invasive for most users).

Probably want: decoder returns `Result<T, DecodeError>`, Column carries
`on_decode_error: FallbackStrategy` — `Error | Skip | Fallback(T)`. UI
mode can default to `Skip` or `Fallback`; strict mode stays `Error`.

**Validation on encode.** Symmetric — a value might be valid in Rust but
not storable in this backend (too long for varchar, negative for unsigned,
non-UTF8 string). Encoder returns `Result<Value, EncodeError>`.

**Combining columns.** "Store `first + last` as `full_name`" is a
whole-row concern; one column's encoder can't see sibling values. Options:
- Don't solve it here — let users do that transform in their entity's
  `IntoRecord` impl (serde-style).
- Add a `Table::with_row_encoder(|&E| Record<Value>)` hook that runs
  before column encoders.
- Say that's a hook job (#3).

Leaning: let serde's `IntoRecord` handle combination at the entity
boundary, columns only touch their own value, hooks handle side effects.

**Cross-table writes / cross-backend writes belong in hooks (#3).** A
column's encoder shouldn't issue a second query. That's a trigger, not a
transform. Hard rule.

**Sync vs async.** All of the "transform this value" cases are sync.
Async is only tempting when you consider cross-backend writes — but those
belong in hooks. So columns stay sync. If the case for async column
transforms ever becomes real, revisit.

**Binary data.** `Vec<u8>` is already in TODO.md as a type-system gap.
With encoder/decoder hooks we can solve two orthogonal things at once:
backend type mapping (BYTEA vs BLOB vs base64) and format (raw bytes,
base64 string, hex). But we should lock down the `Vec<u8>` type-system
story separately from the mapping feature — they compose but shouldn't
block each other.

### Open questions

- Encoder only, decoder only, or both? (Probably both, but many users
  only need one.)
- Signature: `Fn(&T) -> Result<Value, E>` or `Fn(T) -> Result<Value, E>`?
  (Ownership vs copy; matters for large values like `Vec<u8>`.)
- How does the column know which error strategy to apply on decode —
  column-level config vs table-level default?
- How does physical name interact with joins / nested documents?
  `meta.created_at` as a physical name for a top-level entity field?
- Is there a way to reuse serde's existing `serialize_with`/`deserialize_with`
  attributes at the entity level, and when should a user reach for that vs
  a column-level mapping?
- Versioning: if the encoder changes, old data read back with the new
  decoder breaks. Out of scope, or schema-versioning territory?

---

## 3. Table-level hooks

### What we want

Third-party extensions (soft delete, audit trail, automatic timestamps,
tenant scoping, denormalised counters, cache invalidation) need to hook
into a `Table<T, E>`'s lifecycle without subclassing or wrapping. Current
workaround is to wrap one table in another, which doesn't compose.

### Sketch

```rust
pub trait TableHook<T: TableSource, E: Entity<T::Value>>: Send + Sync {
    async fn before_select_build(&self, select: &mut T::Select) -> Result<()> { Ok(()) }

    async fn before_insert(&self, record: &mut Record<T::Value>) -> Result<Outcome> { Ok(Outcome::Continue) }
    async fn after_insert (&self, record: &Record<T::Value>) -> Result<()> { Ok(()) }

    async fn before_update(&self, id: &T::Id, patch: &mut Record<T::Value>) -> Result<Outcome> { Ok(Outcome::Continue) }
    async fn after_update (&self, id: &T::Id, record: &Record<T::Value>) -> Result<()> { Ok(()) }

    async fn before_delete(&self, id: &T::Id) -> Result<Outcome> { Ok(Outcome::Continue) }
    async fn after_delete (&self, id: &T::Id) -> Result<()> { Ok(()) }
}

pub enum Outcome {
    Continue,      // proceed normally
    Skip,          // pretend it succeeded, don't touch storage
    // extension-defined variants, e.g. "converted delete into update"
}
```

`Table<T, E>` holds `Vec<Arc<dyn TableHook<T, E>>>`, dispatched in
registration order. An "extension" is just a struct implementing the
trait that bundles several methods (SoftDelete hooks `before_delete` +
`before_select_build`; Audit hooks all the `after_*` calls).

### The hard parts

**Save points on ActiveEntity / ActiveRecord.** `entity.save()` calls
`dataset.replace` under the hood — goes through `before_update` then
`after_update`. Does it? Or does save get its own pair (`before_save`,
`after_save`)? Users think in "save" terms, not "update" terms. Probably
*both*: the dataset-level hook fires, and `ActiveEntity::save` is just
a convenience wrapper. Symmetric for `entity.delete()` and `entity.reload()`
(no hook needed — read-only).

**Type erasure through `AnyTable`.** Hooks typed on `<T, E>` don't
survive erasure. Options:
- (a) Provide an erased hook trait `AnyTableHook` that works on
  `Record<Value>` only. Doubles the trait surface; every feature has two
  flavours.
- (b) Hooks don't follow through erasure; when a table is erased you lose
  its hooks, which might be surprising.
- (c) Hooks follow through erasure but are only *invoked* when operations
  go through typed paths; `AnyTable::insert_value` skips hooks.

Given the UI-driven scenario that motivated this whole plan, (a) looks
necessary — but it's a lot of code.

**Ordering and conflicts.** Two hooks both want `before_insert`. What
if they stamp the same field? Registration order is simple and usually
enough. Priority numbers invite bikeshedding.

**Veto vs skip vs intercept.** Three distinct outcomes:
- *Veto*: `Err(...)` — operation fails.
- *Skip*: pretend it succeeded, don't touch storage (soft-hide under
  filtered view).
- *Intercept*: the hook handled the operation itself (soft delete's
  before_delete turns into an update).

The `Outcome` enum handles the second. Intercept is harder — the hook
needs to be able to run *instead of* the default storage call. Probably
`Outcome::Handled` meaning "I did the work, skip storage, but the caller
should see success."

**Interaction with #1 (defaults).** Defaults also write values into the
record on insert. Does that run before or after `before_insert` hooks?
Defaults first (they're column-level config, not custom logic), then
hooks (which might override). Document clearly.

**Interaction with #2 (encoders).** Column encoders transform values at
the storage boundary. Hooks see encoded records — storage-layer view. So
a hook logging "user insert" sees `_id` not `id` in the Mongo case. If
that's surprising, we could offer entity-level hooks too, but now we have
four variants per event. Not yet.

**What about `ActiveRecord<Category>` fields?** If an entity has a field
like `category: ActiveRecord<Category>`, saving the outer entity implies
what — upserting the nested one? Setting the `category_id` FK only?
Silent?

This is a bigger feature in itself — see below.

### Open questions

- Start with minimal lifecycle (`before_insert`, `before_delete`,
  `before_select_build`) and add the rest as use cases surface?
- Sync-capable or async-only? Async-only is simpler but forces `tokio::block_on`
  for trivial sync hooks.
- Erased hook variant `AnyTableHook` — worth the doubled surface, or
  scope down to typed hooks only for now?
- Outcome variants: just `Continue | Skip`, or add `Handled` for
  intercept?
- Where does `ActiveEntity::save()` sit — same hook as `replace`, or its
  own pair?

---

## Cross-cutting

### The three features overlap a lot

- **Defaults (#1) + encoders (#2).** A default is "encoder that ignores
  input and returns literal". We could unify, but the ergonomics diverge
  — defaults are UI-hint-ey, encoders are data-shape-ey.
- **Defaults (#1) + hooks (#3).** A `before_insert` hook could stamp any
  value into the record. That makes defaults a special case of hooks.
  But defaults are declarative per-column metadata; hooks are imperative
  per-table. Keep separate.
- **Encoders (#2) + hooks (#3).** Encoders transform the value of *one
  column*. Hooks see the whole record and can write *other rows*. Encoders
  are sync + cheap; hooks are async + arbitrary. Clear separation.

Consensus: keep them as three distinct mechanisms with well-drawn
boundaries — overlaps are ergonomic, not mechanical.

### Binary data

Separate thread from all three features. `Vec<u8>` needs type-system
entries per backend (Postgres BYTEA, MySQL BLOB, SQLite BLOB, Mongo
Binary BSON). Already tracked in TODO.md. With column encoders (#2)
users can additionally choose format — raw, base64, hex — per column.
Don't block the type-system work on the mapping feature.

### Relationship eagerness — `ActiveRecord<Category>` / `Thing<Category>`

Raised separately but deserves its own section. Today we have:

```rust
struct Order {
    id: String,
    category_id: String,   // just the FK
}
```

User does `order.ref("category")` to traverse. That's two round trips
(or an `IN` subquery) to render an order list with category names.

Two shapes proposed:

```rust
// Option A — eager-loaded full record
struct Order {
    id: String,
    category: ActiveRecord<Category>,
}

// Option B — ID-with-type-hint (SurrealDB-native via Thing, generalisable)
struct Order {
    id: String,
    category: Thing<Category>,
}
```

(A) is powerful but expensive — every order list fetches every category.
Needs batching (`IN (...)` on the fetch) to avoid N+1. (B) is thin — just
a typed id you can `.resolve()` when you need the full record.

Open questions:
- Does (A) auto-batch, or does the user have to opt in?
- Does saving an entity with `category: ActiveRecord<Category>` upsert
  the nested record, or only the outer one?
- Is `Thing<T>` just a `(T::Id, PhantomData<T>)` pair with a `.resolve(&table) -> Option<T>`
  method? That's probably the minimal useful shape.
- Should this integrate with column encoders (#2) — `Thing<Category>` has
  an encoder that flattens to the FK string, and a decoder that rehydrates
  from it?

This one's maybe a 0.6 feature, not 0.5 — calling it out so we don't
forget.

---

## Revisit list

Things the three sections above left explicitly unresolved:

1. Column visibility modes — full enumeration.
2. Literal vs function defaults; sync vs async.
3. `Option<T>` field + column default — precedence.
4. Explicit API name for `with_default` / `with_narrowing`.
5. Reference-based narrowing — which cases get defaults, which need
   transaction-rollback validation.
6. Condition inspection feasibility across SQL / Mongo / Surreal backends.
7. Encoder/decoder signature — ownership, fallibility, error-strategy scope.
8. Whether serde's `serialize_with` attribute is the right abstraction to
   borrow.
9. `AnyTableHook` erased-hook trait — build or skip.
10. Hook outcome variants — `Continue | Skip` or add `Handled`.
11. `ActiveEntity::save()` — routed through `before_update` or its own
    `before_save`?
12. Interaction order between defaults, encoders, and hooks on the insert
    path.
13. `ActiveRecord<T>` / `Thing<T>` — 0.5 or 0.6.

Not a commit-to list. Most of these only get resolved when we sit down
to implement.
