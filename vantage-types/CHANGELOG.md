# Changelog

## 0.6.0 — 2026-06-10

- New `TryIntoRecord` trait, the fallible counterpart to `TryFromRecord`. Serializing a serde
  entity into a `Record` now goes through `TryIntoRecord` and returns `Result` instead of
  panicking — so a failing `Serialize` (non-string map keys under JSON, a non-text CBOR key, an
  out-of-range number) becomes a recoverable error rather than a process abort deep inside a write
  path. CBOR map entries with non-text keys now error instead of being silently dropped.
- `IntoRecord` is now reserved for infallible conversions: type-system reshaping
  (`Record<T>` → `Record<U>`) and `#[entity]`-generated impls. The serde blanket impls moved to
  `TryIntoRecord`.
- `Entity` now requires `TryIntoRecord<Value>` instead of `IntoRecord<Value>`; the write paths
  (`insert` / `replace` / `patch` / `insert_return_id`) propagate the serialization error.

## 0.5.0 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.4.4 — 2026-05-23

- Doc comment refresh on the `Entity<ciborium::Value>` blanket — references the `Vista` boundary now
  that [vantage-table 0.5.2](https://docs.rs/vantage-table/0.5.2/vantage_table/) drops `AnyTable`.

## 0.4.3 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.2 — 2026-04-30

Catches up the crates.io release with the additions that landed locally in
[#214](https://github.com/romaninsh/vantage/pull/214) but never got their own version bump.

- New `RichText` / `Span` / `Style` types and a refactored `TerminalRender` trait —
  `render() -> RichText` instead of `render() -> String` plus a separate `color_hint()`. `Style` is
  semantic (`Default`, `Dim`, `Muted`, `Strong`, `Success`, `Error`, `Warning`, `Info`) — UI layers
  map to native presentation. `RichText` impls `Display` (writes the plain text), so existing
  string-shaped consumers keep compiling without code changes.
- Default `TerminalRender` impls migrated for `String`, `&str`, `i32` / `i64` / `f64`, `bool`,
  `Option<T>`, `serde_json::Value`, and `ciborium::Value`. Booleans render as `Style::Success` /
  `Style::Error`; nulls render as a dim em-dash.

## 0.4.1 — 2026-04-25

- `TerminalRender` impl for `ciborium::Value` so generic CLI/UI rendering keeps working when records
  flow through `AnyTable`.
- Blanket `From<ciborium::Value> for Record<ciborium::Value>` (and reverse), plus serde-blanket
  `IntoRecord<CborValue>` / `TryFromRecord<CborValue>` so any `Serialize + DeserializeOwned` entity
  auto-implements `Entity<CborValue>`.
