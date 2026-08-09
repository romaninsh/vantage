# Changelog

## 0.6.0 — 2026-08-09

Initial release. Holds the `#[actions]` attribute macro, which
`vantage-action` re-exports — depend on `vantage-action` and reach the
attribute at `vantage_action::actions` rather than naming this crate.

It exists as a separate crate only because a `proc-macro` crate cannot
also export ordinary items.
