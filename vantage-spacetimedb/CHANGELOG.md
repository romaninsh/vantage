# Changelog

All notable changes to `vantage-spacetimedb` are documented here.

## 0.8.0 — 2026-11-01

- Requires vantage-core 2.0.0 and vantage-vista 3.0.0.
- **Breaking:** `delete_all` needs `DeleteAll` rather than `Delete`. The vista advertises
  `DeleteAll` wherever it advertises `Delete`.

## 0.7.1 — 2026-10-31

- Requires vantage-vista 2.6.0. Inside an `atomic` block the SpacetimeDB vista writes directly and
  is reported as having no transactions.

## 0.7.0 — unreleased

### Breaking changes

- The shell implements `TableShell` 2.0.
- `IN`, `NOT IN` and `LIKE` are refused as `unsupported(FilterOps)`, so they are evaluated
  locally.
- Keyless tables advertise no writes. That includes `delete_all`.
- Writes through a narrowed handle are refused; `ConfineWrites` is not advertised.
- `fetch_window` serves only the first window, and `Window` is not advertised.
- `insert(Some(id))` for an id the set holds returns the stored row, and `insert(None)` is
  `Unsupported`.
- `replace` and `upsert` create a missing row; `replace` failed on one.
- An unknown column is "Unknown column for condition" with a `field` key; the `table` key is gone.
- A keyless row that fails to encode to BSATN is an error; it hashed to the empty byte string's
  id.
- A dotted path into a declared column is refused as `Unimplemented`.
- `preview()` is `{query, note}`; the driver name is `vista.driver_name()`.

### Other changes

- Vista 2.0 driver cleanup: builds against vantage-vista 2.0.0, through path dependencies.
- An update skips undeclared columns.
- An insert that races another re-reads the row; a replace that races is a `Conflict`.

Migration guide: https://vantage-ui.com/book/migration-2.0.html

## 0.6.0 — unreleased

Initial incubating crate: SpacetimeDB as a Vantage datasource. Excluded from the
workspace, alongside `vantage-aws` and `vantage-kubernetes`.

Starts at 0.6.0 rather than 0.1.0 because a new crate joins the family at the
family's version — `vantage-core` and `vantage-vista` are both 0.6.x, and the
`vantage-*` crates are versioned together rather than each from scratch.

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
- `VistaFactory` implementation, so a relation can be named in YAML and reached from
  a `VistaCatalog`, the CLI or a UI inventory. `load()` performs the schema fetch and
  the ownership probe up front, leaving `build_from_spec` synchronous — which the
  trait and `ModelLoader` both require. A spec contributes the name, optionally the
  relation behind it, and optionally a narrowed column list; anything it declares
  that this driver cannot lower (references, contained relations, computed columns,
  a contradicting `id_column`) is refused rather than ignored.
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
