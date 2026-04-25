# Changelog

## 0.4.1 — 2026-04-25

- `TerminalRender` impl for `ciborium::Value` so generic CLI/UI rendering keeps working when records flow through `AnyTable`.
- Blanket `From<ciborium::Value> for Record<ciborium::Value>` (and reverse), plus serde-blanket `IntoRecord<CborValue>` / `TryFromRecord<CborValue>` so any `Serialize + DeserializeOwned` entity auto-implements `Entity<CborValue>`.
