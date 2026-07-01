# Changelog

## 0.6.0 — 2026-07-01

- Initial release. Synthetic, optionally-live datasource for Vantage.
- `StaticEffect` generates rows once; `FifoEffect` inserts newest-first and expires each after a
  random retention, broadcasting `ChangeEvent`s so a subscribed Dio animates inserts/expiries
  without re-listing.
- Name-aware value generation via the `fake` crate (email, name, phone, city, …) with a type
  fallback (int, decimal, bool, datetime, string).
- `fifo_cli` and `scenery_cli` examples.
