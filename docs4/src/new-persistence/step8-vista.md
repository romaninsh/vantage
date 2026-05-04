# Step 8: Add Vista Support to Your Driver

`Vista` is the universal, schema-bearing handle that scripting, UI, and agent
code consume. Your driver already exposes a typed `Table<T, E>` (Steps 1–7);
adding Vista means wrapping that table so callers can drive it through a
CBOR/`String` boundary without knowing which backend is behind it.

This is a *thin* layer. Most of the work is decisions, not code.

### Before you start

Vista support sits on top of everything Steps 1–7 deliver — there is nothing
new to learn about your backend's protocol here. Confirm you have:

- [ ] Working `TableSource` impl (Step 5) — Vista delegates all CRUD to it.
- [ ] Native value type with `From<&Bson|&Value>`/`Into<CborValue>` story
      (Step 1) — the source layer translates at this boundary.
- [ ] An id type that round-trips to `String` (Step 5). If it doesn't yet,
      add `FromStr` and `Display` impls before going further.
- [ ] A `vista` cargo feature gating the new module so non-Vista users don't
      pull in `vantage-vista`.

### Pick your YAML vocabulary

Driver-specific YAML lives under a top-level key named after your driver
(`mongo:`, `csv:`, `sqlite:`, …). Keep the universal vocabulary —
`name`, `columns`, `flags`, `references`, `id_column` — for things every
backend understands. Push backend quirks (collection name, file path,
auth profile) into your block.

The strict rule: any struct you deserialize for your block sets
`#[serde(deny_unknown_fields)]`. Typos in YAML must be loud, not silent.

- [ ] Define `<Driver>TableExtras` with one field per top-level YAML knob,
      under `#[serde(deny_unknown_fields)]`.
- [ ] Define `<Driver>ColumnExtras` for per-column quirks (BSON aliases,
      nested paths, type hints your backend cares about).
- [ ] Reuse `NoExtras` for `ReferenceExtras` until you have a concrete need
      (Step 5 reference handling, stage 5 conditions).
- [ ] Make every field inside the block `Option<T>` with `#[serde(default)]`
      so the block can be omitted entirely when the spec name is enough.
- [ ] Type-alias `<Driver>VistaSpec = VistaSpec<TableExtras, ColumnExtras, NoExtras>`
      so callers see one short name.

### Decide your capability surface

`VistaCapabilities` is *the* contract. Lying here is worse than not
implementing — a `true` flag with no override is reported as
`Unimplemented`, while a `false` flag with no override is reported as
`Unsupported`. Both are loud, but they tell the operator different things.

Default policy: **only set a flag to `true` if the matching trait method
is overridden and works**. Read-only stores set `can_count: true` and stop
there. Don't pre-flag `can_subscribe` for "later" — flip it on the day
LIVE-query / change-streams actually land.

- [ ] List the operations your backend natively supports (read, write,
      count, subscribe, invalidate).
- [ ] For each, decide: implement now, or accept the default `Unsupported`
      error. Skip rather than stub.
- [ ] Pick `PaginateKind` honestly (`None`/`Offset`/`Cursor`). Cursor-only
      backends should not advertise offset.
- [ ] Document non-obvious gaps in the driver's README — `can_update: true`
      with no support for partial patches, for instance.

### Implement the value bridge

Your source converts between the native value type and CBOR at the
`VistaSource` boundary. Callers see `CborValue`; the wrapped table sees
your native type.

- [ ] Write `native_to_cbor(&NativeValue) -> CborValue` — exhaustive match
      on all variants, lossy paths flagged in module docs.
- [ ] Write `cbor_to_native(&CborValue) -> NativeValue` — handle CBOR `Tag`
      by unwrapping (drop the tag). Integers wider than your native type
      should stringify rather than silently truncate.
- [ ] Cover scalars + nested map + nested array in unit tests; one
      round-trip test catches most regressions.
- [ ] Don't expose the bridge as `pub` outside the vista module unless a
      specific caller needs it — keeps the surface small.

### Implement the source

The source is a struct that owns one `Table<Driver, EmptyEntity>` plus the
capability set, plus any per-column metadata you need (nested paths,
aliases). Methods translate at the boundary and delegate everything else
to the table.

- [ ] Struct holds: typed `Table<Driver, EmptyEntity>`, `VistaCapabilities`,
      and a `column_paths: IndexMap<String, Vec<String>>` if your backend
      supports nested fields.
- [ ] `impl VistaSource` — implement only the methods your capability set
      claims; let the others fall through to the default `Unsupported`
      error.
- [ ] `add_eq_condition` translates `(field, CborValue)` into your native
      condition type and pushes it onto the wrapped table via
      `table.add_condition(...)`. **Never store conditions on Vista or in
      the source struct itself.**
- [ ] Path-aware reads: when `column_paths` is non-empty, walk the path
      for each spec column instead of returning top-level keys verbatim.
- [ ] Path-aware writes: rebuild nested sub-documents from sibling
      columns sharing a prefix.

### Implement the factory

The factory is the construction surface. It exposes one inherent method
per entry path and one trait impl for YAML.

- [ ] `<Driver>::vista_factory(&self) -> <Driver>VistaFactory` — inherent
      method on your data source so callers find it without imports.
- [ ] `from_table<E>(Table<Driver, E>) -> Result<Vista>` — typed entry
      point. Collapse to `EmptyEntity`, harvest column metadata, hand off
      to the source.
- [ ] `impl VistaFactory` with the `Extras` associated types and a
      `build_from_spec` that builds a `Table<Driver, EmptyEntity>` from
      the spec, then wraps it via the same code path as `from_table`.
      One construction path, one reading path.
- [ ] Override the vista's display name with `spec.name` (since the
      spec name often differs from the underlying collection/file/table).
- [ ] Resolve the id column in this order: `spec.id_column` → first
      column with the `id` flag → backend default (e.g. `"_id"` for Mongo,
      `"id"` for most SQL).

### Test the integration

Ship two layers: cheap unit tests for the bridge + spec parsing (no
backend needed), and gated integration tests against a real instance.

- [ ] Unit-test the value bridge — round-trip every variant, including
      lossy ones (assert the documented lossy form).
- [ ] Unit-test YAML parsing — minimal spec, unknown-field rejection,
      missing optional block.
- [ ] Integration: typed `from_table` round-trip — insert via table,
      list via vista, assert CBOR shapes.
- [ ] Integration: spec round-trip — `from_yaml`, list, filter via
      `add_condition_eq`, write via vista, read back.
- [ ] Capability honesty test — `vista.capabilities()` matches what the
      source actually overrides.
- [ ] Use `Result<(), Box<dyn Error>>` as the test return type and `?`
      everywhere. Avoid `.unwrap()` in test bodies — error context is
      worth more than a panic.

### Known sharp edges

A few things bite every driver. They're worth flagging up front rather
than discovering during review.

- [ ] **Id translation is the most common bug.** Round-trip your native
      id through `String` and back; assert that `vista.get_value(&id)`
      finds what `vista.list_values()` returned the id for.
- [ ] **Aliases at the table level may not survive.** Some drivers'
      `TableSource` impls ignore column aliases when materializing
      records. Audit your read path before relying on aliases for
      renames; the safer bet is to handle renames in the vista source
      via `column_paths`.
- [ ] **Don't filter in memory.** If your backend can push the filter
      down (SQL `WHERE`, Mongo filter doc, REST query param), do that —
      that's the entire reason `add_eq_condition` mutates the underlying
      table instead of stashing state on Vista.
- [ ] **Conditions stay driver-typed for now.** Universal/portable
      conditions land in a later stage; until then, only translate `eq`,
      and reject the rest at construction with a clear message.
- [ ] **Capability flags are cheap to flip later.** Start narrow.
