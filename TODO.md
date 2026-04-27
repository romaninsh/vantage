# MongoDB PoC & Trait Boundary Improvements (from MongoDB work, 2026-04)

## MongoDB reference traversal

- [x] **Make `with_one` / `with_many` usable across `ObjectId` / `String` id-field boundaries**
      — `related_in_condition` now pushes both the raw value and its alternate representation
      (ObjectId's hex string, or the parsed ObjectId of a hex-string value) into the `$in`
      filter, so traversal works whether the target's FK is stored as `ObjectId` or as a
      plain string. Also added `impl From<MongoId> for AnyMongoType` so user-land narrowing
      by id (`c.id().eq(MongoId::parse(...))`) dispatches to the right BSON type.
- [ ] **Drop the `String` variant from `MongoId`** — commit the framework to `ObjectId`-only
      ids and lean on Mongo's native convention. Simplifies `id.rs`, removes the dual-push
      hack in `related_in_condition`, and drops the smart-parse paths added to
      `AnyMongoType`/`MongoId`. Blast radius: `scripts/db/v2.js` seeds string `_id`s
      (`"hill_valley"`, `"order1"`, etc.) and `tests/5_references.rs` asserts them directly —
      both need rewriting to real ObjectIds. Users who genuinely want string-keyed documents
      can model them in a non-`_id` field. ~0.5–1 day including test fixture rewrite.

## Trait boundary fixes needed

- [ ] **Move `get_count`/`get_sum`/`get_max`/`get_min` off `SelectableDataSource`** — currently in
      `table/impls/selectable.rs` behind `T: SelectableDataSource`. They just delegate to
      `TableSource` methods. Move to a separate impl block requiring only `T: TableSource` so
      MongoDB and other non-query backends can use them directly.
- [x] **Remove `delete`/`delete_all` from `WritableDataSet`** — `WritableValueSet` is the canonical
      place for deletion (doesn't require entity type). Having both causes ambiguity when calling
      `table.delete()`. Keep only in `WritableValueSet`.
- [x] **Change `ReadableDataSet::get(id)` to return `Result<Option<E>>`** — went with the full
      contract change. `ReadableValueSet::get_value` and the per-backend `get_table_value`
      helpers flipped the same way. `ActiveEntitySet::get_entity` used to swallow errors as
      `Ok(None)` — now propagates them. axum tutorial's `contains("no row found")` hack gone.
- [ ] **Decouple `column_table_values_expr` from `ExprDataSource`** — the method returns
      `AssociatedExpression` which forces `ExprDataSource` dependency. Consider moving to a
      sub-trait so non-SQL backends don't carry dead code. SQL backends use it internally in
      `related_in_condition`; MongoDB never touches it.
- [ ] **Explore `Selectable` parameterized on condition type** — currently `add_where_condition`
      takes `impl Expressive<T>`, hardcoding Expression-based conditions. MongoDB could implement
      its own `select()` if `Selectable` (or a parallel trait) accepted `Condition` type directly.

## Cleanup (lower priority)

- [ ] **Remove `From<Expression<AnyMongoType>> for MongoCondition` panic impl** — exists only to
      satisfy trait bounds. Could be eliminated by separating the `resolve_as_any` bounds or
      splitting `with_one`/`with_many` bounds from the `Reference` impl bounds.
- [ ] **Consider removing `related_in_condition` from `TableSource`** — now only used by
      `Table::get_ref_as` (same-backend resolution). Could be moved into the `HasOne`/`HasMany`
      `resolve_as_any` implementations directly, removing it from the trait surface.

# Type System — missing entity-level impls

- [ ] `Vec<u8>` — binary data (BLOB/BYTEA/BLOB), bind/read paths already exist, needs `impl XxxType`
- [ ] `Uuid` — Postgres has native UUID column + variant, MySQL uses CHAR(36); `uuid` crate

# Query Builder Improvements (from MySQL work, 2026-04)

- [x] `expr.as_alias()` — `AliasExt` blanket impl in vantage-sql, removed `Option<String>` from
      `Selectable::with_expression`, stripped alias from all primitives (Fx, Iif, Concat,
      GroupConcat, JsonExtract, DateFormat, Case, Ternary). Fixes Fx/Case hardcoded `"` quoting.
- [ ] `sql_fx!()` macro — mixed-type args for function calls:
      `sql_fx!("find_in_set", "write", (ident("permissions")))` instead of wrapping every arg in
      `mysql_expr!`
- [ ] PostgreSQL ingress — split into `vantage_v2`, `vantage_v3`, `vantage_v4_pg` with DROP+CREATE,
      matching MySQL pattern
- [ ] `Expression::empty()` sweep — replace all `Expression::new("", vec![])` across the codebase

# SurrealDB

- [ ] Implement `only_column()` method for SurrealSelect query builder
- [ ] **BUG**: SurrealDB IN subquery returns record objects not scalar values
  - Reference traversal generates `WHERE bakery IN (SELECT id FROM bakery WHERE ...)`
  - SurrealDB returns `{id: "bakery:hill_valley"}` from subquery, not `"bakery:hill_valley"`
  - Need `SELECT VALUE id` but that's SurrealDB-specific, not in generic Selectable trait
  - Affects: Reference traversal in bakery_model4 (e.g., `bakery ref products list`)
- [ ] **Wire up real LIVE query support end-to-end.** `surreal-client/src/engines/ws_cbor.rs`
      has a comment claiming LIVE was added, but `handle_messages` only routes responses with
      a matching pending-request id. LIVE notifications arrive as `Binary` frames whose id
      references a live-query UUID with no pending request — they're silently dropped today.
      Full plumbing needs (rough order):

  - [ ] **`surreal-client`**: detect notification frames in `WsCborEngine::handle_messages`
        and route to per-live-query channels. Add `Client::live_select(query) -> impl Stream<
        Item = Notification>` (or similar). Drop semantics: cancelling the stream should send
        a `KILL <uuid>` so the server stops emitting. Mirror impl in `WsEngine` (text JSON).
        Patch bump to surreal-client.
  - [ ] **`vantage-surrealdb`**: new `SurrealLiveStream` (gated behind a `live` feature) that
        implements `vantage_live::LiveStream`. Subscribes via the new surreal-client method
        and translates `Notification { action: CREATE | UPDATE | DELETE, … }` into vantage
        `LiveEvent::{Inserted, Updated, Deleted}`. Adds `vantage-live` as an optional dep.
        Patch bump.
  - [ ] **`vantage-live` demo (`examples/live_demo.rs`)**: replace the `local` redb-as-master
        subcommand with a `bakery` mode using `bakery_model3`'s SurrealDB tables. Expose all
        four entities (`bakery`, `clients`, `products`, `orders`) as subcommands. Drop
        redb-as-master entirely from the example. Add `--watch` and `--timeout <secs>` flags
        on `list`: poll on a 1s tick (cache-served when warm) AND consume the SurrealLiveStream
        in the background so external mutations land immediately as cache invalidations →
        next poll re-fetches.
  - [ ] **Helper script** at `bakery_model3/scripts/insert-client-every-second.sh`: bash loop
        that uses `surreal sql` against the bakery namespace to insert a fresh client every
        second. Lets you run the watch demo in one terminal and the helper in another to see
        cache invalidation fire from real LIVE events.
  - [ ] **CHANGELOG entries** in surreal-client / vantage-surrealdb / vantage-live (new
        feature in the demo).

  Future-universal note: the `LiveStream` trait currently lives in `vantage-live`. If more
  backends grow live-event support (Postgres LISTEN/NOTIFY, Mongo change streams, Kafka),
  consider lifting the trait into a lower-level crate so backends can implement it without
  taking vantage-live as a dep.

# CI/CD

- [ ] **Automate crate publishing in CI** — add a workflow that publishes crates to crates.io
      on tag/release, in dependency order. Require version bump (reject if version matches
      what's already on crates.io).
- [ ] **Rebuild book on Cargo.toml changes** — the book workflow currently only triggers on
      `docs4/**` changes. Version bumps update rustdoc links, so the book should also rebuild
      when any `Cargo.toml` in the workspace changes.

# Architecture

- [ ] **Make `ImTable` / `ImDataSource` generic over `Value`** — currently hardcoded to
      `serde_json::Value` (`vantage-dataset/src/im/im_table.rs:28`,
      `vantage-dataset/src/im/mod.rs:19`). It's the canonical "schema-less ValueSet" reference
      impl people copy, so locking it to JSON undersells the persistence-native type story.
      Parameterise as `ImTable<V, E>` (default `V = ciborium::Value` after AnyTable swap), or
      at least switch the default to CBOR. Not urgent — internal tests + prototyping use only.
- [ ] Refactor Expressions — split out "Owned" and "Lazy" expressions, use dyn/into patterns

# AnyTable CBOR-swap follow-up

The 0.4 swap of `AnyTable`'s carrier from `serde_json::Value` to `ciborium::Value`
left a few items deferred (chosen over yak-shaving the test fixtures):

- [ ] **Convert `MockTableSource` to `Value = ciborium::Value`** — currently still uses
      `serde_json::Value` (`vantage-table/src/mocks/mock_table_source.rs:108`). It bridges to
      `ImTable` (which is JSON-hardcoded — see above), so the conversion needs internal
      JSON↔CBOR shims at the trait-impl boundary OR can wait for the `ImTable` generification.
      Until then, `AnyTable::new(mock_table)` won't compile (you can use `from_table` once a
      JSON↔CBOR `From`/`Into` is provided for `serde_json::Value`).
- [ ] **Restore `vantage-table/tests/table_like.rs`** — disabled in `Cargo.toml` during the
      swap. The four integration tests use `AnyTable::new(MockTableSource)` which needs the
      mock conversion above. Once `MockTableSource` switches to CBOR, re-enable by removing
      the `[[test]] test = false` block.
- [ ] **Restore inline `AnyTable` tests in `vantage-table/src/any.rs`** — same reason; the
      inline tests were dropped (see comment in `any.rs` where the `tests` module was). The
      original tests covered creation/downcast/is_type/debug; resurrect from git history when
      the mock is ready.
- [ ] **`bakery_model4` sweep** — `bakery_model4` is excluded from the workspace; its example
      code (especially `examples/cli4.rs`) probably uses `AnyTable` and will need the same
      kind of CBOR conversions that `bakery_model3` got (see the JSON-to-CBOR shim added in
      `bakery_model3/examples/cli.rs:271`). Ditto for any custom `TerminalRender` calls — the
      framework now ships `impl TerminalRender for ciborium::Value` so most consumers should
      Just Work, but verify.
- [ ] **MongoDB / CSV CBOR fidelity** — both backends added `From<CborValue>`/`Into<CborValue>`
      via a `serde_json::Value` round-trip (`vantage-mongodb/src/types/value.rs`,
      `vantage-csv/src/type_system.rs`). This loses the same bits JSON loses (binary blobs,
      tags, NaN). Acceptable for now (MongoDB users typically don't store CBOR-only types in
      AnyTable round-trips), but a direct BSON↔CBOR path would be more honest if these become
      real workloads.
- [ ] Implement transaction support
- [ ] `returning id` should properly choose ID column
- [ ] `with_id()` shouldn't need `into()`
- [ ] Add a sample CSV table implementation
- [ ] Table::join_table should preserve conditions on other_table
- [ ] Table::join_table should resolve clashes in table aliases
- [ ] Condition::or() shouldn't be limited to only two arguments

# Someday maybe

- [ ] Implement associated records (update and save back)
- [ ] Implement table aggregations (group by)
- [ ] Implement RestAPI support
- [ ] Implement Queue support
- [ ] Add expression as a field value (e.g. when inserting)
- [ ] All persistences should implement idempotent CRUD — `insert()` with duplicate ID should
      succeed silently (INSERT OR IGNORE / ON CONFLICT DO NOTHING). Currently only `replace()`
      and `delete()` are idempotent.
- [ ] Explore replayability for idempotent operations and workflow retries
- [ ] Implement and Document Disjoint Subtypes pattern
- [ ] Implement "Realworld" example application in a separate repository
- [ ] In-memory cache layer with transparent invalidation
- [ ] Cross-datasource operations (business logic agnostic to storage backend)
