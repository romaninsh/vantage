# Changelog

## 0.6.8 — 2026-08-09

- `Record::changes_from(&before)` returns the fields that differ — the
  partial record that turns `before` into `self`. Write this instead of
  a whole entity: a whole-entity write reverts fields another writer
  changed while the caller was working, and saves back values the caller
  never read, including any the entity failed to parse and filled with a
  default. A field that disappeared is not reported, because a merge
  cannot express a removal.
- The `Any<Trait>` wrapper that `vantage_type_system!` generates now
  implements `PartialEq`, comparing the stored representation only. Its
  `type_variant` is an inference about the value and differs by how the
  value was built, so comparing it would report changes where the stored
  bytes are identical.

## 0.6.7 — 2026-07-27

- `cbor_id_to_string` renders a CBOR value as a **record id** — a cache key, a
  traversal target, a selection identity. Deliberately separate from
  `cbor_to_string` and not a substitute for it: the `Option` is load-bearing.
  `None` means "not a plausible id" (`Null`, `Float`, `Bytes`, `Array`, `Map`)
  so a caller can skip the row, where `cbor_to_string` would answer `""` and
  key a cache entry under the empty string. `Tag(8, [table, id])` renders
  `table:id`; any other tag is transparent.
- `record_to_json` and `hex_encode` are re-exported at the crate root. Both were
  already public in `cbor_json`, so callers had to reach through the module path.

## 0.6.6 — 2026-07-24

- `Tag(12)` epoch pairs keep their exact shape through an untouched
  round trip: a one-element `[seconds]` no longer normalizes into
  `[seconds, 0]` (the hint's own bytes reproduce verbatim when the
  string matches its rendering). Malformed payloads — extra elements,
  non-integer members — fall back to the plain rendering instead of
  being silently coerced.

## 0.6.5 — 2026-07-24

- SurrealDB epoch-pair datetimes (`Tag(12, [seconds, nanos])`) join the
  CBOR↔JSON bridge: `PresentationDialect` renders them as a lossless
  nanosecond RFC-3339 string, and `json_to_cbor_with_hint` re-encodes an
  (edited) string back to the same tag — an untouched field reproduces
  identical bytes, killing the phantom "unsaved change" for datetime
  columns. The conversions are exported (`tag12_to_rfc3339` /
  `rfc3339_to_tag12`) so UI crates stop carrying their own copies.
  Adds a no-default-features `chrono` dependency behind the `serde`
  feature.

## 0.6.4 — 2026-07-23

- `json_to_cbor_with_hint(value, hint)` — a hint-aware inverse of the
  `PresentationDialect` renderings that `json_to_cbor` (forward-only) can't
  undo. Given the tracked CBOR as a shape hint, a `"table:id"` string restores
  to `Tag(8, [table, id])`, a text-carrying tag (datetime/uuid/decimal/duration)
  re-wraps, and a scalar string coerces to the hinted int/float/bool. Falls
  back to `json_to_cbor` with no hint, so it's a strict superset. Fixes the
  `CBOR → JSON → edit → CBOR` round-trip that otherwise left record-id and
  typed fields comparing as permanently changed (phantom "unsaved change").

## 0.6.3 — 2026-07-16

- `cbor_json` module (behind the `serde` feature): one shared CBOR↔JSON walker with
  per-backend rendering policy expressed as a `CborDialect` trait (hooks for tags, bytes,
  non-finite floats, integers beyond u64, and map keys). Includes total `json_to_cbor`
  and `cbor_to_string`, plus `PlainDialect` (neutral defaults) and `PresentationDialect`
  (SurrealDB record ids as `"table:id"`, NONE as null, datetime/UUID/decimal/duration
  text kept, binary UUIDs as hex). Replaces the divergent hand-written walkers and the
  tag-lossy serde round-trips previously scattered across the driver crates.
- `vantage_type_system!`'s generated `From<Value>` constructs the `Any*` type directly
  instead of routing through `from_*(&value).expect(...)` — one less clone and no dead
  panic path.

## 0.6.2 — 2026-06-25

- `InvariantValue` trait: a value type's null check (`is_null`) and equality (`value_eq`) — the two
  operations `vantage-table` needs to enforce set invariants. Implemented for `serde_json::Value`
  and `ciborium::Value`, and emitted by `vantage_type_system!` for every generated `Any*Type` with
  an optional `null_when:` pattern for the null check (omitted for non-nullable representations,
  which are never null).

## 0.6.1 — 2026-06-17

- Internal dependency realignment for the coordinated 0.6 release; no public API changes.

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
