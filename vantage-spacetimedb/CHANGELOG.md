# Changelog

All notable changes to `vantage-spacetimedb` are documented here.

## 0.1.0 — unreleased

Initial incubating crate: SpacetimeDB as a Vantage datasource. Excluded from the
workspace, alongside `vantage-aws` and `vantage-kubernetes`.

### Added

- `SpacetimeDb` connection handle addressing one database on one host. Clones share
  the connection state — host, database, token and HTTP client — but not a
  subscription socket: `subscribe` opens a connection per subscription. Sharing the
  state is the groundwork for a demultiplexer, since SpacetimeDB subscriptions are
  per-connection and keyed by query-set id, but that is not written yet.
- `ModuleSchema` introspection over `GET /v1/database/:db/schema`, normalising the
  two module-definition ABIs into one shape: tables, views, columns resolved out of
  the typespace, primary keys and unique constraints resolved from column indices,
  and reducer names.
- `RowIdentity` — primary key, else a single-column unique constraint, else a content
  hash of the row. The hash is sound here because SpacetimeDB deletes carry the whole
  row, so a delete hashes to the same id as its insert.
- `VistaMetadata` generation, flagging the id column and nominating the first non-id
  string column as the display title. No column is flagged orderable, because the
  dialect has no `ORDER BY` and this driver refuses to sort rather than sorting a
  materialised set — the flag agrees with `can_order` instead of contradicting it.
- `SpacetimeDb::sql` and `SpacetimeDb::call_reducer` returning the host's raw JSON.
- Row decoding, seeded by the schema. SQL rows arrive as **positional arrays**
  with sums encoded as `[variant_index, value]`, so `Some(1)` is `[0, 1]` and
  `None` is `[1, []]` — indistinguishable from a genuine two-element array
  without the schema. Decoding therefore goes through SATS's own seeded
  deserializer rather than walking `serde_json::Value`.
- `SpacetimeTableShell`: listing, keyed lookup, `get_some` via `LIMIT 1`,
  `COUNT(*)`, and conditions lowered into a real `WHERE`.
- `watch_vista` over the v2 BSATN WebSocket, with the vista's conditions pushed
  into the subscription query.
- Writes over SQL DML, keyed on the row identity, plus `SpacetimeDb::can_write`.
- Offline tests against schema fixtures captured verbatim from a live host, plus
  `#[ignore]`d live tests covering reads, the Vista facade and the change feed.

### Notes on behaviour

- **Schema v10 is preferred, v9 is the fallback.** `is_event` exists only in v10, so
  on a v9 host event tables are undetectable; that loss is reported through
  `ModuleSchema::event_detection()` and logged at warn, rather than guessed at.
- **Event tables are refused**, not rendered. Their rows are deleted in the same
  transaction that inserts them, so a grid over one is permanently empty and the
  insert/delete pair would be misread as an update.
- **Private tables and parameterised views are refused** with messages naming the
  cause, since both would otherwise present as an empty table.
- **Integers wider than 64 bits map to `string`**, because a silently truncated
  identifier is worse than a textual one. Relatedly, the change feed speaks BSATN
  rather than the older JSON protocol, which encodes every SATS integer as a JSON
  number and so loses precision above 2^53 — fatal for a `u64` primary key.
- **Nothing is emulated client-side.** Sorting, searching and pagination are
  refused rather than performed over a materialised set, so a `false` capability
  flag means the operation genuinely does not happen. Only the methods the server
  can answer are overridden; the rest keep the trait defaults, which report
  `Unsupported`.
- **Write capabilities follow database ownership, not token presence.**
  SpacetimeDB restricts SQL DML to the database owner, so a valid token belonging
  to any other identity is refused. The driver compares the `spacetime-identity`
  response header against `owner_identity`, and assumes read-only if it cannot
  tell — over-claiming a write is worse than under-claiming it.
- **Set membership on the change feed is the server's job.** Conditions go into
  the subscription query, and SpacetimeDB maintains it incrementally, so a row
  leaving the filtered set arrives as a delete. No per-notification re-read and no
  whole-set reload.
