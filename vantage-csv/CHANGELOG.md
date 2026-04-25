# Changelog

## 0.4.3 — 2026-04-25

- `From`/`Into<ciborium::Value>` impls on `AnyCsvType` so CSV tables can be wrapped via `AnyTable::from_table`. Round-trips via `serde_json::Value` (same lossy bits as the existing JSON bridge — binary, NaN, etc.).
- Pins `vantage-table = "0.4.4"` to keep the pair in lock-step.
