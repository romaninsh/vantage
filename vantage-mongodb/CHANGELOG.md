# Changelog

## 0.4.4 — 2026-04-25

- `From`/`Into<ciborium::Value>` impls on `AnyMongoType` so MongoDB tables can be wrapped via `AnyTable::from_table`. Round-trips via `serde_json::Value` (Bson and ciborium are both serde-friendly; same lossy bits as the existing JSON bridge).
- Pins `vantage-table = "0.4.4"` to keep the pair in lock-step.

## 0.4.3 — 2026-04-19

- Reference traversal now bridges `ObjectId` and `String` id-field boundaries via `related_in_condition`'s dual push.
- `impl From<MongoId> for AnyMongoType` so `c.id().eq(MongoId::parse(...))` dispatches to the right BSON type.
