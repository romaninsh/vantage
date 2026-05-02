# Changelog

## 0.4.2 — 2026-04-30

Catches up the crates.io release with the additions that landed locally in [#214](https://github.com/romaninsh/vantage/pull/214) but never got their own version bump.

- New `RichText` / `Span` / `Style` types and a refactored `TerminalRender` trait — `render() -> RichText` instead of `render() -> String` plus a separate `color_hint()`. `Style` is semantic (`Default`, `Dim`, `Muted`, `Strong`, `Success`, `Error`, `Warning`, `Info`) — UI layers map to native presentation. `RichText` impls `Display` (writes the plain text), so existing string-shaped consumers keep compiling without code changes.
- Default `TerminalRender` impls migrated for `String`, `&str`, `i32` / `i64` / `f64`, `bool`, `Option<T>`, `serde_json::Value`, and `ciborium::Value`. Booleans render as `Style::Success` / `Style::Error`; nulls render as a dim em-dash.

## 0.4.1 — 2026-04-25

- `TerminalRender` impl for `ciborium::Value` so generic CLI/UI rendering keeps working when records flow through `AnyTable`.
- Blanket `From<ciborium::Value> for Record<ciborium::Value>` (and reverse), plus serde-blanket `IntoRecord<CborValue>` / `TryFromRecord<CborValue>` so any `Serialize + DeserializeOwned` entity auto-implements `Entity<CborValue>`.
