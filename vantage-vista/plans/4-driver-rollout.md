# Stage 4 — Driver rollout

Status: **Mostly done** — CSV, MongoDB, SurrealDB, SQLite/Postgres/MySQL,
REST, GraphQL, and LogWriter all ship Vista bridges. AWS (account-level
+ DynamoDB), Redb, and the api-pool wrapper remain. A cross-cutting
`TableShell::get_ref` gap applies to every driver except REST/GraphQL.

Roll out Vista support to remaining drivers. Each driver is its own
sub-discussion — different backends have different gotchas and may
declare different capabilities.

## Pattern from CSV + MongoDB

The two earliest drivers settled the cross-driver shape that later
drivers should follow. Authoritative writeup is in
`docs4/src/new-persistence/step8-vista.md`; quick recap:

- A `vista` cargo feature gates the bridge so non-vista users don't pull
  in `vantage-vista`.
- `<Driver>::vista_factory()` inherent method as the entry point;
  returns a `<Driver>VistaFactory` wrapping the configured data source.
- `from_table<E>(Table<Driver, E>) -> Result<Vista>` — typed entry path.
  Later drivers (SQL, SurrealDB) preserve the original entity type
  instead of erasing to `EmptyEntity` so `with_expression` closures
  parameterised over `E` keep typechecking.
- `VistaFactory::build_from_spec` — builds a `Table<Driver, …>`
  from the spec, then wraps it through the same source-creation code as
  `from_table`. One construction path, one reading path.
- Driver extras live under a top-level key named after the driver
  (`csv:`, `mongo:`, `sqlite:`, …); column extras live under the same
  key inside each column entry. All driver blocks set
  `deny_unknown_fields` to catch typos.
- The source converts native id type → `String` and native value type
  → `CborValue` at the boundary.
- **Conditions never live on Vista.** `add_eq_condition` translates the
  CBOR pair into the driver's native condition type and pushes it onto
  the wrapped table — server-side filter push-down is automatic.
- **Nested fields use `column_paths: IndexMap<String, Vec<String>>`.**
  Source walks paths on read, rebuilds sub-docs on write, uses
  dot-notation (or backend equivalent) on filter. MongoDB ships this
  via `mongo: { nested_path: address.city }`.

The driver-level file layout the in-tree drivers converged on:

```
<driver>/src/vista/
├── mod.rs       re-exports + <Driver>::vista_factory() inherent impl
├── spec.rs      <Driver>TableExtras / <Driver>ColumnExtras / <Driver>VistaSpec
├── factory.rs   <Driver>VistaFactory + impl VistaFactory + spec→table helpers
├── source.rs    <Driver>TableShell + impl TableShell
└── cbor.rs      native ↔ CBOR bridge (only when native value type ≠ JSON-shaped)
```

## Status snapshot

| Driver | TableSource | Factory | Shell | Integration tests | Capabilities advertised |
|---|---|---|---|---|---|
| `vantage-csv` | `Csv` | ✅ | ✅ | ✅ | `count` (read-only) |
| `vantage-mongodb` | `MongoDB` | ✅ | ✅ | ✅ | full CRUD + `count` |
| `vantage-surrealdb` | `SurrealDB` | ✅ | ✅ | ✅ | full CRUD + `count` |
| `vantage-sql/sqlite` | `SqliteDB` | ✅ | ✅ | ✅ | full CRUD + `count` |
| `vantage-sql/postgres` | `PostgresDB` | ✅ | ✅ | ✅ | full CRUD + `count` |
| `vantage-sql/mysql` | `MysqlDB` | ✅ | ✅ | ✅ | full CRUD + `count` |
| `vantage-api-client/rest` | `RestApi` | ✅ | ✅ | ✅ (`tests/yaml_factory.rs`) | `count` (read-only) |
| `vantage-api-client/graphql` | `GraphqlApi` | ✅ | ✅ | example only (`graphql_spacex.rs`) | `count` (read-only) |
| `vantage-log-writer` | `LogWriter` | ✅ | ✅ | ✅ | `insert` only (write-only) |
| `vantage-aws` (account) | `AwsAccount` | ❌ | ❌ | — | — |
| `vantage-aws/dynamodb` | `DynamoDB` | ❌ | ❌ | — | — |
| `vantage-redb` | `Redb` | ❌ | ❌ | — | — |
| `vantage-api-pool` | `PoolApi` | ❌ | ❌ | — | — |

## Cross-cutting gaps across the done drivers

The following are not driver-specific bugs — they're trait methods that
the default impl returns `Unimplemented`/`Unsupported` for, and every
shipped driver except where noted leaves them defaulted. Each is a
mechanical one-time fix per driver.

### `TableShell::get_ref` — only REST + GraphQL override it

`Vista::get_ref(relation)` calls into `TableShell::get_ref`, whose
default returns `Unimplemented`. CSV, MongoDB, SurrealDB, all three SQL
drivers, and LogWriter all fall through, even though the underlying
typed `Table<T, E>` they wrap has full `get_ref` / `get_ref_as` support
via the references registered on it.

```rust,ignore
let client_vista = sqlite_factory.from_table(Client::sqlite_table(db));
let orders = client_vista.get_ref("orders")?;   // ❌ Unimplemented today
```

The shape of the fix is the same everywhere: call `TableLike::get_ref`
on the wrapped typed table to obtain an `AnyTable`, then re-wrap it as
a `Vista` via the same driver factory (column metadata + capability set
already known). Doing this once per driver closes the biggest practical
Vista gap.

Open question: how does the re-wrap reach a `VistaFactory` from inside
`TableShell::get_ref`? Three options:

1. Store an `Arc<Factory>` on the shell at construction. Cheap, but
   each shell now holds a back-reference to its factory.
2. Free-function `vista_from_any_table(any_table, capabilities)` on
   the driver crate that doesn't need factory state. Simplest; matches
   what REST does today.
3. Stateless trait method on the factory keyed off the wrapped
   table's metadata.

Lean: option 2 — every driver already has the construction code; expose
it as a free function and call it from the shell's `get_ref`.

### `TableShell::add_raw_condition` — only REST overrides it

Used by YAML-driven cross-source reference traversal. The default
returns `Unimplemented`, blocking YAML-defined references that need
deferred FK conditions. REST is the only driver that overrides today;
every other driver needs the override before YAML-driven traversal
through their tables works.

### `paginate_kind` — universally `None`

Every factory uses `..VistaCapabilities::default()`, which sets
`paginate_kind: PaginateKind::None`. SQL drivers natively support
offset pagination; MongoDB has `skip`/`limit`; SurrealDB has both.
None of them advertise it.

Resolution waits for Stage 5b (`Vista::set_pagination`) — without a
universal setter, advertising `Offset` doesn't get a UI anywhere — but
the factory-side flip is trivial and should land in lockstep.

### `can_subscribe` — universally `false`

SurrealDB has native LIVE queries. MongoDB has change streams. Neither
advertises `can_subscribe: true` today. SurrealDB's CHANGELOG (0.4.5)
explicitly defers this to a later pass. Wiring belongs in Stage 7
(Coop's `with_live`), not here.

## Per-driver status

### Already shipped

#### vantage-csv — done (stage 3)

- [x] Read-only — `can_count: true`, all writes return `Unsupported`.
- [x] `csv:` table block carries `path`; `csv: { source }` per column for
      header rename.
- [x] `add_eq_condition` builds `column.eq(any_csv_value)` and pushes to
      `Table.add_condition` — the table's existing `apply_condition`
      machinery does the filtering.
- [x] Identity id (`String`) at the boundary; no translation needed.
- [x] Tests in `tests/vista.rs` and `tests/vista_yaml.rs`.
- [ ] **Cross-cutting**: implement `TableShell::get_ref`.

#### vantage-mongodb — done

- [x] ObjectId-as-id vs string fallback handled via `MongoId::FromStr` /
      `Display`. Decision: keep both `MongoId` variants for now; the
      "Drop the String variant from MongoId" TODO is independent.
- [x] **Condition delegation shipped early** (originally deferred to
      stage 5). `add_eq_condition` builds `doc!{path: bson_value}` and
      pushes to `Table.add_condition` — server-side filter push-down
      via Mongo's existing `find` filter. Vista carries no condition
      state.
- [x] `MongoVistaFactory` + `MongoTableShell` (read + write).
- [x] Driver extras: `mongo:` block with `collection` only.
- [x] Column extras: `mongo: { field }` for single-level rename and
      `mongo: { nested_path: address.city }` for nested-doc projection.
      `column_paths: IndexMap<String, Vec<String>>` on `MongoTableShell`
      drives read/write/filter consistently.
- [x] BSON ↔ CBOR bridge in `vista/cbor.rs` with unit tests.
- [x] Integration tests in `tests/6_vista.rs`.
- [ ] **Cross-cutting**: implement `TableShell::get_ref`.
- [ ] **Cross-cutting**: implement `TableShell::add_raw_condition` for
      cross-source YAML refs.

#### vantage-surrealdb — done (0.4.5)

- [x] `SurrealVistaFactory` + `SurrealTableShell`; `from_table` and
      `from_yaml` both produce a `Vista`. `from_table` preserves the
      entity type so `with_expression` closures still typecheck.
- [x] YAML `surreal:` block carries `table` and per-column `field`
      alias. `thing`/`record` column type maps to `Thing`; `datetime`
      and `decimal` round-trip via `AnySurrealType`.
- [x] String id boundary translates `"table:id"` straight to `Thing`;
      bare ids get prefixed with the wrapped table's name.
- [x] `add_eq_condition` translates `(field, CborValue)` into
      `column.eq(value)` and pushes to the wrapped table — `WHERE` is
      server-side.
- [x] Capabilities: `can_count`, `can_insert`, `can_update`,
      `can_delete` all true.
- [x] Integration tests in `tests/6_vista.rs`.
- [ ] **Cross-cutting**: implement `TableShell::get_ref`.
- [ ] **Cross-cutting**: implement `TableShell::add_raw_condition`.
- [ ] **Deferred**: `can_subscribe` + LIVE-query subscription — moves to
      Stage 7 (Coop's `with_live`).

#### vantage-sql/sqlite, postgres, mysql — done (0.4.4)

Wasn't on the original Stage 4 list, landed together as one drop.

- [x] `<db>::vista_factory()` returns a `<Db>VistaFactory`; each backend
      ships its own `*VistaSpec` / `*VistaFactory` / `*TableShell` triple
      under `mysql::vista`, `postgres::vista`, and `sqlite::vista`.
- [x] Full read/write/count capabilities and `eq` filtering through the
      existing typed-column path.
- [x] Backend-specific `sqlite:` / `postgres:` / `mysql:` blocks in the
      YAML spec for table and column name overrides.
- [x] `from_table` preserves the original entity type.
- [x] `driver_name` reports `"sqlite"` / `"postgres"` / `"mysql"` for
      diagnostics.
- [x] Integration tests in `tests/{sqlite,postgres,mysql}/6_vista.rs`.
- [ ] **Cross-cutting**: implement `TableShell::get_ref` (all three).
- [ ] **Cross-cutting**: implement `TableShell::add_raw_condition`
      (all three).
- [ ] **Cross-cutting**: once Stage 5b lands, advertise
      `paginate_kind: Offset`.

#### vantage-api-client/rest — done (0.1.4–0.1.6)

- [x] `RestApi::Value` is `ciborium::Value` end-to-end.
- [x] `RestApiVistaFactory` + `RestApiTableShell`. URI templates in
      table names substitute from eq-conditions at request time, letting
      `with_many`/`with_one` traversal hit nested REST endpoints
      natively.
- [x] `related_in_condition` implemented; `with_one` resolves the
      parent record on demand through a deferred condition executed at
      fetch time.
- [x] YAML-driven model registration via `register_yaml` +
      `with_model_resolver` for cross-driver setups.
- [x] **`TableShell::get_ref` is implemented** (unlike the other
      drivers).
- [x] **`TableShell::add_raw_condition` is implemented** (the
      cross-source reference case is what motivated the trait method
      in the first place).
- [x] `tests/yaml_factory.rs` exercises the YAML path.
- [ ] Capability honesty: REST is "read-only" via `can_count: true`
      only — `can_insert/update/delete` need a discussion on which
      verbs to wire to PUT/POST/PATCH/DELETE.

#### vantage-api-client/graphql — done (0.1.6)

Wasn't on the original Stage 4 list; landed alongside the REST work.

- [x] `GraphqlApi` — POST-based GraphQL data source. Renders typed
      query documents with inline filters and `$limit`/`$offset`
      variables. Two filter dialects: `Hasura` and `Generic`.
- [x] `GraphqlApiVistaFactory` — builds a Vista from typed
      `Table<GraphqlApi, E>` or YAML schema. YAML carries `graphql:`
      blocks for `root_field`, `dialect`, `filter_arg`.
- [x] Typed condition operators via `GraphqlOperation` (`.eq()`,
      `.ne()`, `.gt()`/etc., `.in_()`, `.like()`/`.ilike()`,
      `.is_null()`/`.is_not_null()`).
- [x] Relationship traversal via `with_many`/`with_one` with
      dialect-correct rendering.
- [x] `TableShell::get_ref` implemented.
- [x] `examples/graphql_spacex.rs` — YAML-driven CLI over the SpaceX
      public API.
- [ ] Integration tests under `tests/` (currently only example coverage).
- [ ] **Cross-cutting**: implement `TableShell::add_raw_condition` if
      cross-source YAML refs targeting GraphQL endpoints are wanted.

#### vantage-log-writer — done

Wasn't on the original Stage 4 list. Write-only sink driver — useful
test pattern for asymmetric capabilities.

- [x] `LogWriterVistaFactory` + `LogWriterTableShell`.
- [x] Capability: `can_insert: true` only; reads return `Unsupported`
      via `default_error`.
- [x] `add_eq_condition` is a no-op error (writes-only — narrowing has
      no meaning).
- [x] Integration tests in `tests/vista.rs`.

### Remaining

#### vantage-aws (account + dynamodb)

Two `TableSource` impls in this crate: `AwsAccount` (the account-level
service registry) and `DynamoDB` (the per-table driver). They likely
need separate Vista treatments — `AwsAccount` is a service catalogue,
not a queryable table — but the decision is part of the discussion.

- [ ] **Discuss**: does `AwsAccount` need a Vista at all, or only its
      sub-services (DynamoDB, S3, etc.)? Current `vantage-aws/examples/dynamo-single-table.rs`
      consumes `AwsAccount` directly via the older `AnyTable` route.
- [ ] **Discuss**: composite-key handling (partition + sort) — is the
      id a single CBOR map, or two separate boundary keys?
- [ ] **Discuss**: where the magic `array_key:service/target` table
      addressing moves (driver extras, not universal `table:`)
- [ ] **Discuss**: `narrow_via` field — into driver extras for
      references
- [ ] **Discuss**: AWS REST/JSON/XML protocol family — does each
      service need its own factory, or one factory with a service
      discriminator?
- [ ] Implement `AwsVistaFactory` + `AwsTableShell` (and/or
      `DynamoVistaFactory` + `DynamoTableShell`)
- [ ] Implement `TableShell::get_ref` per the cross-cutting pattern
- [ ] Implement `TableShell::add_raw_condition`
- [ ] Integration test against DynamoDB Local

#### vantage-redb

Embedded key/value store. New to the Vista rollout list.

- [ ] **Discuss**: redb is key/value with no native query language —
      conditions other than id lookup require client-side scan. Is
      `add_eq_condition` rejected at construction (refuse to advertise
      capability) or filled by Coop client-side?
- [ ] **Discuss**: range queries (redb's native predicate shape) — do
      they belong on Vista at all, or only as a redb-specific extension?
- [ ] **Discuss**: capability declaration — `can_count: true` is fine
      (full-table scan), but pagination is cheap (key-ordered), so
      this is the first driver where `paginate_kind: Cursor` is
      natural — wait for Stage 5b.
- [ ] Implement `RedbVistaFactory` + `RedbTableShell`
- [ ] Decide on schema: redb stores opaque bytes — what does
      `column_types()` return? Lean: a `cbor: …` per column extras
      block defining the layout, mirroring the mongodb nested-path
      pattern.
- [ ] Integration test

#### vantage-api-pool

`PoolApi` wraps a base `RestApi` with rate-limiting, retry, and
caching. It's a wrapper around another `TableSource` rather than a
primary backend.

- [ ] **Discuss**: does PoolApi need its own Vista, or is it strictly
      transparent — i.e. `RestApi`'s Vista observed through PoolApi
      "just works"? Lean: transparent, and PoolApi becomes one of the
      first real-world consumers of Coop (Stage 7) once Coop ships,
      since the rate-limit/retry/cache responsibilities are exactly
      what Coop's wrappers cover.
- [ ] If transparent: no Vista work needed; document the path in the
      crate README.
- [ ] If not transparent: full factory + shell rollout.

## References

- Subsumes:
  - `../../TODO.md` "MongoDB / CSV CBOR fidelity" — addressed natively
    per driver (no JSON middle step)
  - `../../TODO.md` "Add a sample CSV table implementation"
  - `../../FINAL_TODO.md` "In-process SQL-dialect-faithful mock" — out
    of scope here but driver pattern enables future addition
- Touches:
  - `../../TODO.md` "Drop the String variant from MongoId" — independent
  - `../../TODO.md` "Wire up real LIVE query support end-to-end" —
    deferred to stage 7
- Pairs with:
  - Stage 5b (query controls): `paginate_kind` honesty waits on
    `Vista::set_pagination`; same for `add_search` and `add_order`
    landing as overrides here.
  - Stage 7 (Coop): `with_upstream` / `with_writes` fills the
    write-side capabilities the read-only drivers honestly can't claim.
