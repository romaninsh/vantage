# Changelog

All notable changes to `vantage-spacetimedb` are documented here.

## 0.1.0 — unreleased

Initial incubating crate: SpacetimeDB as a Vantage datasource. Excluded from the
workspace, alongside `vantage-aws` and `vantage-kubernetes`.

### Added

- `SpacetimeDb` connection handle addressing one database on one host. Clones share
  the inner state, because SpacetimeDB subscriptions are per-connection — every
  table of a database must reach the same handle or each would open its own socket.
- `ModuleSchema` introspection over `GET /v1/database/:db/schema`, normalising the
  two module-definition ABIs into one shape: tables, views, columns resolved out of
  the typespace, primary keys and unique constraints resolved from column indices,
  and reducer names.
- `RowIdentity` — primary key, else a single-column unique constraint, else a content
  hash of the row. The hash is sound here because SpacetimeDB deletes carry the whole
  row, so a delete hashes to the same id as its insert.
- `VistaMetadata` generation, flagging the id column, nominating the first non-id
  string column as the display title, and marking every column orderable (this
  driver sorts client-side, so there is no per-column server constraint).
- `SpacetimeDb::sql` and `SpacetimeDb::call_reducer` returning the host's raw JSON.
- Offline tests against schema fixtures captured verbatim from a live host, plus
  `#[ignore]`d live tests.

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
  identifier is worse than a textual one.
