# Changelog

## 0.6.1 — 2026-07-02

- `PulseSim`: a generic, config-driven "live aggregate feed". One shared run loop drives three
  coupled tables — a raw `Feed` append log (`{key, delta, updated}`, newest-first, expiring after
  `feed_retention` so the stream visibly flows; `Inserted`/`Deleted`), a derived `Aggregate`
  keyed-upsert (`{key, value, vs_baseline, live}`; `Updated`), and a `Minutes` arrivals time series
  (`{minute, attendees}`, one bucket per `bucket` window, summing only arrivals/positive deltas,
  kept to the last `minutes_window` buckets) — so a subscribed Dio applies changes in place. Per-key
  values mean-revert within a rubber-banded ±`band_pct` of a configured baseline; each key re-fires
  on its own random interval (bursts, never a whole-interval sleep); designated keys periodically
  blip offline. Keys, baselines, rates, retention, buckets, column names, and offline set are all
  config (`PulseConfig`).

## 0.6.0 — 2026-07-01

- Initial release. Synthetic, optionally-live datasource for Vantage.
- `StaticEffect` generates rows once; `FifoEffect` inserts newest-first and expires each after a
  random retention, broadcasting `ChangeEvent`s so a subscribed Dio animates inserts/expiries
  without re-listing.
- Name-aware value generation via the `fake` crate (email, name, phone, city, …) with a type
  fallback (int, decimal, bool, datetime, string).
- `fifo_cli` and `scenery_cli` examples.
