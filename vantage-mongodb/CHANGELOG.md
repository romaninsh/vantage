# Changelog

## 0.4.5 — 2026-05-04

New opt-in [`vista`](https://docs.rs/vantage-mongodb/0.4.5/vantage_mongodb/struct.MongoVistaFactory.html) feature: build a [`Vista`](https://docs.rs/vantage-vista/0.4.2/vantage_vista/struct.Vista.html) from a typed `Table<MongoDB, E>` or from YAML, with full read+write CRUD, server-side `eq` filtering, and nested-document column projection.

- `MongoDB::vista_factory()` returns a [`MongoVistaFactory`](https://docs.rs/vantage-mongodb/0.4.5/vantage_mongodb/struct.MongoVistaFactory.html); `from_table` and `from_yaml` both produce a `Vista`.
- YAML `mongo:` block carries `collection`. Per-column `mongo: { field }` renames a single BSON key, `mongo: { nested_path: address.city }` projects out of nested sub-documents — reads walk the path, writes rebuild sibling sub-docs, filters use dot-notation.
- BSON ↔ CBOR bridge in `vista::cbor`. Lossy paths (`ObjectId`, `DateTime`, `Decimal128`, `Regex`) collapse to their string representation; documented in module docs.
- Capabilities: `can_count`, `can_insert`, `can_update`, `can_delete` all true. `can_subscribe` deferred to change-streams work.
- YAML validation: empty `nested_path: ""` and empty segments (`a..b`, `.a`, `a.`) now error at spec load with the offending column named, so the mistake doesn't surface later as a malformed BSON filter.
- Off by default; non-vista users still don't depend on `vantage-vista`.

## 0.4.4 — 2026-04-25

- `From`/`Into<ciborium::Value>` impls on `AnyMongoType` so MongoDB tables can be wrapped via `AnyTable::from_table`. Round-trips via `serde_json::Value` (Bson and ciborium are both serde-friendly; same lossy bits as the existing JSON bridge).
- Pins `vantage-table = "0.4.4"` to keep the pair in lock-step.

## 0.4.3 — 2026-04-19

- Reference traversal now bridges `ObjectId` and `String` id-field boundaries via `related_in_condition`'s dual push.
- `impl From<MongoId> for AnyMongoType` so `c.id().eq(MongoId::parse(...))` dispatches to the right BSON type.
